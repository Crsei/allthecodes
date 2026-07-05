//! SDK output contract shared by engine, IPC, daemon, web, and TUI adapters.
//!
//! These are pure DTOs. Runtime accumulation and process-state updates stay in
//! the engine crate; consumers import this module directly instead of reaching
//! through `cc-engine`.

use serde::Serialize;
use uuid::Uuid;

use crate::brief::BriefMessagePayload;
use crate::message::{AssistantMessage, CompactMetadata, ContentBlock, StreamEvent, Usage};

/// Usage tracking accumulated across API calls in a session.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageTracking {
    /// Total input tokens consumed.
    pub total_input_tokens: u64,
    /// Total output tokens produced.
    pub total_output_tokens: u64,
    /// Total cache-read tokens.
    pub total_cache_read_tokens: u64,
    /// Total cache-creation tokens.
    pub total_cache_creation_tokens: u64,
    /// Total reasoning output tokens.
    pub total_reasoning_output_tokens: u64,
    /// Total cost in USD.
    pub total_cost_usd: f64,
    /// Number of API calls made.
    pub api_call_count: u64,
}

impl UsageTracking {
    /// Return a copy with one API call's usage accumulated.
    pub fn with_added_usage(mut self, usage: &Usage, cost_usd: f64) -> Self {
        self.total_input_tokens += usage.input_tokens;
        self.total_output_tokens += usage.output_tokens;
        self.total_cache_read_tokens += usage.cache_read_input_tokens;
        self.total_cache_creation_tokens += usage.cache_creation_input_tokens;
        self.total_reasoning_output_tokens += usage.reasoning_output_tokens;
        self.total_cost_usd += cost_usd;
        self.api_call_count += 1;
        self
    }
}

/// A record of a permission denial surfaced in the final SDK result.
#[derive(Debug, Clone, Serialize)]
pub struct PermissionDenial {
    pub tool_name: String,
    pub tool_use_id: String,
    pub reason: String,
    pub timestamp: i64,
}

/// SDK output message yielded by `QueryEngine::submit_message()`.
///
/// Corresponds to TypeScript `SDKMessage` (`agentSdkTypes.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SdkMessage {
    /// System initialisation (tool list, model info).
    SystemInit(SystemInitMessage),
    /// Assistant message (text, tool calls).
    Assistant(SdkAssistantMessage),
    /// User message replay (for SDK consumers to confirm receipt).
    UserReplay(SdkUserReplay),
    /// Streaming event (real-time text deltas for TUI display).
    StreamEvent(SdkStreamEvent),
    /// Compact boundary (produced after context compaction).
    CompactBoundary(SdkCompactBoundary),
    /// API retry notification.
    ApiRetry(SdkApiRetry),
    /// Tool-use summary.
    ToolUseSummary(SdkToolUseSummary),
    /// Durable session goal state changed.
    GoalUpdated(SdkGoalUpdated),
    /// Structured brief message intended for the user.
    BriefMessage(BriefMessagePayload),
    /// Tombstone for an assistant message abandoned by fallback retry.
    Tombstone(SdkTombstone),
    /// Final result. Every submit call ends with exactly one result.
    Result(SdkResult),
}

impl SdkMessage {
    pub fn event_name(&self) -> &'static str {
        match self {
            SdkMessage::SystemInit(_) => "system_init",
            SdkMessage::Assistant(_) => "assistant",
            SdkMessage::UserReplay(_) => "user_replay",
            SdkMessage::StreamEvent(_) => "stream_event",
            SdkMessage::CompactBoundary(_) => "compact_boundary",
            SdkMessage::ApiRetry(_) => "api_retry",
            SdkMessage::ToolUseSummary(_) => "tool_use_summary",
            SdkMessage::GoalUpdated(_) => "goal_updated",
            SdkMessage::BriefMessage(_) => "brief_message",
            SdkMessage::Tombstone(_) => "tombstone",
            SdkMessage::Result(_) => "result",
        }
    }
}

