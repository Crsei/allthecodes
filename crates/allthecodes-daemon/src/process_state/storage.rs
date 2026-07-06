use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlite-storage")]
use tracing::warn;

use super::atomic_write_json;
use super::paths::{
    bridge_session_state_path, control_token_path, daemon_dir, health_url, logs_dir,
    proactive_state_path, shutdown_request_path, sleep_state_path, state_path,
    terminal_focus_state_path, worker_state_path, workers_dir,
};
use super::platform::{process_matches_record, process_start_key};
#[cfg(feature = "sqlite-storage")]
use super::sqlite_store;
use super::types::{
    DaemonBridgeSessionState, DaemonControlToken, DaemonProactiveState, DaemonProcessState,
    DaemonRunStatus, DaemonShutdownRequest, DaemonSleepState, DaemonStatusSnapshot,
    DaemonTerminalFocusState, DaemonWorkerState, DaemonWorkerStatus, DaemonWorkerSummary,
    ProcessIdentityStatus, StaleStateCleanupReport, SCHEMA_VERSION,
};

const DEFAULT_LOG_TAIL_BYTES: usize = 16 * 1024;

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
        command_kind: Some("daemon-supervisor".to_string()),
        binary_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        binary_path: current_binary_path(),
        log_path: Some(daemon_dir().join("supervisor.log")),
        ready_url: Some(crate::readiness::ready_url(port)),
        process_start_key: process_start_key(std::process::id()),
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
    clear_proactive_state()?;
    clear_terminal_focus_state()?;
    let now = Utc::now();
    let state = DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Stopped,
        pid: std::process::id(),
        cwd: cwd.to_path_buf(),
        port,
        health_url: health_url(port),
        command_kind: Some("daemon-supervisor".to_string()),
        binary_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        binary_path: current_binary_path(),
        log_path: Some(daemon_dir().join("supervisor.log")),
        ready_url: Some(crate::readiness::ready_url(port)),
        process_start_key: process_start_key(std::process::id()),
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
        command_kind: Some("daemon-supervisor".to_string()),
        binary_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        binary_path: current_binary_path(),
        log_path: Some(daemon_dir().join("supervisor.log")),
        ready_url: Some(crate::readiness::ready_url(port)),
        process_start_key: process_start_key(std::process::id()),
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
    state.command_kind = Some("daemon-supervisor".to_string());
    state.binary_version = Some(env!("CARGO_PKG_VERSION").to_string());
    state.binary_path = current_binary_path();
    state.log_path = Some(daemon_dir().join("supervisor.log"));
    state.ready_url = Some(crate::readiness::ready_url(port));
    state.process_start_key = process_start_key(std::process::id());
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
        command_kind: Some(format!("daemon-worker:{kind}")),
        binary_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        binary_path: current_binary_path(),
        ready_url: None,
        process_start_key: process_start_key(pid),
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
    match process_matches_record(state.pid, state.process_start_key.as_deref()) {
        ProcessIdentityStatus::Matched
        | ProcessIdentityStatus::Unknown
        | ProcessIdentityStatus::Unrecorded => {
            state.workers = worker_summaries()?;
            Ok(DaemonStatusSnapshot::Running(state))
        }
        ProcessIdentityStatus::Dead | ProcessIdentityStatus::Mismatched { .. } => {
            state.status = DaemonRunStatus::Stale;
            state.updated_at = Utc::now();
            write_state(&state)?;
            Ok(DaemonStatusSnapshot::Stale(state))
        }
    }
}

