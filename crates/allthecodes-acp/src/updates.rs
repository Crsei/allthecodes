//! SdkMessage -> ACP SessionUpdate mapping.
//!
//! Converts engine stream events into ACP v2 SessionUpdate notifications
//! that the ACP client can consume.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agent_client_protocol_schema::v2::{
    AgentMessage, AgentThought, ContentBlock, ContentChunk, IdleStateUpdate, MessageId, PlanEntry,
    PlanEntryPriority, PlanEntryStatus, PlanUpdate, PlanUpdateContent, RunningStateUpdate,
    SessionId, SessionUpdate, StateUpdate, StopReason, TextContent, ToolCallStatus, ToolCallUpdate,
    UsageUpdate, UserMessage,
};
use allthecodes_types::brief::BriefMessagePayload;
use allthecodes_types::message::{
    ContentBlock as InternalContentBlock, Message, MessageContent, StreamEvent, SystemSubtype,
    ToolResultContent,
};
use allthecodes_types::sdk::{
    ResultSubtype, SdkApiRetry, SdkCompactBoundary, SdkGoalUpdated, SdkMessage, SdkResult,
    SdkStreamEvent, SdkTombstone, SdkToolUseSummary, SdkUserReplay,
};

use crate::tool_calls::ToolCallContextCache;

/// Message counter for generating deterministic message IDs.
#[derive(Debug, Default, Clone)]
pub struct MessageCounter {
    next_user_msg: u64,
    next_agent_msg: u64,
    next_thought_msg: u64,
}

impl MessageCounter {
    pub fn next_user_message_id(&mut self) -> MessageId {
        self.next_user_msg += 1;
        MessageId::new(format!("user-msg-{}", self.next_user_msg))
    }

    pub fn next_agent_message_id(&mut self) -> MessageId {
        self.next_agent_msg += 1;
        MessageId::new(format!("agent-msg-{}", self.next_agent_msg))
    }

    pub fn next_thought_message_id(&mut self) -> MessageId {
        self.next_thought_msg += 1;
        MessageId::new(format!("thought-{}", self.next_thought_msg))
    }
}

#[derive(Debug, Default, Clone)]
struct MessageIdState {
    counter: MessageCounter,
    agent_by_block: HashMap<usize, MessageId>,
    thought_by_block: HashMap<usize, MessageId>,
}

impl MessageIdState {
    fn next_user_message_id(&mut self) -> MessageId {
        self.counter.next_user_message_id()
    }

    fn next_agent_message_id(&mut self) -> MessageId {
        self.counter.next_agent_message_id()
    }

    fn next_thought_message_id(&mut self) -> MessageId {
        self.counter.next_thought_message_id()
    }

    fn agent_message_id_for_block(&mut self, index: usize) -> MessageId {
        self.agent_by_block
            .entry(index)
            .or_insert_with(|| self.counter.next_agent_message_id())
            .clone()
    }

    fn thought_message_id_for_block(&mut self, index: usize) -> MessageId {
        self.thought_by_block
            .entry(index)
            .or_insert_with(|| self.counter.next_thought_message_id())
            .clone()
    }
}

/// Stateful per-turn SDK-to-ACP mapper.
#[derive(Debug, Clone)]
pub struct AcpUpdateMapper {
    session_id: SessionId,
    message_ids: MessageIdState,
    tool_cache: ToolCallContextCache,
}

impl AcpUpdateMapper {
    pub fn new(session_id: SessionId, cwd: impl Into<PathBuf>) -> Self {
        Self {
            session_id,
            message_ids: MessageIdState::default(),
            tool_cache: ToolCallContextCache::new(cwd),
        }
    }

