use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(super) const SCHEMA_VERSION: u32 = 2;
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
    #[serde(default)]
    pub command_kind: Option<String>,
    #[serde(default)]
    pub binary_version: Option<String>,
    #[serde(default)]
    pub binary_path: Option<PathBuf>,
    #[serde(default)]
    pub log_path: Option<PathBuf>,
    #[serde(default)]
    pub ready_url: Option<String>,
    #[serde(default)]
    pub process_start_key: Option<String>,
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
    #[serde(default)]
    pub command_kind: Option<String>,
    #[serde(default)]
    pub binary_version: Option<String>,
    #[serde(default)]
    pub binary_path: Option<PathBuf>,
    #[serde(default)]
    pub ready_url: Option<String>,
    #[serde(default)]
    pub process_start_key: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonBridgeSessionState {
    pub schema_version: u32,
    pub session_id: String,
    pub account_id: Option<String>,
    pub profile: Option<String>,
    pub cwd: PathBuf,
    pub assistant_worker_id: String,
    pub last_poll_cursor: Option<String>,
    pub last_ack_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub workspace_key: String,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub remote_session_key: Option<String>,
    #[serde(default)]
    pub assistant_session_id: Option<String>,
    #[serde(default)]
    pub last_run_id: Option<String>,
    #[serde(default)]
    pub lease_owner: Option<String>,
    #[serde(default)]
    pub lease_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatusSnapshot {
    Running(DaemonProcessState),
    Stale(DaemonProcessState),
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessIdentityStatus {
    Dead,
    Matched,
    Mismatched {
        expected: String,
        actual: Option<String>,
    },
    Unknown,
    Unrecorded,
}

impl ProcessIdentityStatus {
    pub fn as_diagnostic(&self) -> String {
        match self {
            Self::Dead => "identity=dead".to_string(),
            Self::Matched => "identity=matched".to_string(),
            Self::Mismatched { expected, actual } => format!(
                "identity=pid_reused expected={} actual={}",
                expected,
                actual.as_deref().unwrap_or("unknown")
            ),
            Self::Unknown => "identity=unknown".to_string(),
            Self::Unrecorded => "identity=unrecorded".to_string(),
        }
    }

    pub fn is_current_process_record(&self) -> bool {
        matches!(self, Self::Matched | Self::Unknown | Self::Unrecorded)
    }
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