pub fn cleanup_stale_state_before_start() -> Result<StaleStateCleanupReport> {
    let mut report = StaleStateCleanupReport::default();
    let Some(state) = read_state()? else {
        return Ok(report);
    };
    if state.status == DaemonRunStatus::Stopped
        || process_matches_record(state.pid, state.process_start_key.as_deref())
            .is_current_process_record()
    {
        return Ok(report);
    }

    report.supervisor_pid = Some(state.pid);
    report.supervisor_state_removed = remove_state_value("supervisor", &state_path())?;
    report.control_token_removed = remove_state_value("control-token", &control_token_path())?;
    report.shutdown_request_removed =
        remove_state_value("shutdown-request", &shutdown_request_path())?;
    let _ = clear_terminal_focus_state();
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

pub fn tail_log(path: &Path, max_bytes: Option<usize>) -> Result<String> {
    ensure_daemon_dir()?;
    let max_bytes = max_bytes.unwrap_or(DEFAULT_LOG_TAIL_BYTES);
    let daemon_root = daemon_dir()
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", daemon_dir().display()))?;
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize log path {}", path.display()))?;
    if !canonical_path.starts_with(&daemon_root) {
        anyhow::bail!(
            "refusing to read log outside daemon dir: {}",
            path.display()
        );
    }

    let mut file = fs::File::open(&canonical_path)
        .with_context(|| format!("failed to open log {}", canonical_path.display()))?;
    let len = file
        .metadata()
        .with_context(|| format!("failed to stat log {}", canonical_path.display()))?
        .len();
    let read_len = len.min(max_bytes as u64);
    file.seek(SeekFrom::Start(len.saturating_sub(read_len)))
        .with_context(|| format!("failed to seek log {}", canonical_path.display()))?;
    let mut bytes = Vec::with_capacity(read_len as usize);
    file.take(read_len)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read log {}", canonical_path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
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
    let state = map_sleep_state(allthecodes_config::proactive_sleep::write_sleep_state(
        duration_seconds,
        reason,
    )?);
    write_sleep_state_sqlite_backup(&state);
    Ok(state)
}

pub fn write_sleep_state_until(
    sleeping_until: DateTime<Utc>,
    reason: &str,
) -> Result<DaemonSleepState> {
    let state = map_sleep_state(
        allthecodes_config::proactive_sleep::write_sleep_state_until(sleeping_until, reason)?,
    );
    write_sleep_state_sqlite_backup(&state);
    Ok(state)
}

pub fn read_sleep_state() -> Result<Option<DaemonSleepState>> {
    #[cfg(feature = "sqlite-storage")]
    {
        let sqlite_state = match sqlite_store::read_state_value::<DaemonSleepState>("sleep-state") {
            Ok(state) => state,
            Err(err) => {
                warn!(
                    error = %err,
                    "failed to read daemon sleep state from sqlite; falling back to JSON"
                );
                None
            }
        };
        let json_state = match allthecodes_config::proactive_sleep::read_sleep_state() {
            Ok(state) => state.map(map_sleep_state),
            Err(err) if sqlite_state.is_some() => {
                warn!(
                    error = %err,
                    "failed to read daemon sleep state JSON backup; using sqlite"
                );
                None
            }
            Err(err) => return Err(err),
        };
        return Ok(reconcile_sleep_states(sqlite_state, json_state));
    }

    #[cfg(not(feature = "sqlite-storage"))]
    allthecodes_config::proactive_sleep::read_sleep_state().map(|state| state.map(map_sleep_state))
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
    #[cfg(feature = "sqlite-storage")]
    if let Err(err) = sqlite_store::remove_state_value("sleep-state") {
        warn!(
            key = "sleep-state",
            error = %err,
            "failed to remove daemon sleep state from sqlite; removing JSON backup"
        );
    }
    allthecodes_config::proactive_sleep::clear_sleep_state("process_state").map(|_| ())
}

pub fn write_proactive_state(
    active: bool,
    next_tick_at: Option<DateTime<Utc>>,
) -> Result<DaemonProactiveState> {
    let state = map_durable_proactive_state(allthecodes_services::proactive::write_durable_state(
        active,
        next_tick_at,
    )?);
    write_proactive_state_sqlite_backup(&state);
    Ok(state)
}

pub fn write_proactive_context_blocked(
    blocked: bool,
    reason: &str,
) -> Result<DaemonProactiveState> {
    let current = read_proactive_state()?;
    let active = current.as_ref().map(|state| state.active).unwrap_or(true);
    let next_tick_at = if blocked {
        None
    } else if active {
        Some(
            Utc::now()
                + chrono::Duration::milliseconds(crate::tick::DEFAULT_TICK_INTERVAL_MS as i64),
        )
    } else {
        None
    };
    let state = map_durable_proactive_state(
        allthecodes_services::proactive::write_durable_state_with_context(
            active,
            next_tick_at,
            blocked,
            blocked.then(|| reason.trim().to_string()),
        )?,
    );
    write_proactive_state_sqlite_backup(&state);
    Ok(state)
}

pub fn read_proactive_state() -> Result<Option<DaemonProactiveState>> {
    #[cfg(feature = "sqlite-storage")]
    {
        let sqlite_state =
            match sqlite_store::read_state_value::<DaemonProactiveState>("proactive-state") {
                Ok(state) => state,
                Err(err) => {
                    warn!(
                        error = %err,
                        "failed to read daemon proactive state from sqlite; falling back to JSON"
                    );
                    None
                }
            };
        let json_state = match allthecodes_services::proactive::read_durable_state() {
            Ok(state) => state.map(map_durable_proactive_state),
            Err(err) if sqlite_state.is_some() => {
                warn!(
                    error = %err,
                    "failed to read daemon proactive state JSON backup; using sqlite"
                );
                None
            }
            Err(err) => return Err(err),
        };
        return Ok(reconcile_proactive_states(sqlite_state, json_state));
    }

    #[cfg(not(feature = "sqlite-storage"))]
    allthecodes_services::proactive::read_durable_state()
        .map(|state| state.map(map_durable_proactive_state))
}

pub fn clear_proactive_state() -> Result<()> {
    remove_state_value("proactive-state", &proactive_state_path()).map(|_| ())
}

pub fn write_terminal_focus_state(focused: bool) -> Result<DaemonTerminalFocusState> {
    let state = DaemonTerminalFocusState {
        schema_version: SCHEMA_VERSION,
        focused,
        updated_at: Utc::now(),
    };
    write_state_value("terminal-focus-state", &terminal_focus_state_path(), &state)?;
    Ok(state)
}

pub fn read_terminal_focus_state() -> Result<Option<DaemonTerminalFocusState>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::read_state_value::<DaemonTerminalFocusState>("terminal-focus-state") {
        Ok(state) => return Ok(state),
        Err(err) => warn!(
            error = %err,
            "failed to read daemon terminal focus state from sqlite; falling back to JSON"
        ),
    }

    let path = terminal_focus_state_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read daemon terminal focus state {}",
            path.display()
        )
    })?;
    serde_json::from_str(&text)
        .with_context(|| {
            format!(
                "failed to parse daemon terminal focus state {}",
                path.display()
            )
        })
        .map(Some)
}