    pub fn map_message(&mut self, msg: &SdkMessage) -> Vec<SessionUpdate> {
        match msg {
            SdkMessage::UserReplay(replay) => self.map_user_replay(replay),
            SdkMessage::Assistant(assistant) => {
                self.map_assistant_blocks(&assistant.message.content)
            }
            SdkMessage::BriefMessage(brief) => vec![self.brief_message_update(brief)],
            SdkMessage::StreamEvent(event) => self.map_stream_event(event),
            SdkMessage::Result(result) => self.map_result(result, None),
            SdkMessage::SystemInit(_) => Vec::new(),
            SdkMessage::CompactBoundary(boundary) => vec![self.compact_boundary_update(boundary)],
            SdkMessage::ApiRetry(retry) => vec![self.api_retry_update(retry)],
            SdkMessage::ToolUseSummary(summary) => vec![self.tool_use_summary_update(summary)],
            SdkMessage::GoalUpdated(goal) => vec![self.goal_update(goal)],
            SdkMessage::Tombstone(tombstone) => vec![self.tombstone_update(tombstone)],
        }
    }

    pub fn map_result(
        &mut self,
        result: &SdkResult,
        forced_stop_reason: Option<StopReason>,
    ) -> Vec<SessionUpdate> {
        sdk_result_to_updates(result, forced_stop_reason)
    }

    pub fn map_tool_progress(&mut self, tool_use_id: &str, text: &str) -> Option<SessionUpdate> {
        self.tool_cache
            .content_chunk(tool_use_id, text)
            .map(SessionUpdate::ToolCallContentChunk)
    }

    fn map_user_replay(&mut self, replay: &SdkUserReplay) -> Vec<SessionUpdate> {
        let mut updates = Vec::new();
        if let Some(blocks) = &replay.content_blocks {
            for block in blocks {
                match block {
                    InternalContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let text = tool_result_text(content);
                        updates.push(SessionUpdate::ToolCallUpdate(
                            self.tool_cache
                                .finish_tool_call(tool_use_id, &text, *is_error),
                        ));
                    }
                    _ => {
                        if let Some(content) = internal_content_to_acp_text(block) {
                            updates.push(SessionUpdate::UserMessage(
                                UserMessage::new(self.message_ids.next_user_message_id())
                                    .content(vec![content]),
                            ));
                        }
                    }
                }
            }
        }

