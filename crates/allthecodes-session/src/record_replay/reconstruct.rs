use std::collections::{HashMap, VecDeque};

use allthecodes_types::message::{
    AssistantMessage, AttachmentMessage, InfoLevel, Message, MessageContent, ProgressMessage,
    SystemMessage, SystemSubtype, UserMessage,
};

use super::types::{
    ArtifactCreatedRecord, LegacyMessageRecord, MessageRecord, RecordItem, RecordLine,
    RecordedMessage, RecordedMessageContent, RecordedSystemSubtype, SessionMetaRecord,
    SessionReportGeneratedRecord, VerificationEvidenceRecord, VerificationFinishedRecord,
    VerificationStartedRecord,
};
use crate::storage::SerializableMessage;

#[derive(Debug, Clone, Default)]
pub struct ReconstructedRecordSession {
    pub session_id: String,
    pub metadata: Option<SessionMetaRecord>,
    pub message_records: Vec<ReconstructedRecordMessage>,
    pub messages: Vec<RecordedMessage>,
    pub last_seq: u64,
    pub pending_interactions: Vec<PendingInteraction>,
    pub verification_started: Vec<IndexedRecord<VerificationStartedRecord>>,
    pub verification_evidence: Vec<IndexedRecord<VerificationEvidenceRecord>>,
    pub verification_finished: Vec<IndexedRecord<VerificationFinishedRecord>>,
    pub artifacts: Vec<IndexedRecord<ArtifactCreatedRecord>>,
    pub reports: Vec<IndexedRecord<SessionReportGeneratedRecord>>,
}

#[derive(Debug, Clone)]
pub struct IndexedRecord<T> {
    pub seq: u64,
    pub record: T,
}

