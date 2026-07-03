use anyhow::{Context, Result};
use tracing::warn;

use crate::record_replay::paths;
use crate::record_replay::types::{
    LegacyMessageRecord, MessageRecord, RecordItem, RecordLine, RecordedMessage, SessionMetaRecord,
    SessionSnapshotRecord,
};
use crate::storage::{self, SerializableMessage, SessionFile};
use allthecodes_types::message::Message;

#[derive(Debug, Clone, Default)]
pub struct LegacyMigrationReport {
    pub session_id: String,
    pub migrated_messages: usize,
    pub legacy_messages: usize,
}

/// Lazy-migrate a legacy JSON session into a rollout JSONL file.
///
/// Reads the old `sessions/<session_id>.json`, generates synthetic record
/// lines with `migrated_from: "legacy_json"`, writes the rollout, and
/// returns the rollout path.
pub fn migrate_legacy_session(session_id: &str) -> Result<std::path::PathBuf> {
    let file = load_legacy_session_file(session_id)?;
    let rollout_path = generate_synthetic_rollout(session_id, &file)?;
    Ok(rollout_path)
}

/// Create a rollout log from an already-typed message list.
///
/// Used for branch/fork sessions so the child can be replayed without relying
/// on the legacy JSON snapshot copied for compatibility.
pub fn create_rollout_from_messages(
    session_id: &str,
    messages: &[Message],
    cwd: &str,
    parent_session_id: Option<String>,
    branch_from_seq: Option<u64>,
) -> Result<std::path::PathBuf> {
    let created_at = chrono::Utc::now();
    let rollout_path = paths::new_rollout_file(session_id, created_at);

    if rollout_path.exists() {
        warn!(
            "Rollout {} already exists; skipping typed rollout creation",
            rollout_path.display()
        );
        return Ok(rollout_path);
    }

    if let Some(parent) = rollout_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create rollout directory {}", parent.display()))?;
    }

    let cwd_path = std::path::Path::new(cwd);
    let workspace_root = storage::workspace_root(cwd_path);
    let mut lines = Vec::with_capacity(messages.len().saturating_add(2));
    lines.push(RecordLine::new(
        session_id,
        0,
        RecordItem::SessionMeta(SessionMetaRecord {
            created_at,
            cwd: cwd.to_string(),
            workspace_key: Some(storage::workspace_key(cwd_path)),
            workspace_root: Some(workspace_root.to_string_lossy().to_string()),
            workspace_name: Some(storage::workspace_name(&workspace_root)),
            model: None,
            config_summary: None,
            parent_session_id: parent_session_id.clone(),
            branch_from_seq,
            migrated_from: None,
        }),
    ));

    let mut snapshot_messages = Vec::with_capacity(messages.len());
    let mut seq = 1_u64;
    for message in messages {
        let recorded = RecordedMessage::from_message(message);
        snapshot_messages.push(recorded.clone());
        lines.push(RecordLine::new(
            session_id,
            seq,
            RecordItem::Message(MessageRecord { message: recorded }),
        ));
        seq = seq.saturating_add(1);
    }
    lines.push(RecordLine::new(
        session_id,
        seq,
        RecordItem::Snapshot(SessionSnapshotRecord {
            message_count: snapshot_messages.len(),
            messages: snapshot_messages,
            hash: None,
        }),
    ));

    write_rollout_lines(&rollout_path, &lines)?;
    crate::record_replay::index::upsert_rollout(
        &crate::record_replay::index::SessionRolloutIndexEntry {
            session_id: session_id.to_string(),
            rollout_path: rollout_path.clone(),
            schema_version: crate::record_replay::RECORD_SCHEMA_VERSION,
            created_at,
            updated_at: chrono::Utc::now(),
            first_seq: 0,
            last_seq: seq,
            event_count: u64::try_from(lines.len()).unwrap_or(u64::MAX),
            status: "active".to_string(),
            parent_session_id,
            branch_from_seq,
            workspace_key: Some(storage::workspace_key(cwd_path)),
            workspace_root: Some(workspace_root.to_string_lossy().to_string()),
            workspace_name: Some(storage::workspace_name(&workspace_root)),
        },
    )?;
    Ok(rollout_path)
}

/// Try to find a rollout for a session. Returns `Some(rollout_path)` if found,
/// `None` if the session has no rollout yet (legacy JSON only).
pub fn find_rollout_for_session(session_id: &str) -> Result<Option<std::path::PathBuf>> {
    // Check the index first (out of band, caller may handle this separately)
    let day_dir = paths::rollouts_dir();
    if !day_dir.exists() {
        return Ok(None);
    }

    // Walk the rollouts directory tree to find matching files
    let walker = walkdir::WalkDir::new(&day_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.contains(session_id))
                .unwrap_or(false)
        });

    for entry in walker {
        let path = entry.path();
        if paths::is_rollout_path(path) {
            return Ok(Some(path.to_path_buf()));
        }
    }

    Ok(None)
}

