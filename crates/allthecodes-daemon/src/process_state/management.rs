use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::{operation_lock, protocol, readiness};

use super::paths::{daemon_dir, health_url, state_path, worker_log_path};
use super::platform::{configure_detached, process_matches_record};
use super::storage::{
    cleanup_stale_state_before_start, clear_sleep_state, ensure_daemon_dir,
    list_bridge_session_states, read_bridge_session_state, read_control_token, read_worker_state,
    request_shutdown, status_snapshot, tail_log, write_sleep_state, write_stopped,
};
use super::types::{
    DaemonBridgeSessionState, DaemonProcessState, DaemonStatusSnapshot, StaleStateCleanupReport,
};

const SHUTDOWN_REQUEST_GRACE_PERIOD: Duration = Duration::from_secs(60);
const TERMINATE_GRACE_PERIOD: Duration = Duration::from_secs(5);
const FINAL_EXIT_GRACE_PERIOD: Duration = Duration::from_secs(10);

pub fn try_run_management_command(args: &[String], cwd: &Path, port: u16) -> Option<ExitCode> {
    if args.first().map(String::as_str) != Some("daemon") {
        return None;
    }

    let subcommand = args.get(1).map(String::as_str).unwrap_or("status");
    let code = match subcommand {
        "start" => print_result(operation_lock::with_operation_lock("start", cwd, || {
            start_daemon(args, cwd, port)
        })),
        "status" => print_result(operation_lock::with_operation_lock(
            "status",
            cwd,
            print_status,
        )),
        "stop" => print_result(operation_lock::with_operation_lock(
            "stop",
            cwd,
            stop_daemon,
        )),
        "restart" => print_result(operation_lock::with_operation_lock("restart", cwd, || {
            restart_daemon(args, cwd, port)
        })),
        "logs" => print_result(print_logs(args)),
        "submit" => print_result(submit_worker_command(args)),
        "abort" => print_result(abort_worker_command()),
        "command" => print_result(print_worker_command(args)),
        "events" => print_result(print_worker_events(args)),
        "token" => print_result(print_control_token()),
        "bridge" => print_result(run_bridge_command(args, cwd)),
        "sleep" => print_result(operation_lock::with_operation_lock("sleep", cwd, || {
            schedule_sleep_command(args)
        })),
        "wake" => print_result(operation_lock::with_operation_lock("wake", cwd, || {
            wake_daemon_command()
        })),
        "help" | "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown daemon subcommand: {other}");
            print_usage();
            ExitCode::FAILURE
        }
    };
    Some(code)
}

fn start_daemon(args: &[String], cwd: &Path, fallback_port: u16) -> Result<()> {
    let cleanup = cleanup_stale_state_before_start()?;
    print_stale_cleanup_report(&cleanup);

    match status_snapshot()? {
        DaemonStatusSnapshot::Running(state) => {
            println!(
                "daemon already running: pid={} health={}",
                state.pid, state.health_url
            );
            return Ok(());
        }
        DaemonStatusSnapshot::Stale(state) => anyhow::bail!(
            "daemon state is stale for pid={} and could not be cleaned",
            state.pid
        ),
        DaemonStatusSnapshot::Stopped => {}
    }

    if !daemon_start_feature_enabled() {
        anyhow::bail!("daemon start requires FEATURE_KAIROS=1 or FEATURE_PROACTIVE=1");
    }

    ensure_daemon_dir()?;
    let port = parse_port(args).unwrap_or(fallback_port);
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let log_path = daemon_dir().join("supervisor.log");
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open daemon log {}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .with_context(|| format!("failed to clone daemon log {}", log_path.display()))?;

    let mut cmd = Command::new(exe);
    cmd.arg("--daemon")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    configure_detached(&mut cmd);
    let child = cmd.spawn().context("failed to spawn daemon supervisor")?;
    let ready_url = readiness::ready_url(port);
    let readiness = readiness::wait_for_ready(port).with_context(|| {
        format!(
            "daemon start failed readiness check: pid={} ready={} log={}",
            child.id(),
            ready_url,
            log_path.display()
        )
    })?;
    println!(
        "daemon started: pid={} health={} ready={} attempts={} elapsed_ms={} log={}",
        child.id(),
        health_url(port),
        ready_url,
        readiness.attempts,
        readiness.elapsed.as_millis(),
        log_path.display()
    );
    Ok(())
}

