//! Cross-process daemon supervisor state.
//!
//! This is the Phase 1 durability layer for daemon management. It lets one
//! process publish daemon status under `~/.allthecodes/daemon/` and another process
//! inspect or request shutdown without sharing memory with the daemon runtime.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{operation_lock, protocol, readiness};

const SCHEMA_VERSION: u32 = 1;
#[cfg(test)]
const DEFAULT_DAEMON_PORT: u16 = 19836;
const STOP_GRACE_PERIOD: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonRunStatus {
    Running,
    Stopped,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonWorkerStatus {
    Starting,
    Running,
    Stopped,
    Exited,
    Stale,
}

impl DaemonWorkerStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Exited => "exited",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonWorkerSummary {
    pub worker_id: String,
    pub kind: String,
    pub pid: Option<u32>,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonProcessState {
    pub schema_version: u32,
    pub status: DaemonRunStatus,
    pub pid: u32,
    pub cwd: PathBuf,
    pub port: u16,
    pub health_url: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub shutdown_requested: bool,
    pub workers: Vec<DaemonWorkerSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonWorkerState {
    pub schema_version: u32,
    pub worker_id: String,
    pub kind: String,
    pub pid: Option<u32>,
    pub cwd: PathBuf,
    pub log_path: PathBuf,
    pub status: DaemonWorkerStatus,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub restart_count: u32,
    pub required: bool,
    pub exit_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonShutdownRequest {
    pub schema_version: u32,
    pub requested_at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonControlToken {
    pub schema_version: u32,
    pub token: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonSleepState {
    pub schema_version: u32,
    pub sleeping_until: DateTime<Utc>,
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatusSnapshot {
    Running(DaemonProcessState),
    Stale(DaemonProcessState),
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleStateCleanupReport {
    pub supervisor_pid: Option<u32>,
    pub supervisor_state_removed: bool,
    pub control_token_removed: bool,
    pub shutdown_request_removed: bool,
    pub expired_sleep_state_removed: bool,
    pub worker_states_removed: Vec<PathBuf>,
    pub live_worker_states_retained: Vec<DaemonWorkerSummary>,
}

pub(crate) fn data_root() -> Option<PathBuf> {
    let home = std::env::var("ALLTHECODES_HOME").ok()?;
    let home = home.trim();
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".allthecodes"))
}

pub(crate) fn daily_log_path(now: DateTime<Local>) -> PathBuf {
    let year = now.format("%Y").to_string();
    let month = now.format("%m").to_string();
    let filename = now.format("%Y-%m-%d.md").to_string();
    data_root()
        .map(|root| root.join("logs").join(year).join(month).join(filename))
        .unwrap_or_else(|| allthecodes_config::paths::daily_log_path(now))
}

pub(crate) fn team_memory_dir(cwd: &Path) -> PathBuf {
    let Some(root) = data_root() else {
        return allthecodes_config::paths::team_memory_dir(cwd);
    };
    let sanitized: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join("projects")
        .join(sanitized)
        .join("memory")
        .join("team")
}

pub fn daemon_dir() -> PathBuf {
    data_root()
        .map(|root| root.join("daemon"))
        .unwrap_or_else(allthecodes_config::paths::daemon_dir)
}

pub fn state_path() -> PathBuf {
    daemon_dir().join("supervisor.json")
}

pub fn shutdown_request_path() -> PathBuf {
    daemon_dir().join("shutdown-request.json")
}

pub fn control_token_path() -> PathBuf {
    daemon_dir().join("control-token.json")
}

pub fn sleep_state_path() -> PathBuf {
    daemon_dir().join("sleep-state.json")
}

pub fn workers_dir() -> PathBuf {
    daemon_dir().join("workers")
}

pub fn worker_state_path(worker_id: &str) -> PathBuf {
    workers_dir().join(format!("{}.json", sanitize_worker_id(worker_id)))
}

pub fn logs_dir() -> PathBuf {
    daemon_dir().join("logs")
}

pub fn worker_log_path(worker_id: &str) -> PathBuf {
    logs_dir().join(format!("{}.log", sanitize_worker_id(worker_id)))
}

pub fn write_started(port: u16, cwd: &Path) -> Result<DaemonProcessState> {
    clear_shutdown_request()?;
    write_control_token()?;
    let now = Utc::now();
    let state = DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: std::process::id(),
        cwd: cwd.to_path_buf(),
        port,
        health_url: health_url(port),
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    };
    write_state(&state)?;
    Ok(state)
}

pub fn write_stopped(port: u16, cwd: &Path) -> Result<DaemonProcessState> {
    clear_shutdown_request()?;
    clear_control_token()?;
    clear_sleep_state()?;
    let now = Utc::now();
    let state = DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Stopped,
        pid: std::process::id(),
        cwd: cwd.to_path_buf(),
        port,
        health_url: health_url(port),
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    };
    write_state(&state)?;
    Ok(state)
}

pub fn write_supervisor_heartbeat(port: u16, cwd: &Path) -> Result<DaemonProcessState> {
    let now = Utc::now();
    let workers = worker_summaries()?;
    let mut state = read_state()?.unwrap_or_else(|| DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: std::process::id(),
        cwd: cwd.to_path_buf(),
        port,
        health_url: health_url(port),
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    });

    state.schema_version = SCHEMA_VERSION;
    state.status = DaemonRunStatus::Running;
    state.pid = std::process::id();
    state.cwd = cwd.to_path_buf();
    state.port = port;
    state.health_url = health_url(port);
    state.updated_at = now;
    state.shutdown_requested = shutdown_requested();
    state.workers = workers;
    write_state(&state)?;
    Ok(state)
}

pub fn write_worker_running(
    worker_id: &str,
    kind: &str,
    pid: u32,
    cwd: &Path,
    log_path: &Path,
    restart_count: u32,
    required: bool,
) -> Result<DaemonWorkerState> {
    let now = Utc::now();
    let state = DaemonWorkerState {
        schema_version: SCHEMA_VERSION,
        worker_id: worker_id.to_string(),
        kind: kind.to_string(),
        pid: Some(pid),
        cwd: cwd.to_path_buf(),
        log_path: log_path.to_path_buf(),
        status: DaemonWorkerStatus::Running,
        started_at: now,
        updated_at: now,
        last_heartbeat_at: Some(now),
        restart_count,
        required,
        exit_status: None,
    };
    write_worker_state(&state)?;
    Ok(state)
}

pub fn write_worker_heartbeat(worker_id: &str) -> Result<()> {
    let Some(mut state) = read_worker_state(worker_id)? else {
        anyhow::bail!("daemon worker state not found for {worker_id}");
    };
    let now = Utc::now();
    state.status = DaemonWorkerStatus::Running;
    state.updated_at = now;
    state.last_heartbeat_at = Some(now);
    state.exit_status = None;
    write_worker_state(&state)
}

pub fn write_worker_stopped(worker_id: &str, exit_status: Option<String>) -> Result<()> {
    let Some(mut state) = read_worker_state(worker_id)? else {
        return Ok(());
    };
    state.status = if exit_status.is_some() {
        DaemonWorkerStatus::Exited
    } else {
        DaemonWorkerStatus::Stopped
    };
    state.pid = None;
    state.updated_at = Utc::now();
    state.exit_status = exit_status;
    write_worker_state(&state)
}

pub fn write_worker_stale(worker_id: &str, reason: &str) -> Result<()> {
    let Some(mut state) = read_worker_state(worker_id)? else {
        return Ok(());
    };
    state.status = DaemonWorkerStatus::Stale;
    state.updated_at = Utc::now();
    state.exit_status = Some(reason.to_string());
    write_worker_state(&state)
}

pub fn read_worker_state(worker_id: &str) -> Result<Option<DaemonWorkerState>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_worker(worker_id) {
        Ok(Some(state)) => return Ok(Some(state)),
        Ok(None) => {}
        Err(err) => warn!(
            worker_id,
            error = %err,
            "failed to read daemon worker from sqlite; falling back to JSON"
        ),
    }

    let path = worker_state_path(worker_id);
    if !path.exists() {
        return Ok(None);
    }
    read_worker_state_file(&path).map(Some)
}

pub fn read_worker_states() -> Result<Vec<DaemonWorkerState>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_workers() {
        Ok(states) => return Ok(states),
        Err(err) => warn!(
            error = %err,
            "failed to read daemon workers from sqlite; falling back to JSON"
        ),
    }

    let dir = workers_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut states = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        states.push(read_worker_state_file(&path)?);
    }
    states.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
    Ok(states)
}

