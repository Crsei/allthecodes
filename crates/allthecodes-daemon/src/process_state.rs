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

use crate::protocol;

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
    let path = worker_state_path(worker_id);
    if !path.exists() {
        return Ok(None);
    }
    read_worker_state_file(&path).map(Some)
}

pub fn read_worker_states() -> Result<Vec<DaemonWorkerState>> {
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

pub fn request_shutdown(reason: &str) -> Result<()> {
    ensure_daemon_dir()?;
    let req = DaemonShutdownRequest {
        schema_version: SCHEMA_VERSION,
        requested_at: Utc::now(),
        reason: reason.to_string(),
    };
    atomic_write_json(&shutdown_request_path(), &req)?;

    if let Some(mut state) = read_state()? {
        state.shutdown_requested = true;
        state.updated_at = Utc::now();
        write_state(&state)?;
    }
    Ok(())
}

pub fn shutdown_requested() -> bool {
    shutdown_request_path().is_file()
}

pub fn clear_shutdown_request() -> Result<()> {
    let path = shutdown_request_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub fn write_control_token() -> Result<DaemonControlToken> {
    ensure_daemon_dir()?;
    let token = DaemonControlToken {
        schema_version: SCHEMA_VERSION,
        token: uuid::Uuid::new_v4().to_string(),
        created_at: Utc::now(),
    };
    atomic_write_json(&control_token_path(), &token)?;
    Ok(token)
}

pub fn read_control_token() -> Result<Option<DaemonControlToken>> {
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
    let path = control_token_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
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
    atomic_write_json(&sleep_state_path(), &state)?;
    Ok(state)
}

pub fn read_sleep_state() -> Result<Option<DaemonSleepState>> {
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
    let path = sleep_state_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub fn try_run_management_command(args: &[String], cwd: &Path, port: u16) -> Option<ExitCode> {
    if args.first().map(String::as_str) != Some("daemon") {
        return None;
    }

    let subcommand = args.get(1).map(String::as_str).unwrap_or("status");
    let code = match subcommand {
        "start" => print_result(start_daemon(args, cwd, port)),
        "status" => print_result(print_status()),
        "stop" => print_result(stop_daemon()),
        "restart" => print_result(restart_daemon(args, cwd, port)),
        "submit" => print_result(submit_worker_command(args)),
        "abort" => print_result(abort_worker_command()),
        "command" => print_result(print_worker_command(args)),
        "events" => print_result(print_worker_events(args)),
        "token" => print_result(print_control_token()),
        "sleep" => print_result(schedule_sleep_command(args)),
        "wake" => print_result(wake_daemon_command()),
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
    match status_snapshot()? {
        DaemonStatusSnapshot::Running(state) => {
            println!(
                "daemon already running: pid={} health={}",
                state.pid, state.health_url
            );
            return Ok(());
        }
        DaemonStatusSnapshot::Stale(state) => {
            eprintln!("cleaned stale daemon state for pid={}", state.pid);
        }
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
    println!(
        "daemon start requested: pid={} health={} log={}",
        child.id(),
        health_url(port),
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

fn write_state(state: &DaemonProcessState) -> Result<()> {
    ensure_daemon_dir()?;
    atomic_write_json(&state_path(), state)
}

fn write_worker_state(state: &DaemonWorkerState) -> Result<()> {
    ensure_daemon_dir()?;
    atomic_write_json(&worker_state_path(&state.worker_id), state)
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
}