fn clear_terminal_focus_state() -> Result<()> {
    remove_state_value("terminal-focus-state", &terminal_focus_state_path()).map(|_| ())
}

fn map_sleep_state(state: allthecodes_config::proactive_sleep::SleepState) -> DaemonSleepState {
    DaemonSleepState {
        schema_version: state.schema_version,
        sleeping_until: state.sleeping_until,
        reason: state.reason,
        updated_at: state.updated_at,
    }
}

fn map_durable_proactive_state(
    state: allthecodes_services::proactive::DurableProactiveState,
) -> DaemonProactiveState {
    DaemonProactiveState {
        schema_version: state.schema_version,
        active: state.active,
        next_tick_at: state.next_tick_at,
        context_blocked: state.context_blocked,
        blocked_reason: state.blocked_reason,
        updated_at: state.updated_at,
    }
}

fn write_sleep_state_sqlite_backup(state: &DaemonSleepState) {
    #[cfg(feature = "sqlite-storage")]
    if let Err(err) = sqlite_store::write_state_value("sleep-state", state) {
        warn!(
            key = "sleep-state",
            error = %err,
            "failed to write daemon sleep state to sqlite; keeping JSON backup"
        );
    }
    #[cfg(not(feature = "sqlite-storage"))]
    let _ = state;
}

fn write_proactive_state_sqlite_backup(state: &DaemonProactiveState) {
    #[cfg(feature = "sqlite-storage")]
    if let Err(err) = sqlite_store::write_state_value("proactive-state", state) {
        warn!(
            key = "proactive-state",
            error = %err,
            "failed to write daemon proactive state to sqlite; keeping JSON backup"
        );
    }
    #[cfg(not(feature = "sqlite-storage"))]
    let _ = state;
}