pub fn worker_summaries() -> Result<Vec<DaemonWorkerSummary>> {
    Ok(read_worker_states()?
        .into_iter()
        .map(|state| DaemonWorkerSummary {
            worker_id: state.worker_id,
            kind: state.kind,
            pid: state.pid,
            status: state.status.as_str().to_string(),
            updated_at: state.updated_at,
        })
        .collect())
}

pub fn read_state() -> Result<Option<DaemonProcessState>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_state_value::<DaemonProcessState>("supervisor") {
        Ok(state) => return Ok(state),
        Err(err) => warn!(
            error = %err,
            "failed to read daemon supervisor from sqlite; falling back to JSON"
        ),
    }

    let path = state_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read daemon state {}", path.display()))?;
    let state: DaemonProcessState = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon state {}", path.display()))?;
    Ok(Some(state))
}

pub fn status_snapshot() -> Result<DaemonStatusSnapshot> {
    let Some(mut state) = read_state()? else {
        return Ok(DaemonStatusSnapshot::Stopped);
    };
    if state.status != DaemonRunStatus::Running {
        return Ok(DaemonStatusSnapshot::Stopped);
    }
    if process_is_alive(state.pid) {
        state.workers = worker_summaries()?;
        return Ok(DaemonStatusSnapshot::Running(state));
    }
    state.status = DaemonRunStatus::Stale;
    state.updated_at = Utc::now();
    write_state(&state)?;
    Ok(DaemonStatusSnapshot::Stale(state))
}

