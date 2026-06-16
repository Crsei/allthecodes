use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(super) const SCHEMA_VERSION: u32 = 1;
#[cfg(test)]
pub(super) const DEFAULT_DAEMON_PORT: u16 = 19836;

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
    pub(super) fn as_str(&self) -> &'static str {
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
