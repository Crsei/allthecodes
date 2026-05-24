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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonCommandKind {
    Submit,
    Abort,
    PermissionResponse,
    AskUserResponse,
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
            Self::Shutdown => "shutdown",
            Self::ReloadConfig => "reload_config",
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
        for command in self.read_worker_commands(worker_id)? {
            if command.status != DaemonCommandStatus::Pending {
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

pub fn event_to_ndjson_line(event: &DaemonEvent) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    Ok(line)
}

pub fn events_from_ndjson(text: &str) -> serde_json::Result<Vec<DaemonEvent>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect()
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
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))?;
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