/// System initialisation payload.
#[derive(Debug, Clone, Serialize)]
pub struct SystemInitMessage {
    pub tools: Vec<String>,
    pub model: String,
    pub permission_mode: String,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Assistant message wrapper for SDK output.
#[derive(Debug, Clone, Serialize)]
pub struct SdkAssistantMessage {
    pub message: AssistantMessage,
    pub session_id: String,
    pub parent_tool_use_id: Option<String>,
}

/// User message replay echoed back to the SDK consumer.
#[derive(Debug, Clone, Serialize)]
pub struct SdkUserReplay {
    pub content: String,
    pub session_id: String,
    pub uuid: Uuid,
    pub timestamp: i64,
    pub is_replay: bool,
    pub is_synthetic: bool,
    /// Human-readable tool result preview used by TUI/transcript renderers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<String>,
    /// Assistant message that originated the tool call, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_tool_assistant_uuid: Option<Uuid>,
    /// Structured content blocks, present when replay carries block content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,
}

/// Streaming event wrapper for SDK output.
#[derive(Debug, Clone, Serialize)]
pub struct SdkStreamEvent {
    pub event: StreamEvent,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Compact boundary marker for SDK output.
#[derive(Debug, Clone, Serialize)]
pub struct SdkCompactBoundary {
    pub session_id: String,
    pub uuid: Uuid,
    pub compact_metadata: Option<CompactMetadata>,
    #[serde(skip_serializing_if = "is_false")]
    pub internal_metadata_hidden: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// API retry notification for SDK output.
#[derive(Debug, Clone, Serialize)]
pub struct SdkApiRetry {
    pub attempt: u32,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub error_status: Option<u16>,
    pub error: String,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Tool-use summary for SDK output.
#[derive(Debug, Clone, Serialize)]
pub struct SdkToolUseSummary {
    pub summary: String,
    pub preceding_tool_use_ids: Vec<String>,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Durable goal update for SDK/headless/TUI consumers.
#[derive(Debug, Clone, Serialize)]
pub struct SdkGoalUpdated {
    pub event: String,
    pub goal: serde_json::Value,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Tombstone for an orphaned assistant message.
#[derive(Debug, Clone, Serialize)]
pub struct SdkTombstone {
    pub message: AssistantMessage,
    pub session_id: String,
    pub uuid: Uuid,
}

/// Final result for a submit call.
#[derive(Debug, Clone, Serialize)]
pub struct SdkResult {
    pub subtype: ResultSubtype,
    pub is_error: bool,
    pub duration_ms: u64,
    pub duration_api_ms: u64,
    pub num_turns: usize,
    pub result: String,
    pub stop_reason: Option<String>,
    pub session_id: String,
    pub total_cost_usd: f64,
    pub usage: UsageTracking,
    pub permission_denials: Vec<PermissionDenial>,
    pub structured_output: Option<serde_json::Value>,
    pub uuid: Uuid,
    pub errors: Vec<String>,
}

/// Subtype of the final SDK result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSubtype {
    Success,
    ErrorDuringExecution,
    ErrorMaxTurns,
    ErrorMaxBudgetUsd,
    ErrorMaxStructuredOutputRetries,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_message_uses_stable_snake_case_type_tag() {
        let msg = SdkMessage::SystemInit(SystemInitMessage {
            tools: vec!["Read".into(), "Bash".into()],
            model: "claude-sonnet-4-20250514".into(),
            permission_mode: "default".into(),
            session_id: "session-1".into(),
            uuid: Uuid::nil(),
        });

        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["type"], "system_init");
        assert_eq!(value["tools"][0], "Read");
        assert_eq!(value["permission_mode"], "default");
        assert_eq!(msg.event_name(), "system_init");
    }

    #[test]
    fn sdk_brief_message_uses_stable_type_tag() {
        let msg = SdkMessage::BriefMessage(crate::brief::BriefMessagePayload {
            message: "short update".to_string(),
            status: crate::brief::BriefMessageStatus::Proactive,
            attachments: vec!["docs/brief.md".to_string()],
            level: None,
            source_tool_name: Some(crate::brief::BRIEF_TOOL_NAME.to_string()),
            tool_use_id: Some("toolu-brief".to_string()),
            session_id: Some("session-1".to_string()),
            timestamp: Some(100),
        });

        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(value["type"], "brief_message");
        assert_eq!(value["message"], "short update");
        assert_eq!(value["status"], "proactive");
        assert_eq!(value["attachments"], serde_json::json!(["docs/brief.md"]));
        assert_eq!(msg.event_name(), "brief_message");
    }

    #[test]
    fn sdk_result_preserves_usage_and_error_classification_fields() {
        let usage = UsageTracking::default().with_added_usage(
            &Usage {
                input_tokens: 10,
                output_tokens: 4,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 2,
                cache_creation_input_tokens: 1,
            },
            0.25,
        );
        let result = SdkMessage::Result(SdkResult {
            subtype: ResultSubtype::ErrorMaxBudgetUsd,
            is_error: true,
            duration_ms: 12,
            duration_api_ms: 8,
            num_turns: 2,
            result: "budget exceeded".into(),
            stop_reason: Some("max_budget".into()),
            session_id: "session-1".into(),
            total_cost_usd: usage.total_cost_usd,
            usage,
            permission_denials: vec![PermissionDenial {
                tool_name: "Bash".into(),
                tool_use_id: "tool-1".into(),
                reason: "denied".into(),
                timestamp: 100,
            }],
            structured_output: None,
            uuid: Uuid::nil(),
            errors: vec!["max budget".into()],
        });

        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["type"], "result");
        assert_eq!(value["subtype"], "error_max_budget_usd");
        assert_eq!(value["usage"]["total_input_tokens"], 10);
        assert_eq!(value["usage"]["api_call_count"], 1);
        assert_eq!(value["permission_denials"][0]["tool_use_id"], "tool-1");
    }
}