fn load_legacy_session_file(session_id: &str) -> Result<SessionFile> {
    let path = storage::get_session_file(session_id);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read legacy session file {}", path.display()))?;
    let file: SessionFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse legacy session file {}", path.display()))?;
    Ok(file)
}

fn generate_synthetic_rollout(session_id: &str, file: &SessionFile) -> Result<std::path::PathBuf> {
    let created_at =
        chrono::DateTime::from_timestamp(file.created_at, 0).unwrap_or_else(chrono::Utc::now);
    let rollout_path = paths::new_rollout_file(session_id, created_at);

    if rollout_path.exists() {
        warn!(
            "Rollout {} already exists; skipping migration",
            rollout_path.display()
        );
        return Ok(rollout_path);
    }

    if let Some(parent) = rollout_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create rollout directory {}", parent.display()))?;
    }

    let mut lines = Vec::new();
    let mut snapshot_messages = Vec::new();

    // SessionMeta
    let meta = SessionMetaRecord {
        created_at,
        cwd: file.cwd.clone(),
        workspace_key: None,
        workspace_root: None,
        workspace_name: None,
        model: None,
        config_summary: None,
        parent_session_id: None,
        branch_from_seq: None,
        migrated_from: Some("legacy_json".to_string()),
    };
    lines.push(RecordLine::new(
        session_id,
        0,
        RecordItem::SessionMeta(meta),
    ));

    // Messages
    let mut seq = 1u64;
    for sm in &file.messages {
        let item = serializable_to_record_item(sm);
        if let RecordItem::Message(MessageRecord { message }) = &item {
            snapshot_messages.push(message.clone());
        }
        lines.push(RecordLine::new(session_id, seq, item));
        seq += 1;
    }
    lines.push(RecordLine::new(
        session_id,
        seq,
        RecordItem::Snapshot(SessionSnapshotRecord {
            message_count: snapshot_messages.len(),
            messages: snapshot_messages,
            hash: None,
        }),
    ));

    write_rollout_lines(&rollout_path, &lines)?;

    crate::record_replay::index::upsert_rollout(
        &crate::record_replay::index::SessionRolloutIndexEntry {
            session_id: session_id.to_string(),
            rollout_path: rollout_path.clone(),
            schema_version: crate::record_replay::RECORD_SCHEMA_VERSION,
            created_at,
            updated_at: chrono::Utc::now(),
            first_seq: 0,
            last_seq: seq,
            event_count: u64::try_from(lines.len()).unwrap_or(u64::MAX),
            status: "active".to_string(),
            parent_session_id: None,
            branch_from_seq: None,
            workspace_key: None,
            workspace_root: None,
            workspace_name: None,
        },
    )?;
    Ok(rollout_path)
}

fn write_rollout_lines(path: &std::path::Path, lines: &[RecordLine]) -> Result<()> {
    let json_lines: Vec<String> = lines
        .iter()
        .map(|line| serde_json::to_string(line).expect("failed to serialize record line"))
        .collect();
    let content = format!("{}\n", json_lines.join("\n"));
    std::fs::write(path, &content)
        .with_context(|| format!("Failed to write rollout {}", path.display()))
}