pub(super) fn daemon_start_feature_enabled() -> bool {
    allthecodes_config::features::enabled(allthecodes_config::features::Feature::Kairos)
        || allthecodes_config::features::enabled(allthecodes_config::features::Feature::Proactive)
}

fn stop_daemon() -> Result<()> {
    let snapshot = status_snapshot()?;
    let DaemonStatusSnapshot::Running(state) = snapshot else {
        println!("daemon is not running");
        return Ok(());
    };

    request_shutdown("daemon stop command")?;
    if wait_until_not_matching(&state, SHUTDOWN_REQUEST_GRACE_PERIOD)? {
        println!("daemon stopped: pid={}", state.pid);
        return Ok(());
    }

    super::platform::send_soft_terminate(state.pid, state.process_start_key.as_deref())?;
    if wait_until_not_matching(&state, TERMINATE_GRACE_PERIOD)? {
        println!("daemon stopped after terminate: pid={}", state.pid);
        return Ok(());
    }

    super::platform::send_force_kill(state.pid, state.process_start_key.as_deref())?;
    if wait_until_not_matching(&state, FINAL_EXIT_GRACE_PERIOD)? {
        println!("daemon killed after timeout: pid={}", state.pid);
        return Ok(());
    }

    anyhow::bail!(
        "daemon pid={} still appears alive after force kill; {}",
        state.pid,
        process_matches_record(state.pid, state.process_start_key.as_deref()).as_diagnostic()
    )
}

fn wait_until_not_matching(state: &DaemonProcessState, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let identity = process_matches_record(state.pid, state.process_start_key.as_deref());
        if !identity.is_current_process_record() {
            match identity {
                super::types::ProcessIdentityStatus::Dead => {
                    write_stopped(state.port, &state.cwd)?;
                }
                super::types::ProcessIdentityStatus::Mismatched { .. } => {
                    let mut stale = state.clone();
                    stale.status = super::types::DaemonRunStatus::Stale;
                    stale.updated_at = chrono::Utc::now();
                    super::storage::write_state(&stale)?;
                }
                _ => {}
            }
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(false)
}

fn restart_daemon(args: &[String], cwd: &Path, port: u16) -> Result<()> {
    if args.iter().any(|arg| arg == "--if-version-changed") {
        if let DaemonStatusSnapshot::Running(state) = status_snapshot()? {
            let ready_ok = readiness::probe_ready(state.port, Duration::from_millis(500)).is_ok();
            let running_version = state.binary_version.as_deref().unwrap_or("unknown");
            let current_version = env!("CARGO_PKG_VERSION");
            let log_path = state
                .log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| daemon_dir().join("supervisor.log").display().to_string());
            if running_version == current_version && ready_ok {
                let ready_url = state
                    .ready_url
                    .clone()
                    .unwrap_or_else(|| readiness::ready_url(state.port));
                println!(
                    "daemon restart skipped: version={} pid={} ready={} log={} {}",
                    running_version,
                    state.pid,
                    ready_url,
                    log_path,
                    process_matches_record(state.pid, state.process_start_key.as_deref())
                        .as_diagnostic()
                );
                return Ok(());
            }
            println!(
                "daemon restart required: running_version={} current_version={} readiness={} pid={} log={}",
                running_version,
                current_version,
                if ready_ok { "ok" } else { "error" },
                state.pid,
                log_path
            );
        }
    }
    stop_daemon()?;
    start_daemon(args, cwd, port)
}

fn print_logs(args: &[String]) -> Result<()> {
    let target = args
        .get(2)
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str)
        .unwrap_or("supervisor");
    let tail_bytes = parse_tail_bytes(args)?;
    let path = if target == "supervisor" {
        daemon_dir().join("supervisor.log")
    } else {
        read_worker_state(target)?
            .map(|state| state.log_path)
            .unwrap_or_else(|| worker_log_path(target))
    };
    print!("{}", tail_log(&path, tail_bytes)?);
    Ok(())
}

fn parse_tail_bytes(args: &[String]) -> Result<Option<usize>> {
    match args
        .windows(2)
        .find(|pair| pair[0] == "--tail-bytes")
        .map(|pair| pair[1].parse::<usize>())
    {
        Some(Ok(bytes)) => Ok(Some(bytes)),
        Some(Err(err)) => Err(anyhow::anyhow!("--tail-bytes must be an integer: {err}")),
        None => Ok(None),
    }
}