        if updates.is_empty() && !replay.content.is_empty() {
            updates.push(SessionUpdate::UserMessage(
                UserMessage::new(self.message_ids.next_user_message_id()).content(vec![
                    ContentBlock::Text(TextContent::new(replay.content.clone())),
                ]),
            ));
        }
        updates
    }

    fn map_assistant_blocks(&mut self, blocks: &[InternalContentBlock]) -> Vec<SessionUpdate> {
        let mut updates = Vec::new();
        let mut agent_content = Vec::new();
        let mut thought_content = Vec::new();

        for block in blocks {
            match block {
                InternalContentBlock::Text { text } if !text.is_empty() => {
                    agent_content.push(ContentBlock::Text(TextContent::new(text.clone())));
                }
                InternalContentBlock::Text { .. } => {}
                InternalContentBlock::ConnectorText { connector_text, .. }
                    if !connector_text.is_empty() =>
                {
                    agent_content
                        .push(ContentBlock::Text(TextContent::new(connector_text.clone())));
                }
                InternalContentBlock::ConnectorText { .. } => {}
                InternalContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                    thought_content.push(ContentBlock::Text(TextContent::new(thinking.clone())));
                }
                InternalContentBlock::Thinking { .. } => {}
                InternalContentBlock::RedactedThinking { data } if !data.is_empty() => {
                    thought_content.push(ContentBlock::Text(TextContent::new(data.clone())));
                }
                InternalContentBlock::RedactedThinking { .. } => {}
                InternalContentBlock::ToolUse { id, name, input }
                | InternalContentBlock::ServerToolUse { id, name, input } => {
                    updates.push(SessionUpdate::ToolCallUpdate(
                        self.tool_cache.start_tool_call(id, name, input),
                    ));
                }
                InternalContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let text = tool_result_text(content);
                    updates.push(SessionUpdate::ToolCallUpdate(
                        self.tool_cache
                            .finish_tool_call(tool_use_id, &text, *is_error),
                    ));
                }
                InternalContentBlock::Image { .. } => {}
            }
        }

        if !agent_content.is_empty() {
            updates.insert(
                0,
                SessionUpdate::AgentMessage(
                    AgentMessage::new(self.message_ids.next_agent_message_id())
                        .content(agent_content),
                ),
            );
        }
        if !thought_content.is_empty() {
            updates.push(SessionUpdate::AgentThought(
                AgentThought::new(self.message_ids.next_thought_message_id())
                    .content(thought_content),
            ));
        }

        updates
    }

    fn map_stream_event(&mut self, event: &SdkStreamEvent) -> Vec<SessionUpdate> {
        match &event.event {
            StreamEvent::MessageStart { .. } => vec![state_running_update()],
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => self.map_content_block_start(*index, content_block),
            StreamEvent::ContentBlockDelta { index, delta } => {
                self.map_content_block_delta(*index, delta)
            }
            StreamEvent::ContentBlockStop { .. } | StreamEvent::MessageStop => Vec::new(),
            StreamEvent::MessageDelta { .. } => Vec::new(),
        }
    }

    fn map_content_block_start(
        &mut self,
        index: usize,
        content_block: &InternalContentBlock,
    ) -> Vec<SessionUpdate> {
        match content_block {
            InternalContentBlock::Text { text } if !text.is_empty() => {
                vec![self.agent_message_chunk(index, text)]
            }
            InternalContentBlock::Text { .. } | InternalContentBlock::ConnectorText { .. } => {
                self.message_ids.agent_message_id_for_block(index);
                Vec::new()
            }
            InternalContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                vec![self.thought_message_chunk(index, thinking)]
            }
            InternalContentBlock::Thinking { .. }
            | InternalContentBlock::RedactedThinking { .. } => {
                self.message_ids.thought_message_id_for_block(index);
                Vec::new()
            }
            InternalContentBlock::ToolUse { id, name, input }
            | InternalContentBlock::ServerToolUse { id, name, input } => {
                vec![SessionUpdate::ToolCallUpdate(
                    self.tool_cache.start_tool_call(id, name, input),
                )]
            }
            InternalContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let text = tool_result_text(content);
                vec![SessionUpdate::ToolCallUpdate(
                    self.tool_cache
                        .finish_tool_call(tool_use_id, &text, *is_error),
                )]
            }
            InternalContentBlock::Image { .. } => Vec::new(),
        }
    }

    fn map_content_block_delta(
        &mut self,
        index: usize,
        delta: &serde_json::Value,
    ) -> Vec<SessionUpdate> {
        if let Some(text) = delta.get("text").and_then(|value| value.as_str()) {
            if !text.is_empty() {
                return vec![self.agent_message_chunk(index, text)];
            }
        }
        if let Some(text) = delta.get("thinking").and_then(|value| value.as_str()) {
            if !text.is_empty() {
                return vec![self.thought_message_chunk(index, text)];
            }
        }
        if let Some(text) = delta.get("data").and_then(|value| value.as_str()) {
            if !text.is_empty() {
                return vec![self.thought_message_chunk(index, text)];
            }
        }
        Vec::new()
    }

    fn agent_message_chunk(&mut self, index: usize, text: &str) -> SessionUpdate {
        let msg_id = self.message_ids.agent_message_id_for_block(index);
        SessionUpdate::AgentMessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(text.to_string())),
            msg_id,
        ))
    }

    fn thought_message_chunk(&mut self, index: usize, text: &str) -> SessionUpdate {
        let msg_id = self.message_ids.thought_message_id_for_block(index);
        SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(text.to_string())),
            msg_id,
        ))
    }

    fn compact_boundary_update(&mut self, boundary: &SdkCompactBoundary) -> SessionUpdate {
        let text = boundary
            .compact_metadata
            .as_ref()
            .map(|metadata| {
                format!(
                    "Context compacted from {} to {} tokens",
                    metadata.pre_compact_token_count, metadata.post_compact_token_count
                )
            })
            .unwrap_or_else(|| "Context compacted".to_string());
        self.kinded_thought("compact_boundary", text, None)
    }

    fn api_retry_update(&mut self, retry: &SdkApiRetry) -> SessionUpdate {
        self.kinded_thought(
            "api_retry",
            format!(
                "Retrying API request {}/{} after {}ms: {}",
                retry.attempt, retry.max_retries, retry.retry_delay_ms, retry.error
            ),
            None,
        )
    }

    fn tool_use_summary_update(&mut self, summary: &SdkToolUseSummary) -> SessionUpdate {
        let mut meta = serde_json::Map::new();
        meta.insert("kind".into(), serde_json::json!("tool_use_summary"));
        meta.insert(
            "precedingToolUseIds".into(),
            serde_json::json!(summary.preceding_tool_use_ids),
        );
        SessionUpdate::AgentThought(
            AgentThought::new(self.message_ids.next_thought_message_id())
                .content(vec![ContentBlock::Text(TextContent::new(
                    summary.summary.clone(),
                ))])
                .meta(meta),
        )
    }

    fn goal_update(&mut self, goal: &SdkGoalUpdated) -> SessionUpdate {
        if let Some(entries) = goal_entries(&goal.goal) {
            return SessionUpdate::PlanUpdate(PlanUpdate::new(PlanUpdateContent::items(
                format!("goal-{}", self.session_id),
                entries,
            )));
        }
        self.kinded_thought("goal_updated", goal.goal.to_string(), None)
    }

    fn tombstone_update(&mut self, tombstone: &SdkTombstone) -> SessionUpdate {
        self.kinded_thought(
            "tombstone",
            "Assistant message abandoned during retry".to_string(),
            Some(tombstone.message.uuid.to_string()),
        )
    }

    fn brief_message_update(&mut self, brief: &BriefMessagePayload) -> SessionUpdate {
        SessionUpdate::AgentMessage(
            AgentMessage::new(self.message_ids.next_agent_message_id()).content(vec![
                ContentBlock::Text(TextContent::new(brief.message.clone())),
            ]),
        )
    }

    fn kinded_thought(
        &mut self,
        kind: &str,
        text: String,
        abandoned_message_id: Option<String>,
    ) -> SessionUpdate {
        let mut meta = serde_json::Map::new();
        meta.insert("kind".into(), serde_json::Value::String(kind.to_string()));
        if let Some(abandoned_message_id) = abandoned_message_id {
            meta.insert(
                "abandonedMessageId".into(),
                serde_json::Value::String(abandoned_message_id),
            );
        }
        SessionUpdate::AgentThought(
            AgentThought::new(self.message_ids.next_thought_message_id())
                .content(vec![ContentBlock::Text(TextContent::new(text))])
                .meta(meta),
        )
    }
}