#[cfg(feature = "sqlite-storage")]
fn reconcile_sleep_states(
    sqlite_state: Option<DaemonSleepState>,
    json_state: Option<DaemonSleepState>,
) -> Option<DaemonSleepState> {
    match (sqlite_state, json_state) {
        (Some(sqlite), Some(json)) if json.updated_at > sqlite.updated_at => {
            write_sleep_state_sqlite_backup(&json);
            Some(json)
        }
        (Some(sqlite), Some(json)) if sqlite.updated_at > json.updated_at => {
            write_sleep_state_json_backup(&sqlite);
            Some(sqlite)
        }
        (Some(sqlite), Some(_json)) => Some(sqlite),
        (Some(sqlite), None) => {
            write_sleep_state_json_backup(&sqlite);
            Some(sqlite)
        }
        (None, Some(json)) => {
            write_sleep_state_sqlite_backup(&json);
            Some(json)
        }
        (None, None) => None,
    }
}

#[cfg(feature = "sqlite-storage")]
fn reconcile_proactive_states(
    sqlite_state: Option<DaemonProactiveState>,
    json_state: Option<DaemonProactiveState>,
) -> Option<DaemonProactiveState> {
    match (sqlite_state, json_state) {
        (Some(sqlite), Some(json)) if json.updated_at > sqlite.updated_at => {
            write_proactive_state_sqlite_backup(&json);
            Some(json)
        }
        (Some(sqlite), Some(json)) if sqlite.updated_at > json.updated_at => {
            write_proactive_state_json_backup(&sqlite);
            Some(sqlite)
        }
        (Some(sqlite), Some(_json)) => Some(sqlite),
        (Some(sqlite), None) => {
            write_proactive_state_json_backup(&sqlite);
            Some(sqlite)
        }
        (None, Some(json)) => {
            write_proactive_state_sqlite_backup(&json);
            Some(json)
        }
        (None, None) => None,
    }
}

#[cfg(feature = "sqlite-storage")]
fn write_sleep_state_json_backup(state: &DaemonSleepState) {
    let shared = allthecodes_config::proactive_sleep::SleepState {
        schema_version: state.schema_version,
        sleeping_until: state.sleeping_until,
        reason: state.reason.clone(),
        updated_at: state.updated_at,
    };
    if let Err(err) = atomic_write_json(&sleep_state_path(), &shared) {
        warn!(
            key = "sleep-state",
            error = %err,
            "failed to write daemon sleep state JSON backup; keeping sqlite"
        );
    }
}

#[cfg(feature = "sqlite-storage")]
fn write_proactive_state_json_backup(state: &DaemonProactiveState) {
    let shared = allthecodes_services::proactive::DurableProactiveState {
        schema_version: state.schema_version,
        active: state.active,
        next_tick_at: state.next_tick_at,
        context_blocked: state.context_blocked,
        blocked_reason: state.blocked_reason.clone(),
        updated_at: state.updated_at,
    };
    if let Err(err) = atomic_write_json(&proactive_state_path(), &shared) {
        warn!(
            key = "proactive-state",
            error = %err,
            "failed to write daemon proactive state JSON backup; keeping sqlite"
        );
    }
}

#[cfg(test)]
mod sleep_tests {
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
    fn sleep_state_wrapper_writes_canonical_config_json() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

        let state = write_sleep_state(60, " shared config ").unwrap();
        let shared = allthecodes_config::proactive_sleep::read_sleep_state()
            .unwrap()
            .expect("shared config sleep state");