fn submit_worker_command(args: &[String]) -> Result<()> {
    require_running_daemon()?;
    let text = args
        .get(2..)
        .unwrap_or_default()
        .join(" ")
        .trim()
        .to_string();
    if text.is_empty() {
        anyhow::bail!("daemon submit requires text");
    }

    clear_sleep_state()?;
    let command = crate::protocol_store().enqueue_command(
        crate::supervisor::ASSISTANT_WORKER_ID,
        protocol::DaemonCommandKind::Submit,
        serde_json::json!({ "text": text }),
        None,
    )?;
    println!(
        "daemon command queued: id={} worker={} kind=submit",
        command.command_id, command.target_worker_id
    );
    Ok(())
}

fn abort_worker_command() -> Result<()> {
    require_running_daemon()?;
    let command = crate::protocol_store().enqueue_command(
        crate::supervisor::ASSISTANT_WORKER_ID,
        protocol::DaemonCommandKind::Abort,
        serde_json::json!({}),
        None,
    )?;
    println!(
        "daemon command queued: id={} worker={} kind=abort",
        command.command_id, command.target_worker_id
    );
    Ok(())
}

fn print_worker_command(args: &[String]) -> Result<()> {
    let Some(command_id) = args.get(2) else {
        anyhow::bail!("daemon command requires a command id");
    };
    let worker_id = args
        .get(3)
        .map(String::as_str)
        .unwrap_or(crate::supervisor::ASSISTANT_WORKER_ID);
    let Some(command) = crate::protocol_store().read_command(worker_id, command_id)? else {
        anyhow::bail!("daemon command not found: {command_id}");
    };
    println!("{}", serde_json::to_string_pretty(&command)?);
    Ok(())
}

fn print_worker_events(args: &[String]) -> Result<()> {
    let worker_id = args
        .get(2)
        .map(String::as_str)
        .unwrap_or(crate::supervisor::ASSISTANT_WORKER_ID);
    let events = crate::protocol_store().read_worker_events(worker_id)?;
    if events.is_empty() {
        println!("daemon events: none for worker={worker_id}");
        return Ok(());
    }
    for event in events {
        println!("{}", serde_json::to_string(&event)?);
    }
    Ok(())
}

fn print_control_token() -> Result<()> {
    let Some(token) = read_control_token()? else {
        anyhow::bail!("daemon control token is not available");
    };
    println!("{}", token.token);
    Ok(())
}

fn schedule_sleep_command(args: &[String]) -> Result<()> {
    require_running_daemon()?;
    let seconds = args
        .get(2)
        .with_context(|| "daemon sleep requires seconds")?
        .parse::<u64>()
        .with_context(|| "daemon sleep seconds must be a positive integer")?;
    if !(1..=3600).contains(&seconds) {
        anyhow::bail!("daemon sleep seconds must be between 1 and 3600");
    }
    let reason = args.get(3..).unwrap_or_default().join(" ");
    let state = write_sleep_state(seconds, &reason)?;
    println!(
        "daemon sleeping until {}",
        state.sleeping_until.to_rfc3339()
    );
    Ok(())
}

fn wake_daemon_command() -> Result<()> {
    clear_sleep_state()?;
    println!("daemon sleep cleared");
    Ok(())
}

fn run_bridge_command(args: &[String], cwd: &Path) -> Result<()> {
    let subcommand = args.get(2).map(String::as_str).unwrap_or("sessions");
    match subcommand {
        "sessions" => print_bridge_sessions(),
        "status" => {
            if let Some(session_id) = args.get(3) {
                print_bridge_session_status(session_id)
            } else {
                print_bridge_sessions()
            }
        }
        "resume" => {
            let session_id = args
                .get(3)
                .with_context(|| "daemon bridge resume requires a session id")?;
            let session = resume_bridge_session_command(session_id)?;
            println!("bridge session resumed: {}", session.session_id);
            print_bridge_session_detail(&session);
            Ok(())
        }
        "new" => {
            let session = new_bridge_session_command(cwd)?;
            println!("bridge session created: {}", session.session_id);
            print_bridge_session_detail(&session);
            Ok(())
        }
        "release" => {
            let session_id = args
                .get(3)
                .with_context(|| "daemon bridge release requires a session id")?;
            release_bridge_session_command(session_id)?;
            println!("bridge session released: {session_id}");
            if let Some(session) = read_bridge_session_state(session_id)? {
                print_bridge_session_detail(&session);
            }
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_bridge_usage();
            Ok(())
        }
        other => {
            anyhow::bail!("unknown daemon bridge subcommand: {other}");
        }
    }
}