/// Map an SdkMessage to zero or more ACP SessionUpdate notifications.
pub fn sdk_message_to_updates(
    msg: &SdkMessage,
    counter: &mut MessageCounter,
) -> Vec<SessionUpdate> {
    let mut mapper = AcpUpdateMapper {
        session_id: SessionId::new("compat"),
        message_ids: MessageIdState {
            counter: counter.clone(),
            agent_by_block: HashMap::new(),
            thought_by_block: HashMap::new(),
        },
        tool_cache: ToolCallContextCache::new(std::env::current_dir().unwrap_or_default()),
    };
    let updates = mapper.map_message(msg);
    *counter = mapper.message_ids.counter;
    updates
}

/// Convert a loaded transcript message into deterministic ACP replay updates.
pub fn loaded_message_to_updates(msg: &Message, index: usize, cwd: &Path) -> Vec<SessionUpdate> {
    match msg {
        Message::User(user) if !user.is_meta => {
            let content = message_content_to_acp_blocks(&user.content);
            if content.is_empty() {
                Vec::new()
            } else {
                vec![SessionUpdate::UserMessage(
                    UserMessage::new(MessageId::new(format!("loaded-user-{index}")))
                        .content(content),
                )]
            }
        }
        Message::User(_) => Vec::new(),
        Message::Assistant(assistant) => {
            let mut updates = Vec::new();
            let mut agent_content = Vec::new();
            let mut thought_content = Vec::new();

            for block in &assistant.content {
                match block {
                    InternalContentBlock::Text { text } => {
                        agent_content.push(ContentBlock::Text(TextContent::new(text.clone())));
                    }
                    InternalContentBlock::Thinking { thinking, .. } => {
                        thought_content
                            .push(ContentBlock::Text(TextContent::new(thinking.clone())));
                    }
                    InternalContentBlock::RedactedThinking { data } => {
                        thought_content.push(ContentBlock::Text(TextContent::new(data.clone())));
                    }
                    InternalContentBlock::ToolUse { id, name, input }
                    | InternalContentBlock::ServerToolUse { id, name, input } => {
                        updates.push(SessionUpdate::ToolCallUpdate(
                            crate::tool_calls::build_tool_call_update(
                                format!("loaded-tool-{id}"),
                                name,
                                input,
                                cwd,
                            ),
                        ));
                    }
                    InternalContentBlock::ToolResult {
                        tool_use_id,
                        content: _,
                        is_error,
                    } => {
                        let status = if *is_error {
                            ToolCallStatus::Failed
                        } else {
                            ToolCallStatus::Completed
                        };
                        updates.push(SessionUpdate::ToolCallUpdate(
                            ToolCallUpdate::new(format!("loaded-tool-{tool_use_id}"))
                                .status(status),
                        ));
                    }
                    InternalContentBlock::ConnectorText { connector_text, .. } => {
                        agent_content
                            .push(ContentBlock::Text(TextContent::new(connector_text.clone())));
                    }
                    InternalContentBlock::Image { .. } => {}
                }
            }

            if !agent_content.is_empty() {
                updates.insert(
                    0,
                    SessionUpdate::AgentMessage(
                        AgentMessage::new(MessageId::new(format!("loaded-agent-{index}")))
                            .content(agent_content),
                    ),
                );
            }
            if !thought_content.is_empty() {
                updates.push(SessionUpdate::AgentThought(
                    AgentThought::new(MessageId::new(format!("loaded-thought-{index}")))
                        .content(thought_content),
                ));
            }

            updates
        }
        Message::System(system) => {
            let kind = match system.subtype {
                SystemSubtype::CompactBoundary { .. }
                | SystemSubtype::MicrocompactBoundary { .. } => Some("compact_boundary"),
                SystemSubtype::ApiError { .. } => Some("api_retry"),
                SystemSubtype::LocalCommand { .. }
                | SystemSubtype::Informational { .. }
                | SystemSubtype::Warning => Some("system"),
            };
            let Some(kind) = kind else {
                return Vec::new();
            };
            let mut meta = serde_json::Map::new();
            meta.insert("kind".into(), serde_json::Value::String(kind.into()));
            vec![SessionUpdate::AgentThought(
                AgentThought::new(MessageId::new(format!("loaded-thought-{index}")))
                    .content(vec![ContentBlock::Text(TextContent::new(
                        system.content.clone(),
                    ))])
                    .meta(meta),
            )]
        }
        Message::Progress(progress) => {
            let text = progress
                .data
                .get("text")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    progress
                        .data
                        .get("message")
                        .and_then(|value| value.as_str())
                })
                .unwrap_or_default();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![SessionUpdate::ToolCallUpdate(
                    ToolCallUpdate::new(format!("loaded-tool-{}", progress.tool_use_id))
                        .status(ToolCallStatus::InProgress),
                )]
            }
        }
        Message::Attachment(_) => Vec::new(),
    }
}

