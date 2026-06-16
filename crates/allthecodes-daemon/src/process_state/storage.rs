use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
#[cfg(feature = "sqlite-storage")]
use tracing::warn;

use super::atomic_write_json;
use super::paths::{
    control_token_path, daemon_dir, health_url, logs_dir, shutdown_request_path, sleep_state_path,
    state_path, worker_state_path, workers_dir,
};
use super::platform::process_is_alive;
#[cfg(feature = "sqlite-storage")]
use super::sqlite_store;
use super::types::{
    DaemonControlToken, DaemonProcessState, DaemonRunStatus, DaemonShutdownRequest,
    DaemonSleepState, DaemonStatusSnapshot, DaemonWorkerState, DaemonWorkerStatus,
    DaemonWorkerSummary, StaleStateCleanupReport, SCHEMA_VERSION,
};

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