fn print_bridge_sessions() -> Result<()> {
    let sessions = list_bridge_session_states()?;
    println!("bridge sessions: {}", sessions.len());
    for session in &sessions {
        println!(
            "  {} cwd={} assistant={} remote={} run={} lease={}",
            session.session_id,
            session.cwd.display(),
            optional_bridge_value(session.assistant_session_id.as_deref()),
            optional_bridge_value(session.remote_session_key.as_deref()),
            optional_bridge_value(session.last_run_id.as_deref()),
            bridge_lease_summary(session),
        );
    }
    Ok(())
}

fn print_bridge_session_status(session_id: &str) -> Result<()> {
    let session = read_bridge_session_state(session_id)?
        .with_context(|| format!("bridge session not found: {session_id}"))?;
    print_bridge_session_detail(&session);
    Ok(())
}

fn resume_bridge_session_command(session_id: &str) -> Result<DaemonBridgeSessionState> {
    let state = read_bridge_session_state(session_id)?
        .with_context(|| format!("bridge session not found: {session_id}"))?;
    let lease_owner = state
        .lease_owner
        .clone()
        .unwrap_or_else(bridge_management_lease_owner);
    crate::bridge_session::select_or_create_bridge_session(
        bridge_identity_from_state(&state),
        crate::bridge_session::BridgeSessionReusePolicy::ExplicitSession(session_id.to_string()),
        crate::bridge_session::BridgeSessionLease {
            owner: lease_owner,
            ttl: Duration::from_secs(30),
            allow_stale_takeover: true,
        },
    )
}

fn new_bridge_session_command(cwd: &Path) -> Result<DaemonBridgeSessionState> {
    crate::bridge_session::select_or_create_bridge_session(
        crate::bridge_session::BridgeSessionIdentity {
            cwd: cwd.to_path_buf(),
            account_id: None,
            profile: None,
            terminal_id: None,
            remote_session_key: None,
        },
        crate::bridge_session::BridgeSessionReusePolicy::NewSession,
        crate::bridge_session::BridgeSessionLease {
            owner: bridge_management_lease_owner(),
            ttl: Duration::from_secs(30),
            allow_stale_takeover: true,
        },
    )
}

fn release_bridge_session_command(session_id: &str) -> Result<()> {
    let state = read_bridge_session_state(session_id)?
        .with_context(|| format!("bridge session not found: {session_id}"))?;
    if let Some(owner) = state.lease_owner.as_deref() {
        crate::bridge_session::release_bridge_session_lease(session_id, owner)?;
    }
    Ok(())
}

fn bridge_identity_from_state(
    state: &DaemonBridgeSessionState,
) -> crate::bridge_session::BridgeSessionIdentity {
    crate::bridge_session::BridgeSessionIdentity {
        cwd: state.cwd.clone(),
        account_id: state.account_id.clone(),
        profile: state.profile.clone(),
        terminal_id: state.terminal_id.clone(),
        remote_session_key: state.remote_session_key.clone(),
    }
}

fn print_bridge_session_detail(session: &DaemonBridgeSessionState) {
    println!("bridge session: {}", session.session_id);
    println!("  workspace_key: {}", session.workspace_key);
    println!("  cwd: {}", session.cwd.display());
    println!(
        "  account: {}",
        optional_bridge_value(session.account_id.as_deref())
    );
    println!(
        "  profile: {}",
        optional_bridge_value(session.profile.as_deref())
    );
    println!(
        "  assistant_session_id: {}",
        optional_bridge_value(session.assistant_session_id.as_deref())
    );
    println!(
        "  remote_session_key: {}",
        optional_bridge_value(session.remote_session_key.as_deref())
    );
    println!(
        "  last_run_id: {}",
        optional_bridge_value(session.last_run_id.as_deref())
    );
    println!(
        "  last_ack_at: {}",
        session
            .last_ack_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "-".to_string())
    );
    println!("  lease: {}", bridge_lease_summary(session));
}

fn bridge_lease_summary(session: &DaemonBridgeSessionState) -> String {
    match (&session.lease_owner, session.lease_expires_at) {
        (Some(owner), Some(expires_at)) => {
            format!("owner={} expires={}", owner, expires_at.to_rfc3339())
        }
        (Some(owner), None) => format!("owner={owner}"),
        (None, _) => "none".to_string(),
    }
}