pub fn cleanup_stale_state_before_start() -> Result<StaleStateCleanupReport> {
    let mut report = StaleStateCleanupReport::default();
    let Some(state) = read_state()? else {
        return Ok(report);
    };
    if state.status == DaemonRunStatus::Stopped || process_is_alive(state.pid) {
        return Ok(report);
    }

    report.supervisor_pid = Some(state.pid);
    report.supervisor_state_removed = remove_state_value("supervisor", &state_path())?;
    report.control_token_removed = remove_state_value("control-token", &control_token_path())?;
    report.shutdown_request_removed =
        remove_state_value("shutdown-request", &shutdown_request_path())?;
    if read_sleep_state()?.is_some_and(|sleep| sleep.sleeping_until <= Utc::now()) {
        report.expired_sleep_state_removed =
            remove_state_value("sleep-state", &sleep_state_path())?;
    }
    #[cfg(feature = "sqlite-storage")]
    cleanup_worker_state_records(&mut report)?;
    cleanup_worker_state_files(&mut report)?;
    Ok(report)
}

pub fn request_shutdown(reason: &str) -> Result<()> {
    ensure_daemon_dir()?;
    let req = DaemonShutdownRequest {
        schema_version: SCHEMA_VERSION,
        requested_at: Utc::now(),
        reason: reason.to_string(),
    };
    write_state_value("shutdown-request", &shutdown_request_path(), &req)?;

    if let Some(mut state) = read_state()? {
        state.shutdown_requested = true;
        state.updated_at = Utc::now();
        write_state(&state)?;
    }
    Ok(())
}

pub fn shutdown_requested() -> bool {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::state_value_exists("shutdown-request") {
        Ok(true) => return true,
        Ok(false) => {}
        Err(err) => warn!(
            error = %err,
            "failed to check daemon shutdown request in sqlite; falling back to JSON"
        ),
    }
    shutdown_request_path().is_file()
}

pub fn clear_shutdown_request() -> Result<()> {
    remove_state_value("shutdown-request", &shutdown_request_path()).map(|_| ())
}

pub fn write_control_token() -> Result<DaemonControlToken> {
    ensure_daemon_dir()?;
    let token = DaemonControlToken {
        schema_version: SCHEMA_VERSION,
        token: uuid::Uuid::new_v4().to_string(),
        created_at: Utc::now(),
    };
    write_state_value("control-token", &control_token_path(), &token)?;
    Ok(token)
}

pub fn read_control_token() -> Result<Option<DaemonControlToken>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_state_value::<DaemonControlToken>("control-token") {
        Ok(token) => return Ok(token),
        Err(err) => warn!(
            error = %err,
            "failed to read daemon control token from sqlite; falling back to JSON"
        ),
    }

    let path = control_token_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read daemon control token {}", path.display()))?;
    let token = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon control token {}", path.display()))?;
    Ok(Some(token))
}

pub fn verify_control_token(candidate: &str) -> Result<bool> {
    Ok(read_control_token()?
        .map(|stored| stored.token == candidate)
        .unwrap_or(false))
}

pub fn clear_control_token() -> Result<()> {
    remove_state_value("control-token", &control_token_path()).map(|_| ())
}

pub fn write_sleep_state(duration_seconds: u64, reason: &str) -> Result<DaemonSleepState> {
    let until = Utc::now() + chrono::Duration::seconds(duration_seconds as i64);
    write_sleep_state_until(until, reason)
}

pub fn write_sleep_state_until(
    sleeping_until: DateTime<Utc>,
    reason: &str,
) -> Result<DaemonSleepState> {
    ensure_daemon_dir()?;
    let state = DaemonSleepState {
        schema_version: SCHEMA_VERSION,
        sleeping_until,
        reason: if reason.trim().is_empty() {
            None
        } else {
            Some(reason.trim().to_string())
        },
        updated_at: Utc::now(),
    };
    write_state_value("sleep-state", &sleep_state_path(), &state)?;
    Ok(state)
}