#[derive(Debug, Clone)]
pub struct ReconstructedRecordMessage {
    pub seq: u64,
    pub message: RecordedMessage,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PendingInteraction {
    Permission {
        seq: u64,
        request_id: String,
        tool_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<serde_json::Value>,
    },
    Question {
        seq: u64,
        request_id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<serde_json::Value>,
    },
}

pub fn reconstruct_recorded_messages(lines: &[RecordLine]) -> ReconstructedRecordSession {
    let mut session = ReconstructedRecordSession::default();
    let mut visible_messages: Vec<ReconstructedRecordMessage> = Vec::new();
    let mut pending_permissions: HashMap<String, PendingInteraction> = HashMap::new();
    let mut pending_questions: HashMap<String, PendingInteraction> = HashMap::new();
    for line in lines {
        if session.session_id.is_empty() {
            session.session_id = line.session_id.clone();
        }
        session.last_seq = line.seq;
        match &line.item {
            RecordItem::SessionMeta(metadata) if session.metadata.is_none() => {
                session.metadata = Some(metadata.clone());
            }
            RecordItem::Message(MessageRecord { message }) => {
                visible_messages.push(ReconstructedRecordMessage {
                    seq: line.seq,
                    message: message.clone(),
                });
            }
            RecordItem::LegacyMessage(legacy) => {
                for message in legacy_message_to_recorded(legacy) {
                    visible_messages.push(ReconstructedRecordMessage {
                        seq: line.seq,
                        message,
                    });
                }
            }
            RecordItem::Snapshot(snapshot) => {
                visible_messages = snapshot_messages_with_existing_sequences(
                    &visible_messages,
                    &snapshot.messages,
                    line.seq,
                );
            }
            RecordItem::Rollback(rollback) => {
                visible_messages.retain(|entry| entry.seq <= rollback.target_seq);
                pending_permissions.retain(|_, pending| pending.seq() <= rollback.target_seq);
                pending_questions.retain(|_, pending| pending.seq() <= rollback.target_seq);
                session
                    .verification_started
                    .retain(|record| record.seq <= rollback.target_seq);
                session
                    .verification_evidence
                    .retain(|record| record.seq <= rollback.target_seq);
                session
                    .verification_finished
                    .retain(|record| record.seq <= rollback.target_seq);
                session
                    .artifacts
                    .retain(|record| record.seq <= rollback.target_seq);
                session
                    .reports
                    .retain(|record| record.seq <= rollback.target_seq);
            }
            RecordItem::VerificationStarted(record) => {
                session.verification_started.push(IndexedRecord {
                    seq: line.seq,
                    record: record.clone(),
                });
            }
            RecordItem::VerificationEvidence(record) => {
                session.verification_evidence.push(IndexedRecord {
                    seq: line.seq,
                    record: record.clone(),
                });
            }
            RecordItem::VerificationFinished(record) => {
                session.verification_finished.push(IndexedRecord {
                    seq: line.seq,
                    record: record.clone(),
                });
            }
            RecordItem::ArtifactCreated(record) => {
                session.artifacts.push(IndexedRecord {
                    seq: line.seq,
                    record: record.clone(),
                });
            }
            RecordItem::SessionReportGenerated(record) => {
                session.reports.push(IndexedRecord {
                    seq: line.seq,
                    record: record.clone(),
                });
            }
            RecordItem::PermissionRequest(request) => {
                pending_permissions.insert(
                    request.request_id.clone(),
                    PendingInteraction::Permission {
                        seq: line.seq,
                        request_id: request.request_id.clone(),
                        tool_name: request.tool_name.clone(),
                        context: request.context.clone(),
                    },
                );
            }
            RecordItem::PermissionResponse(response) => {
                pending_permissions.remove(&response.request_id);
            }
            RecordItem::QuestionRequest(request) => {
                pending_questions.insert(
                    request.request_id.clone(),
                    PendingInteraction::Question {
                        seq: line.seq,
                        request_id: request.request_id.clone(),
                        prompt: request.prompt.clone(),
                        options: request.options.clone(),
                    },
                );
            }
            RecordItem::QuestionResponse(response) => {
                pending_questions.remove(&response.request_id);
            }
            _ => {}
        }
    }
    session.message_records = visible_messages.clone();
    session.messages = visible_messages
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    session.pending_interactions = pending_permissions
        .into_values()
        .chain(pending_questions.into_values())
        .collect();
    session
}

impl PendingInteraction {
    pub fn seq(&self) -> u64 {
        match self {
            PendingInteraction::Permission { seq, .. }
            | PendingInteraction::Question { seq, .. } => *seq,
        }
    }
}

fn snapshot_messages_with_existing_sequences(
    existing: &[ReconstructedRecordMessage],
    snapshot_messages: &[RecordedMessage],
    snapshot_seq: u64,
) -> Vec<ReconstructedRecordMessage> {
    let mut seqs_by_uuid: HashMap<uuid::Uuid, VecDeque<u64>> = HashMap::new();
    for entry in existing {
        seqs_by_uuid
            .entry(recorded_message_uuid(&entry.message))
            .or_default()
            .push_back(entry.seq);
    }

    snapshot_messages
        .iter()
        .cloned()
        .map(|message| {
            let seq = seqs_by_uuid
                .get_mut(&recorded_message_uuid(&message))
                .and_then(VecDeque::pop_front)
                .unwrap_or(snapshot_seq);
            ReconstructedRecordMessage { seq, message }
        })
        .collect()
}

pub fn message_seq_for_uuid(lines: &[RecordLine], message_uuid: &str) -> Option<u64> {
    let uuid = uuid::Uuid::parse_str(message_uuid).ok()?;
    reconstruct_recorded_messages(lines)
        .message_records
        .into_iter()
        .find_map(|entry| (recorded_message_uuid(&entry.message) == uuid).then_some(entry.seq))
}

pub fn recorded_message_uuid(message: &RecordedMessage) -> uuid::Uuid {
    match message {
        RecordedMessage::User { uuid, .. }
        | RecordedMessage::Assistant { uuid, .. }
        | RecordedMessage::System { uuid, .. }
        | RecordedMessage::Progress { uuid, .. }
        | RecordedMessage::Attachment { uuid, .. } => *uuid,
    }
}

fn legacy_message_to_recorded(legacy: &LegacyMessageRecord) -> Vec<RecordedMessage> {
    let serializable = SerializableMessage {
        msg_type: legacy.msg_type.clone(),
        uuid: legacy.uuid.clone(),
        timestamp: legacy.timestamp,
        data: legacy.data.clone(),
    };
    crate::storage::serializable_to_messages(&[serializable])
        .into_iter()
        .map(|message| RecordedMessage::from_message(&message))
        .collect()
}

/// Reconstruct a full `Vec<Message>` from RecordLine slices.
///
/// Converts each `RecordedMessage` variant into the corresponding `Message`
/// variant so callers can feed the result directly into `QueryEngine` or
/// `storage::save_session`.
pub fn reconstruct_messages(lines: &[RecordLine]) -> Vec<Message> {
    let recorded = reconstruct_recorded_messages(lines);
    recorded_messages_to_typed(&recorded.messages)
}

/// Convert a slice of `RecordedMessage` into typed `Message` values.
pub fn recorded_messages_to_typed(msgs: &[RecordedMessage]) -> Vec<Message> {
    msgs.iter().map(recorded_message_to_typed).collect()
}

fn recorded_message_to_typed(rm: &RecordedMessage) -> Message {
    match rm {
        RecordedMessage::User {
            uuid,
            timestamp,
            role,
            content,
            is_meta,
            tool_use_result,
            source_tool_assistant_uuid,
        } => Message::User(UserMessage {
            uuid: *uuid,
            timestamp: *timestamp,
            role: role.clone(),
            content: recorded_content_to_typed(content),
            is_meta: *is_meta,
            tool_use_result: tool_use_result.clone(),
            source_tool_assistant_uuid: *source_tool_assistant_uuid,
        }),
        RecordedMessage::Assistant {
            uuid,
            timestamp,
            role,
            content,
            usage,
            stop_reason,
            is_api_error_message,
            api_error,
            cost_usd,
        } => Message::Assistant(AssistantMessage {
            uuid: *uuid,
            timestamp: *timestamp,
            role: role.clone(),
            content: content.clone(),
            usage: usage.clone(),
            stop_reason: stop_reason.clone(),
            is_api_error_message: *is_api_error_message,
            api_error: api_error.clone(),
            cost_usd: *cost_usd,
        }),
        RecordedMessage::System {
            uuid,
            timestamp,
            subtype,
            content,
        } => Message::System(SystemMessage {
            uuid: *uuid,
            timestamp: *timestamp,
            subtype: recorded_system_subtype_to_typed(subtype),
            content: content.clone(),
        }),
        RecordedMessage::Progress {
            uuid,
            timestamp,
            tool_use_id,
            data,
        } => Message::Progress(ProgressMessage {
            uuid: *uuid,
            timestamp: *timestamp,
            tool_use_id: tool_use_id.clone(),
            data: data.clone(),
        }),
        RecordedMessage::Attachment {
            uuid,
            timestamp,
            attachment,
        } => Message::Attachment(AttachmentMessage {
            uuid: *uuid,
            timestamp: *timestamp,
            attachment: attachment.clone(),
        }),
    }
}

fn recorded_content_to_typed(content: &RecordedMessageContent) -> MessageContent {
    match content {
        RecordedMessageContent::Text(text) => MessageContent::Text(text.clone()),
        RecordedMessageContent::Blocks(blocks) => MessageContent::Blocks(blocks.clone()),
    }
}

fn recorded_system_subtype_to_typed(subtype: &RecordedSystemSubtype) -> SystemSubtype {
    match subtype {
        RecordedSystemSubtype::CompactBoundary { compact_metadata } => {
            SystemSubtype::CompactBoundary {
                compact_metadata: compact_metadata.clone(),
            }
        }
        RecordedSystemSubtype::MicrocompactBoundary {
            microcompact_metadata,
        } => SystemSubtype::MicrocompactBoundary {
            microcompact_metadata: microcompact_metadata.clone(),
        },
        RecordedSystemSubtype::ApiError {
            retry_attempt,
            max_retries,
            retry_in_ms,
            error,
        } => SystemSubtype::ApiError {
            retry_attempt: *retry_attempt,
            max_retries: *max_retries,
            retry_in_ms: *retry_in_ms,
            error: allthecodes_types::message::ApiErrorInfo {
                status: error.status,
                message: error.message.clone(),
            },
        },
        RecordedSystemSubtype::Informational { level } => {
            let info_level = match level {
                crate::record_replay::types::RecordedInfoLevel::Info => InfoLevel::Info,
                crate::record_replay::types::RecordedInfoLevel::Warning => InfoLevel::Warning,
                crate::record_replay::types::RecordedInfoLevel::Error => InfoLevel::Error,
            };
            SystemSubtype::Informational { level: info_level }
        }
        RecordedSystemSubtype::LocalCommand { content } => SystemSubtype::LocalCommand {
            content: content.clone(),
        },
        RecordedSystemSubtype::Warning => SystemSubtype::Warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_replay::reader::{ReplayReadResult, ReplayReader};
    use crate::record_replay::types::{
        PermissionRequestRecord, RollbackRecord, SessionSnapshotRecord,
    };

    #[test]
    fn reconstructs_legacy_message_records() {
        let line = RecordLine::new(
            "legacy-record-session",
            1,
            RecordItem::LegacyMessage(LegacyMessageRecord {
                msg_type: "user".into(),
                uuid: "10000000-0000-0000-0000-000000000010".into(),
                timestamp: 42,
                data: serde_json::json!({
                    "content": "hello from legacy",
                    "is_meta": false,
                }),
            }),
        );

        let messages = reconstruct_messages(&[line]);

        assert_eq!(messages.len(), 1);
        assert!(matches!(
            &messages[0],
            Message::User(user)
                if user.uuid.to_string() == "10000000-0000-0000-0000-000000000010"
                    && user.timestamp == 42
                    && matches!(&user.content, MessageContent::Text(text) if text == "hello from legacy")
        ));
    }

    #[test]
    fn snapshot_preserves_prior_message_sequences_for_rollback() {
        let first = RecordedMessage::User {
            uuid: uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000011").unwrap(),
            timestamp: 1,
            role: "user".into(),
            content: RecordedMessageContent::Text("first".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        };
        let second = RecordedMessage::User {
            uuid: uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000012").unwrap(),
            timestamp: 2,
            role: "user".into(),
            content: RecordedMessageContent::Text("second".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        };
        let lines = vec![
            RecordLine::new(
                "snapshot-session",
                1,
                RecordItem::Message(MessageRecord {
                    message: first.clone(),
                }),
            ),
            RecordLine::new(
                "snapshot-session",
                2,
                RecordItem::Message(MessageRecord {
                    message: second.clone(),
                }),
            ),
            RecordLine::new(
                "snapshot-session",
                3,
                RecordItem::Snapshot(SessionSnapshotRecord {
                    message_count: 2,
                    messages: vec![first, second],
                    hash: None,
                }),
            ),
            RecordLine::new(
                "snapshot-session",
                4,
                RecordItem::Rollback(RollbackRecord {
                    target_seq: 1,
                    reason: None,
                }),
            ),
        ];

        let reconstructed = reconstruct_recorded_messages(&lines);

        assert_eq!(reconstructed.message_records.len(), 1);
        assert_eq!(reconstructed.message_records[0].seq, 1);
        assert_eq!(
            recorded_message_uuid(&reconstructed.message_records[0].message).to_string(),
            "10000000-0000-0000-0000-000000000011"
        );
    }

    #[test]
    fn tracks_pending_permission_requests() {
        let lines = vec![RecordLine::new(
            "pending-session",
            1,
            RecordItem::PermissionRequest(PermissionRequestRecord {
                request_id: "perm-1".into(),
                tool_name: "Write".into(),
                context: Some(serde_json::json!({ "path": "src/lib.rs" })),
            }),
        )];

        let reconstructed = reconstruct_recorded_messages(&lines);

        assert_eq!(reconstructed.pending_interactions.len(), 1);
        assert!(matches!(
            &reconstructed.pending_interactions[0],
            PendingInteraction::Permission { request_id, tool_name, .. }
                if request_id == "perm-1" && tool_name == "Write"
        ));
    }

    #[test]
    fn resume_with_corrupt_tail_does_not_panic() {
        // Simulate a file with valid JSONL lines followed by a partial last line.
        // The reader should skip the corrupt tail with a warning.
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("corrupt-tail.jsonl");

        let valid_lines = [
            RecordLine::new("corrupt-tail", 0, {
                RecordItem::SessionMeta(SessionMetaRecord {
                    created_at: chrono::Utc::now(),
                    cwd: "/repo".into(),
                    workspace_key: None,
                    workspace_root: None,
                    workspace_name: None,
                    model: None,
                    config_summary: None,
                    parent_session_id: None,
                    branch_from_seq: None,
                    migrated_from: None,
                })
            }),
            RecordLine::new("corrupt-tail", 1, {
                RecordItem::Message(MessageRecord {
                    message: RecordedMessage::User {
                        uuid: uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                            .unwrap(),
                        timestamp: 1,
                        role: "user".into(),
                        content: RecordedMessageContent::Text("hello".into()),
                        is_meta: false,
                        tool_use_result: None,
                        source_tool_assistant_uuid: None,
                    },
                })
            }),
        ];

        let valid_json: Vec<String> = valid_lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap())
            .collect();
        // Write valid JSONL lines with a trailing partial line
        let content = format!("{}\n{{\n", valid_json.join("\n"));
        std::fs::write(&path, &content).unwrap();

        // Read with the default reader (non-strict mode) — should not panic
        let reader = ReplayReader::default();
        let result = reader.read_path(&path).unwrap();

        // Valid lines should have been recovered
        assert_eq!(result.lines.len(), 2);
        // The corrupt line should show as a warning
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message.contains("failed to parse"));

        // Reconstruct from the valid lines — should not panic
        let reconstructed = reconstruct_from_reader_result(&result);
        assert_eq!(reconstructed.messages.len(), 1);
    }

    /// Helper: reconstruct messages from a ReplayReadResult.
    fn reconstruct_from_reader_result(result: &ReplayReadResult) -> ReconstructedRecordSession {
        reconstruct_recorded_messages(&result.lines)
    }
}
