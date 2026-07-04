//! Cross-process daemon supervisor state.
//!
//! This is the Phase 1 durability layer for daemon management. It lets one
//! process publish daemon status under `~/.allthecodes/daemon/` and another process
//! inspect or request shutdown without sharing memory with the daemon runtime.

mod management;
mod paths;
mod platform;
#[cfg(feature = "sqlite-storage")]
mod sqlite_store;
mod storage;
mod types;

#[cfg(test)]
mod tests;

pub use management::try_run_management_command;
pub use paths::{
    control_token_path, daemon_dir, logs_dir, shutdown_request_path, sleep_state_path, state_path,
    worker_log_path, worker_state_path, workers_dir,
};
pub(crate) use paths::{daily_log_path, team_memory_dir};
pub(crate) use platform::{process_is_alive, process_matches_record, terminate_process_tree};
pub use storage::{
    active_sleep_state, cleanup_stale_state_before_start, clear_control_token,
    clear_shutdown_request, clear_sleep_state, read_bridge_session_state, read_control_token,
    read_sleep_state, read_state, read_worker_state, read_worker_states, request_shutdown,
    shutdown_requested, status_snapshot, tail_log, verify_control_token, worker_summaries,
    write_bridge_session_state, write_control_token, write_sleep_state, write_sleep_state_until,
    write_started, write_stopped, write_supervisor_heartbeat, write_worker_heartbeat,
    write_worker_running, write_worker_stale, write_worker_stopped,
};
pub use types::{
    DaemonBridgeSessionState, DaemonControlToken, DaemonProcessState, DaemonRunStatus,
    DaemonShutdownRequest, DaemonSleepState, DaemonStatusSnapshot, DaemonWorkerState,
    DaemonWorkerStatus, DaemonWorkerSummary, ProcessIdentityStatus, StaleStateCleanupReport,
};

pub(crate) fn data_root() -> std::path::PathBuf {
    allthecodes_config::paths::data_root()
}

pub(crate) fn atomic_write_json<T: serde::Serialize>(
    path: &std::path::Path,
    value: &T,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Write;

    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp)
            .with_context(|| format!("failed to create {}", tmp.display()))?;
        let bytes = serde_json::to_vec_pretty(value)?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", tmp.display()))?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first_err) if path.exists() => {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
            std::fs::rename(&tmp, path).with_context(|| {
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
