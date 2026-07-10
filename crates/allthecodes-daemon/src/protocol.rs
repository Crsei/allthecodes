//! Stable daemon worker command/event file contracts.
//!
//! The full daemon runtime still lives in the root binary during Phase 9. This
//! module owns the reusable wire DTOs and pure format helpers so migrations can
//! verify command JSON files and worker event NDJSON without depending on root
//! runtime modules.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonCommandKind {
    Submit,
    Abort,
    PermissionResponse,
    AskUserResponse,
    Resize,
    Shutdown,
    ReloadConfig,
}

impl DaemonCommandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Abort => "abort",
            Self::PermissionResponse => "permission_response",
            Self::AskUserResponse => "ask_user_response",
            Self::Resize => "resize",
            Self::Shutdown => "shutdown",
            Self::ReloadConfig => "reload_config",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DaemonEventKind {
    CommandAck,
    CommandHandled,
    CommandFailed,
    AbortAck,
    WorkerHeartbeat,
    WorkerShutdownAck,
    SubmitStarted,
    SubmitCompleted,
    PermissionRequest,
    AskUserQuestion,
    HistorySnapshot,
    Unknown(String),
}

impl DaemonEventKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "command_ack" => Self::CommandAck,
            "command_handled" => Self::CommandHandled,
            "command_failed" => Self::CommandFailed,
            "abort_ack" => Self::AbortAck,
            "worker_heartbeat" => Self::WorkerHeartbeat,
            "worker_shutdown_ack" => Self::WorkerShutdownAck,
            "submit_started" => Self::SubmitStarted,
            "submit_completed" => Self::SubmitCompleted,
            "permission_request" => Self::PermissionRequest,
            "ask_user_question" => Self::AskUserQuestion,
            "history_snapshot" => Self::HistorySnapshot,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::CommandAck => "command_ack",
            Self::CommandHandled => "command_handled",
            Self::CommandFailed => "command_failed",
            Self::AbortAck => "abort_ack",
            Self::WorkerHeartbeat => "worker_heartbeat",
            Self::WorkerShutdownAck => "worker_shutdown_ack",
            Self::SubmitStarted => "submit_started",
            Self::SubmitCompleted => "submit_completed",
            Self::PermissionRequest => "permission_request",
            Self::AskUserQuestion => "ask_user_question",
            Self::HistorySnapshot => "history_snapshot",
            Self::Unknown(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonCommandStatus {
    Pending,
    Acked,
    Handled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonCommand {
    pub schema_version: u32,
    pub command_id: String,
    pub idempotency_key: Option<String>,
    pub target_worker_id: String,
    pub kind: DaemonCommandKind,
    pub payload: Value,
    pub status: DaemonCommandStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub acked_at: Option<DateTime<Utc>>,
    pub handled_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub worker_id: String,
    pub command_id: Option<String>,
    pub event_type: String,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

pub fn command_file_name(command_id: &str) -> String {
    format!("{}.json", sanitize_path_component(command_id))
}

pub fn worker_event_file_name(worker_id: &str) -> String {
    format!("{}.ndjson", sanitize_path_component(worker_id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonProtocolStore {
    daemon_dir: PathBuf,
}

impl DaemonProtocolStore {
    pub fn new(daemon_dir: impl Into<PathBuf>) -> Self {
        Self {
            daemon_dir: daemon_dir.into(),
        }
    }

    pub fn daemon_dir(&self) -> &Path {
        &self.daemon_dir
    }

    pub fn commands_dir(&self) -> PathBuf {
        self.daemon_dir.join("commands")
    }

    pub fn worker_commands_dir(&self, worker_id: &str) -> PathBuf {
        self.commands_dir().join(sanitize_path_component(worker_id))
    }

    pub fn command_path(&self, worker_id: &str, command_id: &str) -> PathBuf {
        self.worker_commands_dir(worker_id)
            .join(command_file_name(command_id))
    }

    pub fn events_dir(&self) -> PathBuf {
        self.daemon_dir.join("events")
    }

    pub fn worker_events_path(&self, worker_id: &str) -> PathBuf {
        self.events_dir().join(worker_event_file_name(worker_id))
    }

    pub fn enqueue_command(
        &self,
        target_worker_id: &str,
        kind: DaemonCommandKind,
        payload: Value,
        idempotency_key: Option<String>,
    ) -> Result<DaemonCommand> {
        if let Some(key) = idempotency_key.as_deref() {
            if let Some(existing) = self.find_command_by_idempotency_key(target_worker_id, key)? {
                return Ok(existing);
            }
        }

        let now = Utc::now();
        let command = DaemonCommand {
            schema_version: SCHEMA_VERSION,
            command_id: next_id("cmd"),
            idempotency_key,
            target_worker_id: target_worker_id.to_string(),
            kind,
            payload,
            status: DaemonCommandStatus::Pending,
            created_at: now,
            updated_at: now,
            acked_at: None,
            handled_at: None,
            error: None,
        };
        self.write_command(&command)?;
        Ok(command)
    }

    pub fn read_worker_commands(&self, worker_id: &str) -> Result<Vec<DaemonCommand>> {
        let dir = self.worker_commands_dir(worker_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut commands = Vec::new();
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry =
                entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            commands.push(read_command_file(&path)?);
        }
        commands.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.command_id.cmp(&right.command_id))
        });
        Ok(commands)
    }

    pub fn read_command(&self, worker_id: &str, command_id: &str) -> Result<Option<DaemonCommand>> {
        let path = self.command_path(worker_id, command_id);
        if !path.exists() {
            return Ok(None);
        }
        read_command_file(&path).map(Some)
    }

    pub fn claim_next_pending_command(
        &self,
        worker_id: &str,
        worker_kind: &str,
    ) -> Result<Option<DaemonCommand>> {
        self.claim_next_pending_command_matching(worker_id, worker_kind, |_| true)
    }

    pub fn claim_next_control_command(
        &self,
        worker_id: &str,
        worker_kind: &str,
    ) -> Result<Option<DaemonCommand>> {
        self.claim_next_pending_command_matching(worker_id, worker_kind, |kind| kind.is_control())
    }

    fn claim_next_pending_command_matching(
        &self,
        worker_id: &str,
        worker_kind: &str,
        include: impl Fn(DaemonCommandKind) -> bool,
    ) -> Result<Option<DaemonCommand>> {
        let mut commands = self.read_worker_commands(worker_id)?;
        commands.sort_by_key(|command| command_priority(command.kind));
        for command in commands {
            if command.status != DaemonCommandStatus::Pending {
                continue;
            }
            if !include(command.kind) {
                continue;
            }

            let mut command = transition_command(command, DaemonCommandStatus::Acked, None);
            command.acked_at = Some(Utc::now());
            self.write_command(&command)?;
            self.append_event(
                worker_id,
                Some(&command.command_id),
                "command_ack",
                json!({
                    "kind": command.kind.as_str(),
                    "worker_kind": worker_kind,
                }),
            )?;
            return Ok(Some(command));
        }

        Ok(None)
    }

    pub fn mark_command_handled(&self, command: DaemonCommand) -> Result<DaemonCommand> {
        let mut command = transition_command(command, DaemonCommandStatus::Handled, None);
        command.handled_at = Some(Utc::now());
        self.write_command(&command)?;
        Ok(command)
    }

    pub fn mark_command_failed(
        &self,
        command: DaemonCommand,
        error: impl Into<String>,
    ) -> Result<DaemonCommand> {
        let mut command =
            transition_command(command, DaemonCommandStatus::Failed, Some(error.into()));
        command.handled_at = Some(Utc::now());
        self.write_command(&command)?;
        Ok(command)
    }

    pub fn append_event(
        &self,
        worker_id: &str,
        command_id: Option<&str>,
        event_type: &str,
        data: Value,
    ) -> Result<DaemonEvent> {
        let event = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: next_id("evt"),
            worker_id: worker_id.to_string(),
            command_id: command_id.map(ToOwned::to_owned),
            event_type: event_type.to_string(),
            data,
            created_at: Utc::now(),
        };
        let path = self.worker_events_path(worker_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open daemon event log {}", path.display()))?;
        let line = event_to_ndjson_line(&event)?;
        file.write_all(line.as_bytes())
            .with_context(|| format!("failed to write daemon event log {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync daemon event log {}", path.display()))?;
        Ok(event)
    }

    pub fn read_worker_events(&self, worker_id: &str) -> Result<Vec<DaemonEvent>> {
        let path = self.worker_events_path(worker_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read daemon event log {}", path.display()))?;
        events_from_ndjson(&text)
            .with_context(|| format!("failed to parse daemon event log {}", path.display()))
    }

    fn write_command(&self, command: &DaemonCommand) -> Result<()> {
        atomic_write_json(
            &self.command_path(&command.target_worker_id, &command.command_id),
            command,
        )
    }

    fn find_command_by_idempotency_key(
        &self,
        worker_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<DaemonCommand>> {
        Ok(self
            .read_worker_commands(worker_id)?
            .into_iter()
            .find(|command| command.idempotency_key.as_deref() == Some(idempotency_key)))
    }
}

impl DaemonCommandKind {
    pub(crate) fn is_control(self) -> bool {
        !matches!(self, Self::Submit)
    }
}

fn command_priority(kind: DaemonCommandKind) -> u8 {
    match kind {
        DaemonCommandKind::Shutdown => 0,
        DaemonCommandKind::Abort => 1,
        DaemonCommandKind::PermissionResponse | DaemonCommandKind::AskUserResponse => 2,
        DaemonCommandKind::Resize | DaemonCommandKind::ReloadConfig => 3,
        DaemonCommandKind::Submit => 4,
    }
}

pub fn event_to_ndjson_line(event: &DaemonEvent) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    Ok(line)
}

pub fn events_from_ndjson(text: &str) -> serde_json::Result<Vec<DaemonEvent>> {
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok(event) = serde_json::from_str(line) {
            events.push(event);
        }
    }
    Ok(events)
}

pub fn sanitize_path_component(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn transition_command(
    mut command: DaemonCommand,
    status: DaemonCommandStatus,
    error: Option<String>,
) -> DaemonCommand {
    command.status = status;
    command.updated_at = Utc::now();
    command.error = error;
    command
}

fn read_command_file(path: &Path) -> Result<DaemonCommand> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon command {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon command {}", path.display()))
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        allthecodes_config::paths::set_private_directory_permissions(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))?;
    allthecodes_config::paths::set_private_file_permissions(path)?;
    Ok(())
}

fn next_id(prefix: &str) -> String {
    let now = Utc::now();
    let nanos = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_micros() * 1_000);
    format!("{prefix}-{nanos}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;
    use serial_test::serial;

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    #[test]
    fn worker_command_json_matches_root_file_contract() {
        let command = DaemonCommand {
            schema_version: SCHEMA_VERSION,
            command_id: "cmd-1".to_string(),
            idempotency_key: Some("idem-1".to_string()),
            target_worker_id: "assistant-session-1".to_string(),
            kind: DaemonCommandKind::PermissionResponse,
            payload: json!({
                "decision": "allow",
                "toolUseId": "toolu_1",
                "gateway": { "runId": "run_abc123" }
            }),
            status: DaemonCommandStatus::Acked,
            created_at: ts(1_700_000_000),
            updated_at: ts(1_700_000_010),
            acked_at: Some(ts(1_700_000_005)),
            handled_at: None,
            error: None,
        };

        assert_eq!(
            serde_json::to_value(&command).unwrap(),
            json!({
                "schema_version": 1,
                "command_id": "cmd-1",
                "idempotency_key": "idem-1",
                "target_worker_id": "assistant-session-1",
                "kind": "permission_response",
                "payload": {
                    "decision": "allow",
                    "toolUseId": "toolu_1",
                    "gateway": { "runId": "run_abc123" }
                },
                "status": "acked",
                "created_at": "2023-11-14T22:13:20Z",
                "updated_at": "2023-11-14T22:13:30Z",
                "acked_at": "2023-11-14T22:13:25Z",
                "handled_at": null,
                "error": null
            })
        );
    }

    #[test]
    fn worker_command_json_from_root_contract_deserializes() {
        let command: DaemonCommand = serde_json::from_value(json!({
            "schema_version": 1,
            "command_id": "cmd-2",
            "idempotency_key": null,
            "target_worker_id": "worker/sub",
            "kind": "ask_user_response",
            "payload": { "answer": "yes" },
            "status": "failed",
            "created_at": "2023-11-14T22:13:20Z",
            "updated_at": "2023-11-14T22:13:30Z",
            "acked_at": null,
            "handled_at": "2023-11-14T22:13:30Z",
            "error": "cancelled"
        }))
        .unwrap();

        assert_eq!(command.kind, DaemonCommandKind::AskUserResponse);
        assert_eq!(command.status, DaemonCommandStatus::Failed);
        assert_eq!(command.payload["answer"], "yes");
    }

    #[test]
    fn interaction_command_kinds_have_stable_json_names() {
        assert_eq!(
            serde_json::to_value(DaemonCommandKind::PermissionResponse).unwrap(),
            json!("permission_response")
        );
        assert_eq!(
            serde_json::to_value(DaemonCommandKind::AskUserResponse).unwrap(),
            json!("ask_user_response")
        );
        assert_eq!(
            serde_json::to_value(DaemonCommandKind::Resize).unwrap(),
            json!("resize")
        );
    }

    #[test]
    fn resize_command_json_matches_worker_contract() {
        let command = DaemonCommand {
            schema_version: SCHEMA_VERSION,
            command_id: "cmd-resize".to_string(),
            idempotency_key: Some("resize-120x40".to_string()),
            target_worker_id: "assistant-session-1".to_string(),
            kind: DaemonCommandKind::Resize,
            payload: json!({
                "cols": 120,
                "rows": 40,
                "source": "http",
            }),
            status: DaemonCommandStatus::Pending,
            created_at: ts(1_700_000_000),
            updated_at: ts(1_700_000_000),
            acked_at: None,
            handled_at: None,
            error: None,
        };

        assert_eq!(serde_json::to_value(&command).unwrap()["kind"], "resize");
        assert_eq!(command.payload["cols"], 120);
        assert_eq!(command.payload["rows"], 40);
    }

    #[test]
    fn interaction_event_kinds_have_stable_json_names() {
        assert_eq!(
            DaemonEventKind::PermissionRequest.as_str(),
            "permission_request"
        );
        assert_eq!(
            DaemonEventKind::AskUserQuestion.as_str(),
            "ask_user_question"
        );
        assert_eq!(
            DaemonEventKind::HistorySnapshot.as_str(),
            "history_snapshot"
        );
    }

    #[test]
    fn permission_request_event_json_matches_worker_contract() {
        let event = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-permission".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: Some("cmd-1".to_string()),
            event_type: DaemonEventKind::PermissionRequest.as_str().to_string(),
            data: json!({
                "request_id": "perm-1",
                "tool_use_id": "toolu_1",
                "tool_name": "Bash",
                "input": { "command": "cargo test" },
                "message": "Allow Bash?",
            }),
            created_at: ts(1_700_000_000),
        };

        assert_eq!(
            serde_json::to_value(&event).unwrap()["event_type"],
            "permission_request"
        );
        assert_eq!(event.data["tool_use_id"], "toolu_1");
        assert_eq!(event.data["tool_name"], "Bash");
    }

    #[test]
    fn ask_user_question_event_json_matches_worker_contract() {
        let event = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-ask-user".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: Some("cmd-1".to_string()),
            event_type: DaemonEventKind::AskUserQuestion.as_str().to_string(),
            data: json!({
                "request_id": "ask-1",
                "question": "Which branch should I use?",
                "choices": ["main", "feature"],
            }),
            created_at: ts(1_700_000_000),
        };

        assert_eq!(
            serde_json::to_value(&event).unwrap()["event_type"],
            "ask_user_question"
        );
        assert_eq!(event.data["request_id"], "ask-1");
        assert_eq!(event.data["choices"][0], "main");
    }

    #[test]
    fn history_snapshot_event_json_matches_worker_contract() {
        let event = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-history".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: None,
            event_type: DaemonEventKind::HistorySnapshot.as_str().to_string(),
            data: json!({
                "session_id": "session-1",
                "messages": [
                    { "role": "user", "content": "hello" },
                    { "role": "assistant", "content": "hi" }
                ],
                "cursor": "2",
            }),
            created_at: ts(1_700_000_000),
        };

        assert_eq!(
            serde_json::to_value(&event).unwrap()["event_type"],
            "history_snapshot"
        );
        assert_eq!(event.data["messages"][0]["role"], "user");
        assert_eq!(event.data["cursor"], "2");
    }

    #[test]
    fn worker_event_ndjson_skips_corrupted_lines_and_preserves_unknown_events() {
        let valid = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-history".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: None,
            event_type: DaemonEventKind::HistorySnapshot.as_str().to_string(),
            data: json!({ "messages": [] }),
            created_at: ts(1_700_000_000),
        };
        let unknown = json!({
            "schema_version": 1,
            "event_id": "evt-future",
            "worker_id": "assistant-session-1",
            "command_id": null,
            "event_type": "future_event",
            "data": { "opaque": true },
            "created_at": "2023-11-14T22:13:20Z"
        });
        let text = format!(
            "{}{{\"schema_version\":\n{}\n",
            event_to_ndjson_line(&valid).unwrap(),
            serde_json::to_string(&unknown).unwrap()
        );

        let events = events_from_ndjson(&text).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "history_snapshot");
        assert_eq!(
            DaemonEventKind::parse(&events[1].event_type),
            DaemonEventKind::Unknown("future_event".to_string())
        );
    }

    #[test]
    fn worker_event_ndjson_matches_root_file_contract() {
        let first = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-1".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: Some("cmd-1".to_string()),
            event_type: "command_ack".to_string(),
            data: json!({ "kind": "submit", "worker_kind": "assistant-session" }),
            created_at: ts(1_700_000_000),
        };
        let second = DaemonEvent {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-2".to_string(),
            worker_id: "assistant-session-1".to_string(),
            command_id: None,
            event_type: "worker_heartbeat".to_string(),
            data: json!({}),
            created_at: ts(1_700_000_010),
        };

        let text = format!(
            "{}{}",
            event_to_ndjson_line(&first).unwrap(),
            event_to_ndjson_line(&second).unwrap()
        );
        let events = events_from_ndjson(&text).unwrap();

        assert_eq!(events, vec![first, second]);
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn worker_file_names_use_root_sanitization_rules() {
        assert_eq!(command_file_name("cmd/one:two"), "cmd_one_two.json");
        assert_eq!(
            worker_event_file_name("worker one/../two"),
            "worker_one_.._two.ndjson"
        );
    }

    #[test]
    #[serial]
    fn enqueue_command_deduplicates_idempotency_key() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));

        let first = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "hello" }),
                Some("same-key".to_string()),
            )
            .unwrap();
        let second = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "hello again" }),
                Some("same-key".to_string()),
            )
            .unwrap();

        assert_eq!(first.command_id, second.command_id);
        assert_eq!(
            store
                .read_worker_commands("assistant-session-1")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    #[serial]
    fn claim_next_pending_command_transitions_to_acked() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));
        let command = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "hello" }),
                None,
            )
            .unwrap();

        let claimed = store
            .claim_next_pending_command("assistant-session-1", "assistant-session")
            .unwrap()
            .unwrap();
        let second = store
            .claim_next_pending_command("assistant-session-1", "assistant-session")
            .unwrap();
        let stored = store
            .read_command("assistant-session-1", &command.command_id)
            .unwrap()
            .unwrap();

        assert_eq!(claimed.command_id, command.command_id);
        assert_eq!(stored.status, DaemonCommandStatus::Acked);
        assert!(second.is_none());
        assert!(store
            .read_worker_events("assistant-session-1")
            .unwrap()
            .iter()
            .any(|event| event.event_type == "command_ack"));
    }

    #[test]
    #[serial]
    fn abort_command_is_handled_and_emits_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));
        let command = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Abort,
                json!({}),
                None,
            )
            .unwrap();

        let claimed = store
            .claim_next_pending_command("assistant-session-1", "assistant-session")
            .unwrap()
            .unwrap();
        let handled = store.mark_command_handled(claimed).unwrap();
        store
            .append_event(
                "assistant-session-1",
                Some(&handled.command_id),
                "abort_ack",
                json!({ "handled": true }),
            )
            .unwrap();
        let stored = store
            .read_command("assistant-session-1", &command.command_id)
            .unwrap()
            .unwrap();
        let events = store.read_worker_events("assistant-session-1").unwrap();

        assert_eq!(stored.status, DaemonCommandStatus::Handled);
        assert!(events.iter().any(|event| event.event_type == "abort_ack"));
    }

    #[test]
    #[serial]
    fn control_commands_are_claimed_before_an_older_submit() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));
        let submit = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "slow" }),
                None,
            )
            .unwrap();
        let abort = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Abort,
                json!({}),
                None,
            )
            .unwrap();

        let claimed = store
            .claim_next_pending_command("assistant-session-1", "assistant-session")
            .unwrap()
            .unwrap();

        assert_eq!(claimed.command_id, abort.command_id);
        assert_eq!(
            store
                .read_command("assistant-session-1", &submit.command_id)
                .unwrap()
                .unwrap()
                .status,
            DaemonCommandStatus::Pending
        );
    }

    #[test]
    #[serial]
    fn control_claim_does_not_claim_submit() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));
        store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "slow" }),
                None,
            )
            .unwrap();

        assert!(store
            .claim_next_control_command("assistant-session-1", "assistant-session")
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn active_submit_can_claim_abort_interaction_responses_and_shutdown() {
        let temp = tempfile::tempdir().unwrap();
        let store = DaemonProtocolStore::new(temp.path().join("daemon"));
        let submit = store
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({ "text": "slow" }),
                None,
            )
            .unwrap();
        for kind in [
            DaemonCommandKind::Abort,
            DaemonCommandKind::PermissionResponse,
            DaemonCommandKind::AskUserResponse,
            DaemonCommandKind::Shutdown,
        ] {
            store
                .enqueue_command("assistant-session-1", kind, json!({}), None)
                .unwrap();
        }

        let mut claimed = Vec::new();
        while let Some(command) = store
            .claim_next_control_command("assistant-session-1", "assistant-session")
            .unwrap()
        {
            claimed.push(command.kind);
        }

        assert_eq!(
            claimed,
            vec![
                DaemonCommandKind::Shutdown,
                DaemonCommandKind::Abort,
                DaemonCommandKind::PermissionResponse,
                DaemonCommandKind::AskUserResponse,
            ]
        );
        assert_eq!(
            store
                .read_command("assistant-session-1", &submit.command_id)
                .unwrap()
                .unwrap()
                .status,
            DaemonCommandStatus::Pending
        );
    }

    #[test]
    fn submit_payload_can_carry_optional_gateway_context() {
        let command = DaemonCommand {
            schema_version: SCHEMA_VERSION,
            command_id: "cmd-1".to_string(),
            idempotency_key: None,
            target_worker_id: "assistant-session-1".to_string(),
            kind: DaemonCommandKind::Submit,
            payload: json!({ "text": "hello", "gateway": { "runId": "run_abc123" } }),
            status: DaemonCommandStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            acked_at: None,
            handled_at: None,
            error: None,
        };

        assert_eq!(command.payload["gateway"]["runId"], "run_abc123");
    }
}
