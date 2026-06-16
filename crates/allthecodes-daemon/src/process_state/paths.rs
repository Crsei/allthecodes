use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use super::data_root;

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
pub(super) fn health_url(port: u16) -> String {
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
