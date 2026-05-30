use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 内容块 — 对应 Anthropic API 的 ContentBlock
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },

    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        signature: Option<String>,
    },

    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },

    #[serde(rename = "connector_text")]
    ConnectorText {
        connector_text: String,
        signature: Option<String>,
    },

    #[serde(rename = "image")]
    Image { source: ImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String, // "image/png" etc.
    pub data: String,
}

/// API 使用量
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

/// 用户消息
#[derive(Debug, Clone)]
pub struct UserMessage {
    pub uuid: Uuid,
    pub timestamp: i64,
    pub role: String, // always "user"
    pub content: MessageContent,
    /// 元消息 (系统注入的, 用户不可见)
    pub is_meta: bool,
    /// 工具结果内容 (当此消息携带 tool_result 时)
    pub tool_use_result: Option<String>,
    /// 源工具调用的 assistant 消息 UUID
    pub source_tool_assistant_uuid: Option<Uuid>,
}

/// 助手消息
#[derive(Debug, Clone, Serialize)]
pub struct AssistantMessage {
    pub uuid: Uuid,
    pub timestamp: i64,
    pub role: String, // always "assistant"
    pub content: Vec<ContentBlock>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
    /// 是否为 API 错误的合成消息
    pub is_api_error_message: bool,
    /// API 错误类型 (rate_limit, invalid_request, max_output_tokens, etc.)
    pub api_error: Option<String>,
    /// 此消息的 API 调用成本 (USD)
    pub cost_usd: f64,
}

/// 系统消息子类型
#[derive(Debug, Clone)]
pub enum SystemSubtype {
    /// 上下文压缩边界
    CompactBoundary {
        compact_metadata: Option<CompactMetadata>,
    },
    /// Microcompact 边界：旧工具结果被就地精简，原消息结构仍保留
    MicrocompactBoundary {
        microcompact_metadata: Option<MicrocompactMetadata>,
    },
    /// API 错误 (重试中)
    ApiError {
        retry_attempt: u32,
        max_retries: u32,
        retry_in_ms: u64,
        error: ApiErrorInfo,
    },
    /// 信息性消息 (提示, 警告)
    Informational { level: InfoLevel },
    /// 本地命令输出
    LocalCommand { content: String },
    /// 警告
    Warning,
}

#[derive(Debug, Clone)]
pub enum InfoLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactMetadata {
    pub pre_compact_token_count: u64,
    pub post_compact_token_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_segment: Option<PreservedSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_compact_discovered_tools: Option<Vec<String>>,
}

impl CompactMetadata {
    pub fn has_internal_metadata(&self) -> bool {
        self.preserved_segment.is_some() || self.pre_compact_discovered_tools.is_some()
    }

    pub fn public_copy(&self) -> Self {
        Self {
            pre_compact_token_count: self.pre_compact_token_count,
            post_compact_token_count: self.post_compact_token_count,
            preserved_segment: None,
            pre_compact_discovered_tools: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MicrocompactMetadata {
    pub trigger: String,
    pub pre_tokens: u64,
    pub tokens_saved: u64,
    pub compacted_tool_ids: Vec<String>,
    pub cleared_attachment_uuids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreservedSegment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_message_uuid: Option<String>,
    pub preserved_message_uuids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ApiErrorInfo {
    pub status: Option<u16>,
    pub message: String,
}

/// 系统消息
#[derive(Debug, Clone)]
pub struct SystemMessage {
    pub uuid: Uuid,
    pub timestamp: i64,
    pub subtype: SystemSubtype,
    pub content: String,
}

/// 进度消息 (工具执行进度 / hook 进度)
#[derive(Debug, Clone)]
pub struct ProgressMessage {
    pub uuid: Uuid,
    pub timestamp: i64,
    pub tool_use_id: String,
    pub data: serde_json::Value, // 各工具自定义进度数据
}

/// 附件消息 (文件变更通知, 排队命令, 结构化输出等)
#[derive(Debug, Clone)]
pub struct AttachmentMessage {
    pub uuid: Uuid,
    pub timestamp: i64,
    pub attachment: Attachment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Attachment {
    #[serde(rename = "edited_text_file")]
    EditedTextFile { path: String },

    #[serde(rename = "queued_command")]
    QueuedCommand {
        prompt: String,
        source_uuid: Option<String>,
    },

    #[serde(rename = "max_turns_reached")]
    MaxTurnsReached { max_turns: usize, turn_count: usize },

    #[serde(rename = "structured_output")]
    StructuredOutput { data: serde_json::Value },

    #[serde(rename = "hook_stopped_continuation")]
    HookStoppedContinuation,

    #[serde(rename = "nested_memory")]
    NestedMemory { path: String, content: String },

    #[serde(rename = "skill_discovery")]
    SkillDiscovery { skills: Vec<String> },
}

/// 消息内容 (字符串或内容块数组)
#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// 墓碑消息 (标记已作废的消息, 用于降级重试)
#[derive(Debug, Clone)]
pub struct TombstoneMessage {
    pub message: AssistantMessage,
}

/// 工具使用摘要消息
#[derive(Debug, Clone)]
pub struct ToolUseSummaryMessage {
    pub uuid: Uuid,
    pub summary: String,
    pub preceding_tool_use_ids: Vec<String>,
}

/// 主消息类型 — 所有消息的联合枚举
///
/// 对应 TypeScript: `type Message = UserMessage | AssistantMessage | SystemMessage | ...`
#[derive(Debug, Clone)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    Progress(ProgressMessage),
    Attachment(AttachmentMessage),
}

/// 流事件 (API 流式传输的中间事件)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        usage: Usage,
    },
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: serde_json::Value,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
        usage: Option<Usage>,
    },
    MessageStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
}

/// 请求开始事件 (标记新的 API 请求)
#[derive(Debug, Clone)]
pub struct RequestStartEvent;

/// query() 产出的所有事件类型
#[derive(Debug, Clone)]
pub enum QueryYield {
    Stream(StreamEvent),
    RequestStart(RequestStartEvent),
    Message(Message),
    Tombstone(TombstoneMessage),
    ToolUseSummary(ToolUseSummaryMessage),
}

impl Message {
    pub fn uuid(&self) -> Uuid {
        match self {
            Message::User(m) => m.uuid,
            Message::Assistant(m) => m.uuid,
            Message::System(m) => m.uuid,
            Message::Progress(m) => m.uuid,
            Message::Attachment(m) => m.uuid,
        }
    }

    pub fn timestamp(&self) -> i64 {
        match self {
            Message::User(m) => m.timestamp,
            Message::Assistant(m) => m.timestamp,
            Message::System(m) => m.timestamp,
            Message::Progress(m) => m.timestamp,
            Message::Attachment(m) => m.timestamp,
        }
    }
}