pub fn read_sleep_state() -> Result<Option<DaemonSleepState>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_state_value::<DaemonSleepState>("sleep-state") {
        Ok(state) => return Ok(state),
        Err(err) => warn!(
            error = %err,
            "failed to read daemon sleep state from sqlite; falling back to JSON"
        ),
    }

    let path = sleep_state_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read daemon sleep state {}", path.display()))?;
    let state = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon sleep state {}", path.display()))?;
    Ok(Some(state))
}

pub fn active_sleep_state() -> Result<Option<DaemonSleepState>> {
    let Some(state) = read_sleep_state()? else {
        return Ok(None);
    };
    if state.sleeping_until <= Utc::now() {
        clear_sleep_state()?;
        return Ok(None);
    }
    Ok(Some(state))
}

pub fn clear_sleep_state() -> Result<()> {
    remove_state_value("sleep-state", &sleep_state_path()).map(|_| ())
}

fn cleanup_worker_state_files(report: &mut StaleStateCleanupReport) -> Result<()> {
    let dir = workers_dir();
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if entry
            .metadata()
            .with_context(|| format!("failed to stat {}", path.display()))?
            .len()
            == 0
        {
            fs::remove_file(&path).with_context(|| {
                format!("failed to remove empty worker state {}", path.display())
            })?;
            report.worker_states_removed.push(path);
            continue;
        }

        let state = match read_worker_state_file(&path) {
            Ok(state) => state,
            Err(err) => {
                eprintln!(
                    "retained unreadable daemon worker state {}: {err:#}",
                    path.display()
                );
                continue;
            }
        };
        match state.pid {
            Some(pid) if process_is_alive(pid) => {
                report
                    .live_worker_states_retained
                    .push(DaemonWorkerSummary {
                        worker_id: state.worker_id,
                        kind: state.kind,
                        pid: state.pid,
                        status: state.status.as_str().to_string(),
                        updated_at: state.updated_at,
                    });
            }
            Some(_) | None => {
                fs::remove_file(&path).with_context(|| {
                    format!("failed to remove stale worker state {}", path.display())
                })?;
                report.worker_states_removed.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(feature = "sqlite-storage")]
fn cleanup_worker_state_records(report: &mut StaleStateCleanupReport) -> Result<()> {
    let states = match sqlite_store::read_workers() {
        Ok(states) => states,
        Err(err) => {
            warn!(
                error = %err,
                "failed to read daemon worker records from sqlite for stale cleanup"
            );
            return Ok(());
        }
    };

    for state in states {
        let path = worker_state_path(&state.worker_id);
        match state.pid {
            Some(pid) if process_is_alive(pid) => {
                if !path.exists() {
                    report
                        .live_worker_states_retained
                        .push(DaemonWorkerSummary {
                            worker_id: state.worker_id,
                            kind: state.kind,
                            pid: state.pid,
                            status: state.status.as_str().to_string(),
                            updated_at: state.updated_at,
                        });
                }
            }
            Some(_) | None => {
                if let Err(err) = sqlite_store::remove_worker(&state.worker_id) {
                    warn!(
                        worker_id = %state.worker_id,
                        error = %err,
                        "failed to remove stale daemon worker from sqlite"
                    );
                }
                if !path.exists() {
                    report.worker_states_removed.push(path);
                }
            }
        }
    }

    Ok(())
}

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

    let command = super::protocol_store().enqueue_command(
        super::supervisor::ASSISTANT_WORKER_ID,
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
    let command = super::protocol_store().enqueue_command(
        super::supervisor::ASSISTANT_WORKER_ID,
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
        .unwrap_or(super::supervisor::ASSISTANT_WORKER_ID);
    let Some(command) = super::protocol_store().read_command(worker_id, command_id)? else {
        anyhow::bail!("daemon command not found: {command_id}");
    };
    println!("{}", serde_json::to_string_pretty(&command)?);
    Ok(())
}

fn print_worker_events(args: &[String]) -> Result<()> {
    let worker_id = args
        .get(2)
        .map(String::as_str)
        .unwrap_or(super::supervisor::ASSISTANT_WORKER_ID);
    let events = super::protocol_store().read_worker_events(worker_id)?;
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

fn remove_file_if_exists(path: &Path) -> Result<bool> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn write_state(state: &DaemonProcessState) -> Result<()> {
    write_state_value("supervisor", &state_path(), state)
}

fn write_worker_state(state: &DaemonWorkerState) -> Result<()> {
    ensure_daemon_dir()?;
    #[cfg(feature = "sqlite-storage")]
    if let Err(err) = sqlite_store::write_worker(state) {
        warn!(
            worker_id = %state.worker_id,
            error = %err,
            "failed to write daemon worker to sqlite; keeping JSON backup"
        );
    }
    atomic_write_json(&worker_state_path(&state.worker_id), state)
}

fn write_state_value<T: Serialize>(key: &'static str, path: &Path, value: &T) -> Result<()> {
    ensure_daemon_dir()?;
    #[cfg(feature = "sqlite-storage")]
    if let Err(err) = sqlite_store::write_state_value(key, value) {
        warn!(
            key,
            error = %err,
            "failed to write daemon state to sqlite; keeping JSON backup"
        );
    }
    atomic_write_json(path, value)
}

fn remove_state_value(key: &'static str, path: &Path) -> Result<bool> {
    let mut removed = false;
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::remove_state_value(key) {
        Ok(sqlite_removed) => removed |= sqlite_removed,
        Err(err) => warn!(
            key,
            error = %err,
            "failed to remove daemon state from sqlite; removing JSON backup"
        ),
    }
    removed |= remove_file_if_exists(path)?;
    Ok(removed)
}

fn read_worker_state_file(path: &Path) -> Result<DaemonWorkerState> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon worker state {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon worker state {}", path.display()))
}

fn ensure_daemon_dir() -> Result<()> {
    fs::create_dir_all(daemon_dir())
        .with_context(|| format!("failed to create {}", daemon_dir().display()))?;
    fs::create_dir_all(workers_dir())
        .with_context(|| format!("failed to create {}", workers_dir().display()))?;
    fs::create_dir_all(logs_dir())
        .with_context(|| format!("failed to create {}", logs_dir().display()))?;
    Ok(())
}

pub(crate) fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)
            .with_context(|| format!("failed to create {}", tmp.display()))?;
        let bytes = serde_json::to_vec_pretty(value)?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", tmp.display()))?;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first_err) if path.exists() => {
            fs::remove_file(path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
            fs::rename(&tmp, path).with_context(|| {
                format!(
                    "failed to rename {} to {} after replace fallback: {first_err}",
                    tmp.display(),
                    path.display()
                )
            })
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display())),
    }
}