        assert_eq!(
            state.schema_version,
            allthecodes_config::proactive_sleep::SLEEP_STATE_SCHEMA_VERSION
        );
        assert_eq!(shared.schema_version, state.schema_version);
        assert_eq!(shared.reason.as_deref(), Some("shared config"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial]
    fn sleep_state_wrapper_reads_sqlite_backup_and_clear_removes_both() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

        write_sleep_state(60, " sqlite backup ").unwrap();
        std::fs::remove_file(sleep_state_path()).unwrap();

        let from_sqlite = read_sleep_state()
            .unwrap()
            .expect("sleep state should be readable from sqlite backup");
        assert_eq!(from_sqlite.reason.as_deref(), Some("sqlite backup"));

        clear_sleep_state().unwrap();
        assert!(read_sleep_state().unwrap().is_none());
        assert!(
            crate::process_state::sqlite_store::read_state_value::<DaemonSleepState>("sleep-state")
                .unwrap()
                .is_none()
        );
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial]
    fn active_sleep_state_prefers_newer_json_over_expired_sqlite_state() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let now = Utc::now();
        let stale_sqlite = DaemonSleepState {
            schema_version: allthecodes_config::proactive_sleep::SLEEP_STATE_SCHEMA_VERSION,
            sleeping_until: now - chrono::Duration::seconds(1),
            reason: Some("expired sqlite".to_string()),
            updated_at: now - chrono::Duration::seconds(10),
        };
        crate::process_state::sqlite_store::write_state_value("sleep-state", &stale_sqlite)
            .unwrap();
        let json_state = allthecodes_config::proactive_sleep::write_sleep_state_until(
            now + chrono::Duration::seconds(120),
            "json fresh",
        )
        .unwrap();

        let active = active_sleep_state()
            .unwrap()
            .expect("fresh JSON sleep state should remain active");

        assert_eq!(active.reason.as_deref(), Some("json fresh"));
        assert_eq!(active.sleeping_until, json_state.sleeping_until);
        let mirrored =
            crate::process_state::sqlite_store::read_state_value::<DaemonSleepState>("sleep-state")
                .unwrap()
                .expect("fresh sleep state should be mirrored to sqlite");
        assert_eq!(mirrored.reason.as_deref(), Some("json fresh"));
        assert_eq!(mirrored.sleeping_until, json_state.sleeping_until);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial]
    fn proactive_state_prefers_newer_json_over_sqlite_backup() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let stale_sqlite = DaemonProactiveState {
            schema_version: allthecodes_services::proactive::DURABLE_PROACTIVE_STATE_SCHEMA_VERSION,
            active: true,
            next_tick_at: Some(Utc::now() + chrono::Duration::seconds(30)),
            context_blocked: false,
            blocked_reason: None,
            updated_at: Utc::now() - chrono::Duration::seconds(10),
        };
        crate::process_state::sqlite_store::write_state_value("proactive-state", &stale_sqlite)
            .unwrap();
        allthecodes_services::proactive::write_durable_state(false, None).unwrap();

        let current = read_proactive_state()
            .unwrap()
            .expect("proactive state should be readable from JSON");

        assert!(!current.active);
        assert!(current.next_tick_at.is_none());
        let mirrored =
            crate::process_state::sqlite_store::read_state_value::<DaemonProactiveState>(
                "proactive-state",
            )
            .unwrap()
            .expect("fresh proactive state should be mirrored to sqlite");
        assert!(!mirrored.active);
        assert!(mirrored.next_tick_at.is_none());
    }
}

pub fn write_bridge_session_state(
    state: &DaemonBridgeSessionState,
) -> Result<DaemonBridgeSessionState> {
    ensure_daemon_dir()?;
    let mut state = state.clone();
    state.schema_version = SCHEMA_VERSION;
    state.updated_at = Utc::now();
    let path = bridge_session_state_path(&state.session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    atomic_write_json(&path, &state)?;
    Ok(state)
}

pub fn read_bridge_session_state(session_id: &str) -> Result<Option<DaemonBridgeSessionState>> {
    let path = bridge_session_state_path(session_id);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read bridge session state {}", path.display()))?;
    let raw = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse bridge session state {}", path.display()))?;
    migrate_bridge_session_state(raw).map(Some)
}

pub fn list_bridge_session_states() -> Result<Vec<DaemonBridgeSessionState>> {
    let dir = daemon_dir().join("bridge").join("sessions");
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
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read bridge session state {}", path.display()))?;
        let raw = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse bridge session state {}", path.display()))?;
        states.push(migrate_bridge_session_state(raw)?);
    }
    states.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    Ok(states)
}