fn serializable_to_record_item(sm: &SerializableMessage) -> RecordItem {
    // Try to convert to a typed Message first; fall back to LegacyMessage.
    // Migration must preserve malformed legacy data instead of fabricating
    // empty/default typed messages.
    let uuid = match uuid::Uuid::parse_str(&sm.uuid) {
        Ok(uuid) => uuid,
        Err(_) => return legacy_message_record(sm),
    };
    let ts = sm.timestamp;

    match sm.msg_type.as_str() {
        "user" => {
            let content = match sm.data.get("content") {
                Some(serde_json::Value::String(s)) => {
                    crate::record_replay::types::RecordedMessageContent::Text(s.clone())
                }
                Some(serde_json::Value::Array(blocks)) => {
                    let content_blocks: Vec<ContentBlock> =
                        match serde_json::from_value(serde_json::Value::Array(blocks.clone())) {
                            Ok(blocks) => blocks,
                            Err(_) => return legacy_message_record(sm),
                        };
                    crate::record_replay::types::RecordedMessageContent::Blocks(content_blocks)
                }
                _ => return legacy_message_record(sm),
            };
            RecordItem::Message(MessageRecord {
                message: crate::record_replay::types::RecordedMessage::User {
                    uuid,
                    timestamp: ts,
                    role: "user".into(),
                    content,
                    is_meta: sm
                        .data
                        .get("is_meta")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                },
            })
        }
        "assistant" => {
            let content: Vec<ContentBlock> = match sm
                .data
                .get("content")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                Some(content) => content,
                None => return legacy_message_record(sm),
            };
            let usage: Option<Usage> = sm
                .data
                .get("usage")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            RecordItem::Message(MessageRecord {
                message: crate::record_replay::types::RecordedMessage::Assistant {
                    uuid,
                    timestamp: ts,
                    role: "assistant".into(),
                    content,
                    usage,
                    stop_reason: sm
                        .data
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: sm
                        .data
                        .get("cost_usd")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                },
            })
        }
        "system" => {
            let content = match sm.data.get("content").and_then(|v| v.as_str()) {
                Some(content) => content.to_string(),
                None => return legacy_message_record(sm),
            };
            let subtype = match sm.data.get("subtype").and_then(|v| v.as_str()) {
                Some("CompactBoundary") => {
                    crate::record_replay::types::RecordedSystemSubtype::CompactBoundary {
                        compact_metadata: sm
                            .data
                            .get("compact_metadata")
                            .and_then(|v| serde_json::from_value(v.clone()).ok()),
                    }
                }
                Some("LocalCommand") => {
                    let content = match sm
                        .data
                        .get("local_command_content")
                        .and_then(|v| v.as_str())
                    {
                        Some(content) => content.to_string(),
                        None => return legacy_message_record(sm),
                    };
                    crate::record_replay::types::RecordedSystemSubtype::LocalCommand { content }
                }
                Some("Warning") => crate::record_replay::types::RecordedSystemSubtype::Warning,
                _ => crate::record_replay::types::RecordedSystemSubtype::Informational {
                    level: crate::record_replay::types::RecordedInfoLevel::Info,
                },
            };
            RecordItem::Message(MessageRecord {
                message: crate::record_replay::types::RecordedMessage::System {
                    uuid,
                    timestamp: ts,
                    subtype,
                    content,
                },
            })
        }
        "progress" => {
            let tool_use_id = match sm.data.get("tool_use_id").and_then(|v| v.as_str()) {
                Some(tool_use_id) => tool_use_id.to_string(),
                None => return legacy_message_record(sm),
            };
            let data = match sm.data.get("data").cloned() {
                Some(data) => data,
                None => return legacy_message_record(sm),
            };
            RecordItem::Message(MessageRecord {
                message: crate::record_replay::types::RecordedMessage::Progress {
                    uuid,
                    timestamp: ts,
                    tool_use_id,
                    data,
                },
            })
        }
        "attachment" => {
            let attachment: Attachment = match sm
                .data
                .get("attachment")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                Some(attachment) => attachment,
                None => return legacy_message_record(sm),
            };
            RecordItem::Message(MessageRecord {
                message: crate::record_replay::types::RecordedMessage::Attachment {
                    uuid,
                    timestamp: ts,
                    attachment,
                },
            })
        }
        _ => legacy_message_record(sm),
    }
}

fn legacy_message_record(sm: &SerializableMessage) -> RecordItem {
    RecordItem::LegacyMessage(LegacyMessageRecord {
        msg_type: sm.msg_type.clone(),
        uuid: sm.uuid.clone(),
        timestamp: sm.timestamp,
        data: sm.data.clone(),
    })
}