#[cfg(feature = "sqlite-storage")]
mod sqlite_store {
    use super::*;
    use allthecodes_db::{Migration, MigrationRunner};
    use serde::de::DeserializeOwned;
    use sqlx::{Row, SqlitePool};

    const MIGRATIONS: &[Migration] = &[
        Migration::new(
            1,
            r#"
            CREATE TABLE IF NOT EXISTS daemon_state (
                key TEXT PRIMARY KEY NOT NULL,
                value_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        ),
        Migration::new(
            2,
            r#"
            CREATE TABLE IF NOT EXISTS daemon_workers (
                worker_id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL,
                pid INTEGER,
                status TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                value_json TEXT NOT NULL
            )
            "#,
        ),
        Migration::new(
            3,
            r#"
            CREATE INDEX IF NOT EXISTS idx_daemon_workers_status
                ON daemon_workers(status, updated_at)
            "#,
        ),
    ];

    pub(super) fn write_state_value<T>(key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let value_json = serde_json::to_string(value)?;
        run(move |pool| async move {
            sqlx::query(
                r#"
                INSERT INTO daemon_state (key, value_json, updated_at)
                VALUES (?, ?, ?)
                ON CONFLICT(key) DO UPDATE SET
                    value_json = excluded.value_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(key)
            .bind(value_json)
            .bind(Utc::now().timestamp())
            .execute(&pool)
            .await
            .context("failed to upsert daemon state")?;
            Ok(())
        })
    }

    pub(super) fn read_state_value<T>(key: &'static str) -> Result<Option<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        run(move |pool| async move {
            import_legacy_json(&pool).await?;
            let row = sqlx::query("SELECT value_json FROM daemon_state WHERE key = ?")
                .bind(key)
                .fetch_optional(&pool)
                .await
                .context("failed to read daemon state")?;
            row.map(|row| {
                let value_json: String = row.try_get("value_json")?;
                serde_json::from_str(&value_json).context("failed to decode daemon state JSON")
            })
            .transpose()
        })
    }

    pub(super) fn state_value_exists(key: &'static str) -> Result<bool> {
        run(move |pool| async move {
            import_legacy_json(&pool).await?;
            Ok(sqlx::query("SELECT 1 FROM daemon_state WHERE key = ?")
                .bind(key)
                .fetch_optional(&pool)
                .await
                .context("failed to check daemon state")?
                .is_some())
        })
    }

    pub(super) fn remove_state_value(key: &'static str) -> Result<bool> {
        run(move |pool| async move {
            let result = sqlx::query("DELETE FROM daemon_state WHERE key = ?")
                .bind(key)
                .execute(&pool)
                .await
                .context("failed to delete daemon state")?;
            Ok(result.rows_affected() > 0)
        })
    }