pub fn find_bridge_session_by_workspace_key(
    workspace_key: &str,
    account_id: Option<&str>,
    profile: Option<&str>,
) -> Result<Option<DaemonBridgeSessionState>> {
    let mut matches: Vec<_> = list_bridge_session_states()?
        .into_iter()
        .filter(|state| state.workspace_key == workspace_key)
        .filter(|state| state.account_id.as_deref() == account_id)
        .filter(|state| state.profile.as_deref() == profile)
        .collect();
    matches.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.session_id.cmp(&left.session_id))
    });
    Ok(matches.into_iter().next())
}

pub fn migrate_bridge_session_state(raw: serde_json::Value) -> Result<DaemonBridgeSessionState> {
    #[derive(Debug, Deserialize)]
    struct RawBridgeSessionState {
        #[serde(default)]
        schema_version: u32,
        session_id: String,
        #[serde(default)]
        account_id: Option<String>,
        #[serde(default)]
        profile: Option<String>,
        cwd: PathBuf,
        assistant_worker_id: String,
        #[serde(default)]
        last_poll_cursor: Option<String>,
        #[serde(default)]
        last_ack_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
        #[serde(default)]
        workspace_key: Option<String>,
        #[serde(default)]
        terminal_id: Option<String>,
        #[serde(default)]
        remote_session_key: Option<String>,
        #[serde(default)]
        assistant_session_id: Option<String>,
        #[serde(default)]
        last_run_id: Option<String>,
        #[serde(default)]
        lease_owner: Option<String>,
        #[serde(default)]
        lease_expires_at: Option<DateTime<Utc>>,
    }

    let raw: RawBridgeSessionState =
        serde_json::from_value(raw).context("failed to deserialize bridge session state")?;
    let _schema_version = raw.schema_version;
    let workspace_key = match raw.workspace_key {
        Some(key) if !key.trim().is_empty() => key,
        _ => crate::bridge_session::derive_workspace_key(
            &crate::bridge_session::BridgeSessionIdentity {
                cwd: raw.cwd.clone(),
                account_id: raw.account_id.clone(),
                profile: raw.profile.clone(),
                terminal_id: raw.terminal_id.clone(),
                remote_session_key: raw.remote_session_key.clone(),
            },
        )?,
    };

    Ok(DaemonBridgeSessionState {
        schema_version: SCHEMA_VERSION,
        session_id: raw.session_id,
        account_id: raw.account_id,
        profile: raw.profile,
        cwd: raw.cwd,
        assistant_worker_id: raw.assistant_worker_id,
        last_poll_cursor: raw.last_poll_cursor,
        last_ack_at: raw.last_ack_at,
        updated_at: raw.updated_at,
        workspace_key,
        terminal_id: raw.terminal_id,
        remote_session_key: raw.remote_session_key,
        assistant_session_id: raw.assistant_session_id,
        last_run_id: raw.last_run_id,
        lease_owner: raw.lease_owner,
        lease_expires_at: raw.lease_expires_at,
    })
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
            Some(pid)
                if process_matches_record(pid, state.process_start_key.as_deref())
                    .is_current_process_record() =>
            {
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
            Some(pid)
                if process_matches_record(pid, state.process_start_key.as_deref())
                    .is_current_process_record() =>
            {
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
fn remove_file_if_exists(path: &Path) -> Result<bool> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(super) fn write_state(state: &DaemonProcessState) -> Result<()> {
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
    #[cfg(not(feature = "sqlite-storage"))]
    let _ = key;
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
    #[cfg(not(feature = "sqlite-storage"))]
    let _ = key;
    removed |= remove_file_if_exists(path)?;
    Ok(removed)
}

pub(super) fn read_worker_state_file(path: &Path) -> Result<DaemonWorkerState> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon worker state {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon worker state {}", path.display()))
}

pub(super) fn ensure_daemon_dir() -> Result<()> {
    fs::create_dir_all(daemon_dir())
        .with_context(|| format!("failed to create {}", daemon_dir().display()))?;
    fs::create_dir_all(workers_dir())
        .with_context(|| format!("failed to create {}", workers_dir().display()))?;
    fs::create_dir_all(logs_dir())
        .with_context(|| format!("failed to create {}", logs_dir().display()))?;
    Ok(())
}

fn current_binary_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}