use allthecodes_types::message::{Attachment, ContentBlock, Usage};

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn migrate_legacy_session_creates_rollout() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        // Write a legacy session JSON
        let file = storage::SessionFile {
            session_id: "legacy-1".into(),
            created_at: 1_700_000_000,
            last_modified: 1_700_000_000,
            cwd: "/repo".into(),
            custom_title: None,
            chat_mode_override: None,
            messages: vec![
                SerializableMessage {
                    msg_type: "user".into(),
                    uuid: "10000000-0000-0000-0000-000000000001".into(),
                    timestamp: 1,
                    data: serde_json::json!({
                        "content": "hello",
                        "is_meta": false,
                    }),
                },
                SerializableMessage {
                    msg_type: "assistant".into(),
                    uuid: "10000000-0000-0000-0000-000000000002".into(),
                    timestamp: 2,
                    data: serde_json::json!({
                        "content": [{"type": "text", "text": "hi"}],
                        "stop_reason": "end_turn",
                        "cost_usd": 0.0,
                    }),
                },
            ],
        };
        std::fs::create_dir_all(storage::get_session_dir()).unwrap();
        let json = serde_json::to_string_pretty(&file).unwrap();
        std::fs::write(storage::get_session_file("legacy-1"), json).unwrap();

        let rollout_path = migrate_legacy_session("legacy-1").unwrap();
        assert!(rollout_path.exists());
        let content = std::fs::read_to_string(&rollout_path).unwrap();
        assert!(content.ends_with('\n'));

        let result = crate::record_replay::reader::read_rollout_file(&rollout_path).unwrap();
        assert_eq!(result.lines.len(), 4);
        assert_eq!(result.lines[0].seq, 0);
        assert_eq!(result.lines[1].seq, 1);
        assert_eq!(result.lines[2].seq, 2);
        assert_eq!(result.lines[3].seq, 3);
        assert!(matches!(
            &result.lines[3].item,
            RecordItem::Snapshot(snapshot)
                if snapshot.message_count == 2 && snapshot.messages.len() == 2
        ));

        // Reconstruct messages and verify they match
        let messages = crate::record_replay::reconstruct::reconstruct_messages(&result.lines);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[0],
            allthecodes_types::message::Message::User(u) if !u.is_meta
        ));
        assert!(matches!(
            &messages[1],
            allthecodes_types::message::Message::Assistant(_)
        ));
    }

    #[test]
    #[serial]
    fn migrate_legacy_message_falls_back_to_legacy_message_type() {
        // When serializable_to_record_item encounters an unknown msg_type, it
        // must produce RecordItem::LegacyMessage rather than failing.
        let sm = SerializableMessage {
            msg_type: "unknown_message_type".into(),
            uuid: "10000000-0000-0000-0000-000000000099".into(),
            timestamp: 99,
            data: serde_json::json!({"some_arbitrary_field": "value"}),
        };

        let item = serializable_to_record_item(&sm);

        match &item {
            RecordItem::LegacyMessage(legacy) => {
                assert_eq!(legacy.msg_type, "unknown_message_type");
                assert_eq!(legacy.uuid, "10000000-0000-0000-0000-000000000099");
                assert_eq!(legacy.timestamp, 99);
                assert_eq!(
                    legacy.data.get("some_arbitrary_field"),
                    Some(&serde_json::Value::String("value".into()))
                );
            }
            other => {
                panic!("Expected RecordItem::LegacyMessage for unknown msg_type, got {other:?}")
            }
        }
    }

    #[test]
    #[serial]
    fn migrate_partial_message_creates_legacy_message_record() {
        // A message that is structurally malformed for its known msg_type
        // should still be preserved as a LegacyMessage record instead of
        // panicking, silently skipping, or fabricating empty/default typed data.
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        // Build a session where one message has missing required fields
        // (no "content" for a "user" message).
        let file = storage::SessionFile {
            session_id: "partial-msg-1".into(),
            created_at: 1_700_000_000,
            last_modified: 1_700_000_000,
            cwd: "/repo".into(),
            custom_title: None,
            chat_mode_override: None,
            messages: vec![
                // Valid user message
                SerializableMessage {
                    msg_type: "user".into(),
                    uuid: "10000000-0000-0000-0000-000000000001".into(),
                    timestamp: 1,
                    data: serde_json::json!({
                        "content": "hello",
                        "is_meta": false,
                    }),
                },
                // User message with empty data (partial/corrupt)
                SerializableMessage {
                    msg_type: "user".into(),
                    uuid: "10000000-0000-0000-0000-000000000002".into(),
                    timestamp: 2,
                    data: serde_json::json!({}),
                },
                // Unknown msg_type (should produce LegacyMessage)
                SerializableMessage {
                    msg_type: "proprietary_ext".into(),
                    uuid: "10000000-0000-0000-0000-000000000003".into(),
                    timestamp: 3,
                    data: serde_json::json!({"vendor_blob": [1, 2, 3]}),
                },
            ],
        };
        std::fs::create_dir_all(storage::get_session_dir()).unwrap();
        let json = serde_json::to_string_pretty(&file).unwrap();
        std::fs::write(storage::get_session_file("partial-msg-1"), json).unwrap();

        // Migration must not panic
        let rollout_path = migrate_legacy_session("partial-msg-1").unwrap();
        assert!(rollout_path.exists());

        let result = crate::record_replay::reader::read_rollout_file(&rollout_path).unwrap();
        // SessionMeta (seq 0) + 3 messages (seq 1,2,3) + Snapshot (seq 4) = 5 lines
        assert_eq!(result.lines.len(), 5);

        // seq 1: valid user message -> typed Message (user)
        assert_eq!(result.lines[1].seq, 1);
        assert!(matches!(
            &result.lines[1].item,
            RecordItem::Message(MessageRecord {
                message: RecordedMessage::User { .. },
            })
        ));

        // seq 2: partial user message -> LegacyMessage preserving raw data.
        assert_eq!(result.lines[2].seq, 2);
        assert!(matches!(
            &result.lines[2].item,
            RecordItem::LegacyMessage(LegacyMessageRecord {
                msg_type,
                data,
                ..
            }) if msg_type == "user" && data.as_object().is_some_and(|obj| obj.is_empty())
        ));

        // seq 3: unknown msg_type -> LegacyMessage
        assert_eq!(result.lines[3].seq, 3);
        assert!(matches!(
            &result.lines[3].item,
            RecordItem::LegacyMessage(LegacyMessageRecord {
                msg_type,
                ..
            }) if msg_type == "proprietary_ext"
        ));

        // seq 4: Snapshot
        assert_eq!(result.lines[4].seq, 4);
        assert!(matches!(&result.lines[4].item, RecordItem::Snapshot(_)));
    }

    #[test]
    #[serial]
    fn migrate_legacy_session_creates_rollout_with_migration_metadata() {
        // Verify that the SessionMeta line in the synthetic rollout contains
        // "migrated_from" and that the final line is a Snapshot.
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let file = storage::SessionFile {
            session_id: "meta-check-1".into(),
            created_at: 1_700_000_000,
            last_modified: 1_700_000_000,
            cwd: "/workspace".into(),
            custom_title: None,
            chat_mode_override: None,
            messages: vec![
                SerializableMessage {
                    msg_type: "user".into(),
                    uuid: "10000000-0000-0000-0000-000000000001".into(),
                    timestamp: 1,
                    data: serde_json::json!({
                        "content": "hi",
                        "is_meta": false,
                    }),
                },
                SerializableMessage {
                    msg_type: "assistant".into(),
                    uuid: "10000000-0000-0000-0000-000000000002".into(),
                    timestamp: 2,
                    data: serde_json::json!({
                        "content": [{"type": "text", "text": "hello"}],
                        "stop_reason": "end_turn",
                        "cost_usd": 0.0,
                    }),
                },
            ],
        };
        std::fs::create_dir_all(storage::get_session_dir()).unwrap();
        std::fs::write(
            storage::get_session_file("meta-check-1"),
            serde_json::to_string_pretty(&file).unwrap(),
        )
        .unwrap();

        let rollout_path = migrate_legacy_session("meta-check-1").unwrap();
        assert!(rollout_path.exists());

        let content = std::fs::read_to_string(&rollout_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // First line (SessionMeta) must contain "migrated_from"
        assert!(
            lines[0].contains("migrated_from"),
            "SessionMeta line should contain migrated_from: {}",
            lines[0]
        );

        // Confirm "migrated_from" value is "legacy_json"
        let first_line: serde_json::Value =
            serde_json::from_str(lines[0]).expect("first line should be valid JSON");
        assert_eq!(
            first_line["item"]["migrated_from"], "legacy_json",
            "SessionMeta item should have migrated_from set to legacy_json"
        );

        // Last line must be a Snapshot
        let last_line: serde_json::Value =
            serde_json::from_str(lines.last().unwrap()).expect("last line should be valid JSON");
        assert_eq!(
            last_line["item"]["type"], "snapshot",
            "Last line item type should be snapshot"
        );
    }

    #[test]
    #[serial]
    fn migrate_legacy_session_keeps_original_file() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        // Write a legacy session JSON
        let file = storage::SessionFile {
            session_id: "legacy-keep-1".into(),
            created_at: 1_700_000_000,
            last_modified: 1_700_000_000,
            cwd: "/repo".into(),
            custom_title: None,
            chat_mode_override: None,
            messages: vec![SerializableMessage {
                msg_type: "user".into(),
                uuid: "10000000-0000-0000-0000-000000000001".into(),
                timestamp: 1,
                data: serde_json::json!({
                    "content": "hello",
                    "is_meta": false,
                }),
            }],
        };
        std::fs::create_dir_all(storage::get_session_dir()).unwrap();
        let json = serde_json::to_string_pretty(&file).unwrap();
        let legacy_path = storage::get_session_file("legacy-keep-1");
        std::fs::write(&legacy_path, &json).unwrap();

        assert!(
            legacy_path.exists(),
            "original JSON should exist before migration"
        );

        let rollout_path = migrate_legacy_session("legacy-keep-1").unwrap();
        assert!(rollout_path.exists());
        // The original JSON file must NOT have been deleted
        assert!(
            legacy_path.exists(),
            "original JSON file must still exist after migration"
        );
    }
}