    pub(super) fn write_worker(state: &DaemonWorkerState) -> Result<()> {
        let state = state.clone();
        run(move |pool| async move { upsert_worker(&pool, &state).await })
    }

    pub(super) fn read_worker(worker_id: &str) -> Result<Option<DaemonWorkerState>> {
        let worker_id = worker_id.to_string();
        run(move |pool| async move {
            import_legacy_json(&pool).await?;
            read_worker_from_pool(&pool, &worker_id).await
        })
    }

    pub(super) fn read_workers() -> Result<Vec<DaemonWorkerState>> {
        run(move |pool| async move {
            import_legacy_json(&pool).await?;
            let rows = sqlx::query(
                r#"
                SELECT value_json
                FROM daemon_workers
                ORDER BY worker_id ASC
                "#,
            )
            .fetch_all(&pool)
            .await
            .context("failed to read daemon worker rows")?;
            rows.into_iter()
                .map(|row| {
                    let value_json: String = row.try_get("value_json")?;
                    serde_json::from_str(&value_json).context("failed to decode daemon worker JSON")
                })
                .collect()
        })
    }

    pub(super) fn remove_worker(worker_id: &str) -> Result<bool> {
        let worker_id = worker_id.to_string();
        run(move |pool| async move {
            let result = sqlx::query("DELETE FROM daemon_workers WHERE worker_id = ?")
                .bind(&worker_id)
                .execute(&pool)
                .await
                .context("failed to delete daemon worker")?;
            Ok(result.rows_affected() > 0)
        })
    }

    fn run<F, Fut, T>(op: F) -> Result<T>
    where
        F: FnOnce(SqlitePool) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        allthecodes_db::run_sqlite_sync("allthecodes-daemon-sqlite", async move {
            let pool = migrated_pool().await?;
            op(pool).await
        })
    }

    async fn migrated_pool() -> Result<SqlitePool> {
        let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
        MigrationRunner::new("daemon", MIGRATIONS)
            .run(&pool)
            .await?;
        Ok(pool)
    }

    async fn import_legacy_json(pool: &SqlitePool) -> Result<()> {
        import_state_file::<DaemonProcessState>(pool, "supervisor", &state_path()).await?;
        import_state_file::<DaemonShutdownRequest>(
            pool,
            "shutdown-request",
            &shutdown_request_path(),
        )
        .await?;
        import_state_file::<DaemonControlToken>(pool, "control-token", &control_token_path())
            .await?;
        import_state_file::<DaemonSleepState>(pool, "sleep-state", &sleep_state_path()).await?;
        import_worker_files(pool).await?;
        Ok(())
    }

    async fn import_state_file<T>(pool: &SqlitePool, key: &'static str, path: &Path) -> Result<()>
    where
        T: DeserializeOwned + Serialize,
    {
        if !path.exists()
            || sqlx::query("SELECT 1 FROM daemon_state WHERE key = ?")
                .bind(key)
                .fetch_optional(pool)
                .await
                .context("failed to check daemon state before import")?
                .is_some()
        {
            return Ok(());
        }

        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read daemon JSON {}", path.display()))?;
        let value: T = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse daemon JSON {}", path.display()))?;
        let value_json = serde_json::to_string(&value)?;
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO daemon_state (key, value_json, updated_at)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(key)
        .bind(value_json)
        .bind(Utc::now().timestamp())
        .execute(pool)
        .await
        .context("failed to import daemon state JSON")?;
        Ok(())
    }

    async fn import_worker_files(pool: &SqlitePool) -> Result<()> {
        let dir = workers_dir();
        if !dir.exists() {
            return Ok(());
        }

        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry =
                entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let state = match read_worker_state_file(&path) {
                Ok(state) => state,
                Err(err) => {
                    warn!(
                        path = %path.display(),
                        error = %err,
                        "skipping invalid daemon worker JSON during sqlite import"
                    );
                    continue;
                }
            };
            if read_worker_from_pool(pool, &state.worker_id)
                .await?
                .is_none()
            {
                upsert_worker(pool, &state).await?;
            }
        }

        Ok(())
    }

    async fn read_worker_from_pool(
        pool: &SqlitePool,
        worker_id: &str,
    ) -> Result<Option<DaemonWorkerState>> {
        sqlx::query("SELECT value_json FROM daemon_workers WHERE worker_id = ?")
            .bind(worker_id)
            .fetch_optional(pool)
            .await
            .context("failed to read daemon worker")?
            .map(|row| {
                let value_json: String = row.try_get("value_json")?;
                serde_json::from_str(&value_json).context("failed to decode daemon worker JSON")
            })
            .transpose()
    }

    async fn upsert_worker(pool: &SqlitePool, state: &DaemonWorkerState) -> Result<()> {
        let value_json = serde_json::to_string(state)?;
        sqlx::query(
            r#"
            INSERT INTO daemon_workers (
                worker_id,
                kind,
                pid,
                status,
                updated_at,
                value_json
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(worker_id) DO UPDATE SET
                kind = excluded.kind,
                pid = excluded.pid,
                status = excluded.status,
                updated_at = excluded.updated_at,
                value_json = excluded.value_json
            "#,
        )
        .bind(&state.worker_id)
        .bind(&state.kind)
        .bind(state.pid.map(i64::from))
        .bind(state.status.as_str())
        .bind(state.updated_at.timestamp())
        .bind(value_json)
        .execute(pool)
        .await
        .context("failed to upsert daemon worker")?;
        Ok(())
    }
}

