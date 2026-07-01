use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use allthecodes_types::message::{
    ApiErrorInfo, AssistantMessage, Attachment, AttachmentMessage, CompactMetadata, ContentBlock,
    InfoLevel, Message, MessageContent, MicrocompactMetadata, ProgressMessage, SystemMessage,
    SystemSubtype, Usage, UserMessage,
};

pub const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordLine {
    pub schema_version: u32,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub item: RecordItem,
}

impl RecordLine {
    pub fn new(session_id: impl Into<String>, seq: u64, item: RecordItem) -> Self {
        Self {
            schema_version: RECORD_SCHEMA_VERSION,
            seq,
            timestamp: Utc::now(),
            session_id: session_id.into(),
            turn_id: None,
            item,
        }
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordItem {
    SessionMeta(SessionMetaRecord),
    SessionState(SessionStateRecord),
    TurnStarted(TurnStartedRecord),
    TurnFinished(TurnFinishedRecord),
    Message(MessageRecord),
    QueryEvent(QueryEventRecord),
    ToolProgress(ToolProgressRecord),
    PermissionRequest(PermissionRequestRecord),
    PermissionResponse(PermissionResponseRecord),
    QuestionRequest(QuestionRequestRecord),
    QuestionResponse(QuestionResponseRecord),
    CompactionBoundary(CompactionBoundaryRecord),
    Snapshot(SessionSnapshotRecord),
    Rollback(RollbackRecord),
    Branch(BranchRecord),
    LegacyMessage(LegacyMessageRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetaRecord {
    pub created_at: DateTime<Utc>,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated_from: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStateRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_summary: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurnStartedRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnFinishedRecord {
    pub status: TurnFinishStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnFinishStatus {
    Completed,
    Aborted,
    Errored,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub message: RecordedMessage,
}

impl MessageRecord {
    pub fn from_message(message: &Message) -> Self {
        Self {
            message: RecordedMessage::from_message(message),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryEventRecord {
    RequestStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    RawStream {
        event: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressRecord {
    pub tool_use_id: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestRecord {
    pub request_id: String,
    pub tool_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponseRecord {
    pub request_id: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRequestRecord {
    pub request_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionResponseRecord {
    pub request_id: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionBoundaryRecord {
    pub kind: CompactionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_message_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionKind {
    Compact,
    Microcompact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotRecord {
    pub message_count: usize,
    pub messages: Vec<RecordedMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackRecord {
    pub target_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRecord {
    pub new_session_id: String,
    pub parent_session_id: String,
    pub branch_from_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyMessageRecord {
    pub msg_type: String,
    pub uuid: String,
    pub timestamp: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordedMessage {
    User {
        uuid: Uuid,
        timestamp: i64,
        role: String,
        content: RecordedMessageContent,
        is_meta: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_result: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_tool_assistant_uuid: Option<Uuid>,
    },
    Assistant {
        uuid: Uuid,
        timestamp: i64,
        role: String,
        content: Vec<ContentBlock>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        is_api_error_message: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_error: Option<String>,
        cost_usd: f64,
    },
    System {
        uuid: Uuid,
        timestamp: i64,
        subtype: RecordedSystemSubtype,
        content: String,
    },
    Progress {
        uuid: Uuid,
        timestamp: i64,
        tool_use_id: String,
        data: serde_json::Value,
    },
    Attachment {
        uuid: Uuid,
        timestamp: i64,
        attachment: Attachment,
    },
}

impl RecordedMessage {
    pub fn from_message(message: &Message) -> Self {
        match message {
            Message::User(UserMessage {
                uuid,
                timestamp,
                role,
                content,
                is_meta,
                tool_use_result,
                source_tool_assistant_uuid,
            }) => Self::User {
                uuid: *uuid,
                timestamp: *timestamp,
                role: role.clone(),
                content: RecordedMessageContent::from(content),
                is_meta: *is_meta,
                tool_use_result: tool_use_result.clone(),
                source_tool_assistant_uuid: *source_tool_assistant_uuid,
            },
            Message::Assistant(AssistantMessage {
                uuid,
                timestamp,
                role,
                content,
                usage,
                stop_reason,
                is_api_error_message,
                api_error,
                cost_usd,
            }) => Self::Assistant {
                uuid: *uuid,
                timestamp: *timestamp,
                role: role.clone(),
                content: content.clone(),
                usage: usage.clone(),
                stop_reason: stop_reason.clone(),
                is_api_error_message: *is_api_error_message,
                api_error: api_error.clone(),
                cost_usd: *cost_usd,
            },
            Message::System(SystemMessage {
                uuid,
                timestamp,
                subtype,
                content,
            }) => Self::System {
                uuid: *uuid,
                timestamp: *timestamp,
                subtype: RecordedSystemSubtype::from(subtype),
                content: content.clone(),
            },
            Message::Progress(ProgressMessage {
                uuid,
                timestamp,
                tool_use_id,
                data,
            }) => Self::Progress {
                uuid: *uuid,
                timestamp: *timestamp,
                tool_use_id: tool_use_id.clone(),
                data: data.clone(),
            },
            Message::Attachment(AttachmentMessage {
                uuid,
                timestamp,
                attachment,
            }) => Self::Attachment {
                uuid: *uuid,
                timestamp: *timestamp,
                attachment: attachment.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RecordedMessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl From<&MessageContent> for RecordedMessageContent {
    fn from(content: &MessageContent) -> Self {
        match content {
            MessageContent::Text(text) => Self::Text(text.clone()),
            MessageContent::Blocks(blocks) => Self::Blocks(blocks.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordedSystemSubtype {
    CompactBoundary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compact_metadata: Option<CompactMetadata>,
    },
    MicrocompactBoundary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        microcompact_metadata: Option<MicrocompactMetadata>,
    },
    ApiError {
        retry_attempt: u32,
        max_retries: u32,
        retry_in_ms: u64,
        error: RecordedApiErrorInfo,
    },
    Informational {
        level: RecordedInfoLevel,
    },
    LocalCommand {
        content: String,
    },
    Warning,
}

impl From<&SystemSubtype> for RecordedSystemSubtype {
    fn from(subtype: &SystemSubtype) -> Self {
        match subtype {
            SystemSubtype::CompactBoundary { compact_metadata } => Self::CompactBoundary {
                compact_metadata: compact_metadata.clone(),
            },
            SystemSubtype::MicrocompactBoundary {
                microcompact_metadata,
            } => Self::MicrocompactBoundary {
                microcompact_metadata: microcompact_metadata.clone(),
            },
            SystemSubtype::ApiError {
                retry_attempt,
                max_retries,
                retry_in_ms,
                error,
            } => Self::ApiError {
                retry_attempt: *retry_attempt,
                max_retries: *max_retries,
                retry_in_ms: *retry_in_ms,
                error: RecordedApiErrorInfo::from(error),
            },
            SystemSubtype::Informational { level } => Self::Informational {
                level: RecordedInfoLevel::from(level),
            },
            SystemSubtype::LocalCommand { content } => Self::LocalCommand {
                content: content.clone(),
            },
            SystemSubtype::Warning => Self::Warning,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedApiErrorInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub message: String,
}

impl From<&ApiErrorInfo> for RecordedApiErrorInfo {
    fn from(error: &ApiErrorInfo) -> Self {
        Self {
            status: error.status,
            message: error.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedInfoLevel {
    Info,
    Warning,
    Error,
}

impl From<&InfoLevel> for RecordedInfoLevel {
    fn from(level: &InfoLevel) -> Self {
        match level {
            InfoLevel::Info => Self::Info,
            InfoLevel::Warning => Self::Warning,
            InfoLevel::Error => Self::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::ToolResultContent;

    #[test]
    fn record_line_roundtrips_tool_use_message() {
        let message = Message::Assistant(AssistantMessage {
            uuid: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            timestamp: 1_700_000_000,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: "toolu_1".into(),
                name: "Read".into(),
                input: serde_json::json!({ "file_path": "README.md" }),
            }],
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 20,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.01,
        });
        let line = RecordLine::new(
            "session-1",
            7,
            RecordItem::Message(MessageRecord::from_message(&message)),
        );

        let json = serde_json::to_string(&line).unwrap();
        let parsed: RecordLine = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, RECORD_SCHEMA_VERSION);
        assert_eq!(parsed.seq, 7);
        match parsed.item {
            RecordItem::Message(MessageRecord {
                message: RecordedMessage::Assistant { content, .. },
            }) => assert!(matches!(
                &content[0],
                ContentBlock::ToolUse { name, .. } if name == "Read"
            )),
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[test]
    fn recorded_message_covers_tool_result_blocks() {
        let message = Message::User(UserMessage {
            uuid: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            timestamp: 1,
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".into(),
                content: ToolResultContent::Text("ok".into()),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        let recorded = RecordedMessage::from_message(&message);
        let json = serde_json::to_value(recorded).unwrap();
        assert_eq!(json["type"], "user");
        assert_eq!(json["content"]["type"], "blocks");
    }
}
