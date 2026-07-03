#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type EventSeq = u64;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalCreateRequest {
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<TerminalCommandRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_size: Option<TerminalSize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalCommandRequest {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 34,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum TerminalStatus {
    Starting,
    Running,
    Terminating,
    Exited,
    Expired,
    Failed,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalProfileSummary {
    pub id: String,
    pub label: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalProfilesResponse {
    pub profiles: Vec<TerminalProfileSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalSessionsResponse {
    pub sessions: Vec<TerminalSessionSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalHealthResponse {
    pub status: String,
    pub subsystem: String,
    pub active_sessions: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_spawn_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_spawn_error_at: Option<i64>,
    pub can_spawn_profile: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_success_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_failure_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_idempotency_hit_count: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalSessionSnapshot {
    pub id: String,
    pub label: String,
    pub profile: String,
    pub cwd: String,
    pub command: String,
    pub status: TerminalStatus,
    pub pid: u64,
    pub cols: u16,
    pub rows: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub first_available_seq: EventSeq,
    pub latest_seq: EventSeq,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalSessionParams {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalOutputQuery {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<EventSeq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TerminalOutputResponse {
    pub session_id: String,
    pub events: Vec<OutputEvent>,
    pub next_seq: EventSeq,
    pub truncated: bool,
    pub first_available_seq: EventSeq,
    pub status: TerminalStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum OutputStream {
    Stdout,
    Stderr,
    Pty,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct OutputEvent {
    pub seq: EventSeq,
    pub stream: OutputStream,
    pub chunk: String,
    pub timestamp_ms: u64,
    pub process_or_run_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn terminal_create_request_serializes_client_request_id() {
        let request = TerminalCreateRequest {
            profile: "shell".to_string(),
            client_request_id: Some("task-123-shell".to_string()),
            ..TerminalCreateRequest::default()
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["client_request_id"], json!("task-123-shell"));
    }

    #[test]
    fn terminal_health_response_serializes_metrics() {
        let response = TerminalHealthResponse {
            status: "ok".to_string(),
            subsystem: "terminal".to_string(),
            active_sessions: 2,
            last_spawn_error: None,
            last_spawn_error_at: None,
            can_spawn_profile: true,
            create_success_count: Some(3),
            create_failure_count: Some(1),
            create_idempotency_hit_count: Some(2),
        };

        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["create_success_count"], json!(3));
        assert_eq!(value["create_failure_count"], json!(1));
        assert_eq!(value["create_idempotency_hit_count"], json!(2));
    }

    #[test]
    fn terminal_session_snapshot_serializes_optional_recovery_fields() {
        let snapshot = TerminalSessionSnapshot {
            id: "terminal-1".to_string(),
            label: "Shell".to_string(),
            profile: "shell".to_string(),
            cwd: "/workspace".to_string(),
            command: "sh".to_string(),
            status: TerminalStatus::Exited,
            pid: 42,
            cols: 120,
            rows: 34,
            bytes_in: 1,
            bytes_out: 2,
            created_at: 1000,
            updated_at: 2000,
            client_request_id: Some("task-123-shell".to_string()),
            last_error_at: Some(1500),
            lifecycle_reason: Some("process_exited".to_string()),
            exit_code: Some(0),
            error: None,
            first_available_seq: 1,
            latest_seq: 2,
        };

        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["client_request_id"], json!("task-123-shell"));
        assert_eq!(value["last_error_at"], json!(1500));
        assert_eq!(value["lifecycle_reason"], json!("process_exited"));
    }
}