fn health_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/health")
}

fn sanitize_worker_id(worker_id: &str) -> String {
    worker_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_port(args: &[String]) -> Option<u16> {
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

#[cfg(windows)]
fn configure_detached(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(not(windows))]
fn configure_detached(_cmd: &mut Command) {}

#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.contains(&format!("\"{pid}\"")) || line.contains(&pid.to_string()))
}

#[cfg(unix)]
pub(crate) fn terminate_process_tree(pid: u32) -> Result<()> {
    let pid = pid as libc::pid_t;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(500));
    if process_is_alive(pid as u32) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn terminate_process_tree(pid: u32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .context("failed to run taskkill")?;
    if !status.success() {
        anyhow::bail!("taskkill failed with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    #[serial]
    fn write_and_read_state_uses_allthecodes_home() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();

        let state = write_started(19999, &cwd).unwrap();
        assert!(state_path().starts_with(temp.path().join(".allthecodes")));
        let read_back = read_state().unwrap().unwrap();

        assert_eq!(state.pid, std::process::id());
        assert_eq!(read_back.port, 19999);
        assert_eq!(read_back.cwd, cwd);
        assert!(state_path().starts_with(temp.path()));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial]
    fn read_state_imports_legacy_json() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let now = Utc::now();
        let state = DaemonProcessState {
            schema_version: SCHEMA_VERSION,
            status: DaemonRunStatus::Running,
            pid: std::process::id(),
            cwd: cwd.clone(),
            port: DEFAULT_DAEMON_PORT,
            health_url: health_url(DEFAULT_DAEMON_PORT),
            started_at: now,
            updated_at: now,
            shutdown_requested: false,
            workers: Vec::new(),
        };
        atomic_write_json(&state_path(), &state).unwrap();

        let read_back = read_state().unwrap().unwrap();
        assert_eq!(read_back.cwd, cwd);
        assert_eq!(read_back.port, DEFAULT_DAEMON_PORT);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial]
    fn daemon_state_falls_back_to_json_when_sqlite_path_is_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        fs::create_dir_all(
            temp.path()
                .join(".allthecodes")
                .join("state")
                .join("state_5.sqlite"),
        )
        .unwrap();

        let token = write_control_token().unwrap();
        assert!(control_token_path().exists());
        assert_eq!(read_control_token().unwrap().unwrap().token, token.token);
    }

    #[test]
    #[serial]
    fn shutdown_request_sets_state_flag() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();

        request_shutdown("test").unwrap();
        let state = read_state().unwrap().unwrap();

        assert!(shutdown_requested());
        assert!(state.shutdown_requested);
    }

    #[test]
    fn current_process_is_alive() {
        assert!(process_is_alive(std::process::id()));
    }

    #[test]
    fn parses_port_from_management_args() {
        let args = vec![
            "daemon".to_string(),
            "start".to_string(),
            "--port".to_string(),
            "20100".to_string(),
        ];
        assert_eq!(parse_port(&args), Some(20100));
    }

    #[test]
    #[serial]
    fn worker_state_updates_supervisor_summary() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let log_path = worker_log_path("assistant/session:1");

        write_worker_running(
            "assistant/session:1",
            "assistant-session",
            std::process::id(),
            &cwd,
            &log_path,
            2,
            true,
        )
        .unwrap();
        write_supervisor_heartbeat(DEFAULT_DAEMON_PORT, &cwd).unwrap();

        let state = read_state().unwrap().unwrap();
        assert_eq!(state.workers.len(), 1);
        assert_eq!(state.workers[0].worker_id, "assistant/session:1");
        assert_eq!(state.workers[0].status, "running");
        assert!(worker_state_path("assistant/session:1").ends_with("assistant_session_1.json"));
    }

    #[test]
    #[serial]
    fn worker_heartbeat_preserves_restart_count() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let log_path = worker_log_path("assistant-session-1");
        write_worker_running(
            "assistant-session-1",
            "assistant-session",
            std::process::id(),
            temp.path(),
            &log_path,
            3,
            true,
        )
        .unwrap();

        write_worker_heartbeat("assistant-session-1").unwrap();
        let state = read_worker_state("assistant-session-1").unwrap().unwrap();

        assert_eq!(state.restart_count, 3);
        assert_eq!(state.status, DaemonWorkerStatus::Running);
        assert!(state.last_heartbeat_at.is_some());
    }

    #[test]
    #[serial]
    fn control_token_is_created_and_cleared_with_daemon_state() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

        write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();
        let token = read_control_token().unwrap().unwrap();
        assert!(verify_control_token(&token.token).unwrap());
        assert!(!verify_control_token("wrong").unwrap());

        write_stopped(DEFAULT_DAEMON_PORT, temp.path()).unwrap();
        assert!(read_control_token().unwrap().is_none());
    }

    #[test]
    #[serial]
    fn sleep_state_tracks_active_and_expired_sleep() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

        let active = write_sleep_state(60, "waiting").unwrap();
        assert_eq!(active.reason.as_deref(), Some("waiting"));
        assert!(active_sleep_state().unwrap().is_some());

        write_sleep_state_until(Utc::now() - chrono::Duration::seconds(1), "expired").unwrap();
        assert!(active_sleep_state().unwrap().is_none());
        assert!(read_sleep_state().unwrap().is_none());
    }

    fn dead_test_pid() -> u32 {
        (1_000_000u32..4_194_303u32)
            .rev()
            .find(|pid| !process_is_alive(*pid))
            .expect("dead test pid")
    }

    #[test]
    #[serial]
    fn stale_cleanup_removes_dead_supervisor_state() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let now = Utc::now();
        let state = DaemonProcessState {
            schema_version: SCHEMA_VERSION,
            status: DaemonRunStatus::Running,
            pid: dead_test_pid(),
            cwd: cwd.clone(),
            port: DEFAULT_DAEMON_PORT,
            health_url: health_url(DEFAULT_DAEMON_PORT),
            started_at: now,
            updated_at: now,
            shutdown_requested: true,
            workers: Vec::new(),
        };
        write_state(&state).unwrap();
        write_control_token().unwrap();
        request_shutdown("test").unwrap();
        write_sleep_state_until(Utc::now() - chrono::Duration::seconds(1), "expired").unwrap();

        let report = cleanup_stale_state_before_start().unwrap();

        assert_eq!(report.supervisor_pid, Some(dead_test_pid()));
        assert!(report.supervisor_state_removed);
        assert!(report.control_token_removed);
        assert!(report.shutdown_request_removed);
        assert!(report.expired_sleep_state_removed);
        assert!(!state_path().exists());
        assert!(!control_token_path().exists());
        assert!(!shutdown_request_path().exists());
        assert!(!sleep_state_path().exists());
    }

    #[test]
    #[serial]
    fn stale_cleanup_preserves_live_supervisor_state() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();

        let report = cleanup_stale_state_before_start().unwrap();

        assert_eq!(report, StaleStateCleanupReport::default());
        assert!(state_path().exists());
        assert!(control_token_path().exists());
    }

    #[test]
    #[serial]
    fn stale_cleanup_removes_dead_worker_and_retains_live_worker() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let now = Utc::now();
        write_state(&DaemonProcessState {
            schema_version: SCHEMA_VERSION,
            status: DaemonRunStatus::Running,
            pid: dead_test_pid(),
            cwd: cwd.clone(),
            port: DEFAULT_DAEMON_PORT,
            health_url: health_url(DEFAULT_DAEMON_PORT),
            started_at: now,
            updated_at: now,
            shutdown_requested: false,
            workers: Vec::new(),
        })
        .unwrap();
        write_worker_running(
            "live-worker",
            "assistant-session",
            std::process::id(),
            &cwd,
            &worker_log_path("live-worker"),
            0,
            true,
        )
        .unwrap();
        write_worker_running(
            "dead-worker",
            "assistant-session",
            dead_test_pid(),
            &cwd,
            &worker_log_path("dead-worker"),
            0,
            true,
        )
        .unwrap();
        let empty_path = worker_state_path("empty-worker");
        fs::write(&empty_path, "").unwrap();

        let report = cleanup_stale_state_before_start().unwrap();

        assert!(worker_state_path("live-worker").exists());
        assert!(!worker_state_path("dead-worker").exists());
        assert!(!empty_path.exists());
        assert_eq!(report.live_worker_states_retained.len(), 1);
        assert_eq!(
            report.live_worker_states_retained[0].worker_id,
            "live-worker"
        );
        assert_eq!(report.worker_states_removed.len(), 2);
    }
}