fn optional_bridge_value(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn bridge_management_lease_owner() -> String {
    format!("daemon-cli-pid-{}", std::process::id())
}

fn print_stale_cleanup_report(report: &StaleStateCleanupReport) {
    if let Some(pid) = report.supervisor_pid {
        if report.supervisor_state_removed {
            eprintln!("cleaned stale daemon supervisor state for pid={pid}");
        }
    }
    if report.control_token_removed {
        eprintln!("removed stale daemon control token");
    }
    if report.shutdown_request_removed {
        eprintln!("removed stale daemon shutdown request");
    }
    if report.expired_sleep_state_removed {
        eprintln!("removed expired daemon sleep state");
    }
    for path in &report.worker_states_removed {
        eprintln!("removed stale daemon worker state {}", path.display());
    }
    for worker in &report.live_worker_states_retained {
        let pid = worker
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string());
        eprintln!(
            "retained live daemon worker state worker={} pid={} status={}",
            worker.worker_id, pid, worker.status
        );
    }
}

fn require_running_daemon() -> Result<DaemonProcessState> {
    match status_snapshot()? {
        DaemonStatusSnapshot::Running(state) => Ok(state),
        DaemonStatusSnapshot::Stale(state) => {
            anyhow::bail!("daemon state is stale for pid={}", state.pid)
        }
        DaemonStatusSnapshot::Stopped => anyhow::bail!("daemon is not running"),
    }
}

fn print_status() -> Result<()> {
    match status_snapshot()? {
        DaemonStatusSnapshot::Running(state) => {
            println!("daemon status: running");
            println!("  pid: {}", state.pid);
            println!("  cwd: {}", state.cwd.display());
            println!("  port: {}", state.port);
            println!("  health: {}", state.health_url);
            println!(
                "  ready: {}",
                state.ready_url.as_deref().unwrap_or("unknown")
            );
            println!(
                "  version: {}",
                state.binary_version.as_deref().unwrap_or("unknown")
            );
            println!(
                "  binary: {}",
                state
                    .binary_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            );
            println!(
                "  log: {}",
                state
                    .log_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| daemon_dir().join("supervisor.log").display().to_string())
            );
            println!(
                "  identity: {}",
                process_matches_record(state.pid, state.process_start_key.as_deref())
                    .as_diagnostic()
                    .trim_start_matches("identity=")
            );
            println!("  started_at: {}", state.started_at.to_rfc3339());
            println!("  updated_at: {}", state.updated_at.to_rfc3339());
            println!("  shutdown_requested: {}", state.shutdown_requested);
            println!("  workers: {}", state.workers.len());
            for worker in &state.workers {
                let pid = worker
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "    {} kind={} pid={} status={} updated_at={}",
                    worker.worker_id,
                    worker.kind,
                    pid,
                    worker.status,
                    worker.updated_at.to_rfc3339()
                );
            }
        }
        DaemonStatusSnapshot::Stale(state) => {
            println!("daemon status: stale");
            println!("  stale_pid: {}", state.pid);
            println!("  state: {}", state_path().display());
            println!(
                "  identity: {}",
                process_matches_record(state.pid, state.process_start_key.as_deref())
                    .as_diagnostic()
                    .trim_start_matches("identity=")
            );
        }
        DaemonStatusSnapshot::Stopped => {
            println!("daemon status: stopped");
        }
    }
    Ok(())
}
pub(super) fn parse_port(args: &[String]) -> Option<u16> {
    args.windows(2)
        .find(|pair| pair[0] == "--port")
        .and_then(|pair| pair[1].parse().ok())
}

fn print_result(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("daemon command failed: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:\n  allthecodes daemon [status]\n  allthecodes daemon start [--port <port>]\n  allthecodes daemon stop\n  allthecodes daemon restart [--if-version-changed] [--port <port>]\n  allthecodes daemon logs [supervisor|worker-id] [--tail-bytes N]\n  allthecodes daemon submit <text>\n  allthecodes daemon abort\n  allthecodes daemon command <id> [worker-id]\n  allthecodes daemon events [worker-id]\n  allthecodes daemon token\n  allthecodes daemon bridge sessions|status|resume|new|release\n  allthecodes daemon sleep <seconds> [reason]\n  allthecodes daemon wake"
    );
}

fn print_bridge_usage() {
    eprintln!(
        "Usage:\n  allthecodes daemon bridge sessions\n  allthecodes daemon bridge status [session-id]\n  allthecodes daemon bridge resume <session-id>\n  allthecodes daemon bridge new\n  allthecodes daemon bridge release <session-id>"
    );
}