fn message_content_to_acp_blocks(content: &MessageContent) -> Vec<ContentBlock> {
    match content {
        MessageContent::Text(text) if !text.is_empty() => {
            vec![ContentBlock::Text(TextContent::new(text.clone()))]
        }
        MessageContent::Text(_) => Vec::new(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(internal_content_to_acp_text)
            .collect(),
    }
}

fn internal_content_to_acp_text(block: &InternalContentBlock) -> Option<ContentBlock> {
    match block {
        InternalContentBlock::Text { text } if !text.is_empty() => {
            Some(ContentBlock::Text(TextContent::new(text.clone())))
        }
        InternalContentBlock::ToolResult { content, .. } => {
            let text = tool_result_text(content);
            (!text.is_empty()).then(|| ContentBlock::Text(TextContent::new(text)))
        }
        _ => None,
    }
}

fn tool_result_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                InternalContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn goal_entries(goal: &serde_json::Value) -> Option<Vec<PlanEntry>> {
    let entries_value = goal
        .get("entries")
        .or_else(|| goal.get("items"))
        .or_else(|| goal.get("todos"))?;
    let entries = entries_value.as_array()?;
    let mapped: Vec<PlanEntry> = entries
        .iter()
        .filter_map(|entry| {
            let content = entry
                .get("content")
                .or_else(|| entry.get("text"))
                .or_else(|| entry.get("title"))
                .and_then(|value| value.as_str())?;
            let priority = match entry
                .get("priority")
                .and_then(|value| value.as_str())
                .unwrap_or("medium")
            {
                "high" => PlanEntryPriority::High,
                "low" => PlanEntryPriority::Low,
                other if other != "medium" => PlanEntryPriority::Other(other.to_string()),
                _ => PlanEntryPriority::Medium,
            };
            let status = match entry
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("pending")
            {
                "in_progress" | "in-progress" | "active" => PlanEntryStatus::InProgress,
                "completed" | "complete" | "done" => PlanEntryStatus::Completed,
                other if other != "pending" => PlanEntryStatus::Other(other.to_string()),
                _ => PlanEntryStatus::Pending,
            };
            Some(PlanEntry::new(content.to_string(), priority, status))
        })
        .collect();
    (!mapped.is_empty()).then_some(mapped)
}

pub fn sdk_result_to_updates(
    result: &allthecodes_types::sdk::SdkResult,
    forced_stop_reason: Option<StopReason>,
) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();

    let usage = UsageUpdate::new(
        result.usage.total_input_tokens,
        result.usage.total_output_tokens,
    );
    updates.push(SessionUpdate::UsageUpdate(usage));

    let stop_reason = forced_stop_reason.or_else(|| match result.subtype {
        ResultSubtype::ErrorMaxTurns => Some(StopReason::MaxTurnRequests),
        ResultSubtype::ErrorDuringExecution => Some(StopReason::Refusal),
        _ if result.stop_reason.as_deref() == Some("max_tokens") => Some(StopReason::MaxTokens),
        _ => Some(StopReason::EndTurn),
    });

    updates.push(SessionUpdate::StateUpdate(StateUpdate::Idle(
        IdleStateUpdate::new().stop_reason(stop_reason),
    )));

    updates
}

/// Build a "running" state update.
pub fn state_running_update() -> SessionUpdate {
    SessionUpdate::StateUpdate(StateUpdate::Running(RunningStateUpdate::new()))
}

/// Build an "idle" state update.
pub fn state_idle_update(stop_reason: Option<StopReason>) -> SessionUpdate {
    let mut idle = IdleStateUpdate::new();
    if let Some(reason) = stop_reason {
        idle = idle.stop_reason(reason);
    }
    SessionUpdate::StateUpdate(StateUpdate::Idle(idle))
}
