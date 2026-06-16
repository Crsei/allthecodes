use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::{operation_lock, protocol, readiness};

use super::paths::{daemon_dir, health_url, state_path};
use super::platform::{configure_detached, process_is_alive, terminate_process_tree};
use super::storage::{
    cleanup_stale_state_before_start, clear_sleep_state, ensure_daemon_dir, read_control_token,
    request_shutdown, status_snapshot, write_sleep_state,
};
use super::types::{DaemonProcessState, DaemonStatusSnapshot, StaleStateCleanupReport};

const STOP_GRACE_PERIOD: Duration = Duration::from_secs(5);

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
        "submit" => print_result(submit_worker_command(args)),
        "abort" => print_result(abort_worker_command()),
        "command" => print_result(print_worker_command(args)),
        "events" => print_result(print_worker_events(args)),
        "token" => print_result(print_control_token()),
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

    if !allthecodes_config::features::enabled(allthecodes_config::features::Feature::Kairos) {
        anyhow::bail!("daemon start requires FEATURE_KAIROS=1");
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

fn stop_daemon() -> Result<()> {
    let snapshot = status_snapshot()?;
    let DaemonStatusSnapshot::Running(state) = snapshot else {
        println!("daemon is not running");
        return Ok(());
    };

    request_shutdown("daemon stop command")?;
    let deadline = Instant::now() + STOP_GRACE_PERIOD;
    while Instant::now() < deadline {
        if !process_is_alive(state.pid) {
            println!("daemon stopped: pid={}", state.pid);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    terminate_process_tree(state.pid)?;
    println!("daemon terminated after timeout: pid={}", state.pid);
    Ok(())
}

fn restart_daemon(args: &[String], cwd: &Path, port: u16) -> Result<()> {
    stop_daemon()?;
    start_daemon(args, cwd, port)
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
        "Usage:\n  allthecodes daemon [status]\n  allthecodes daemon start [--port <port>]\n  allthecodes daemon stop\n  allthecodes daemon restart [--port <port>]\n  allthecodes daemon submit <text>\n  allthecodes daemon abort\n  allthecodes daemon command <id> [worker-id]\n  allthecodes daemon events [worker-id]\n  allthecodes daemon token\n  allthecodes daemon sleep <seconds> [reason]\n  allthecodes daemon wake"
    );
}
