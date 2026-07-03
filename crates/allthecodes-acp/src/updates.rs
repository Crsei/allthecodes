//! SdkMessage -> ACP SessionUpdate mapping.
//!
//! Converts engine stream events into ACP v2 SessionUpdate notifications
//! that the ACP client can consume.

use agent_client_protocol_schema::v2::{
    AgentMessage, AgentThought, ContentBlock, ContentChunk, IdleStateUpdate, MessageId,
    RunningStateUpdate, SessionUpdate, StateUpdate, StopReason, TextContent, ToolCallStatus,
    ToolCallUpdate, UsageUpdate, UserMessage,
};
use allthecodes_types::message::{
    ContentBlock as InternalContentBlock, Message, MessageContent, SystemSubtype, ToolResultContent,
};
use allthecodes_types::sdk::{ResultSubtype, SdkMessage};

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

/// Map an SdkMessage to zero or more ACP SessionUpdate notifications.
pub fn sdk_message_to_updates(
    msg: &SdkMessage,
    counter: &mut MessageCounter,
) -> Vec<SessionUpdate> {
    match msg {
        SdkMessage::UserReplay(replay) => {
            vec![map_user_replay(replay, counter)]
        }
        SdkMessage::Assistant(assistant) => {
            vec![map_assistant_message(assistant, counter)]
        }
        SdkMessage::StreamEvent(event) => {
            vec![map_stream_event(event, counter)]
        }
        SdkMessage::Result(result) => sdk_result_to_updates(result, None),
        SdkMessage::SystemInit(_) => {
            vec![]
        }
        SdkMessage::CompactBoundary(_) => {
            vec![SessionUpdate::AgentThought(AgentThought::new(
                counter.next_thought_message_id(),
            ))]
        }
        SdkMessage::ApiRetry(_) => {
            vec![SessionUpdate::AgentThought(AgentThought::new(
                counter.next_thought_message_id(),
            ))]
        }
        SdkMessage::ToolUseSummary(_) => {
            vec![SessionUpdate::AgentThought(AgentThought::new(
                counter.next_thought_message_id(),
            ))]
        }
        SdkMessage::GoalUpdated(_) => {
            vec![SessionUpdate::AgentThought(AgentThought::new(
                counter.next_thought_message_id(),
            ))]
        }
        SdkMessage::Tombstone(_) => {
            vec![SessionUpdate::AgentThought(AgentThought::new(
                counter.next_thought_message_id(),
            ))]
        }
    }
}

/// Convert a loaded transcript message into deterministic ACP replay updates.
pub fn loaded_message_to_updates(
    msg: &Message,
    index: usize,
    cwd: &std::path::Path,
) -> Vec<SessionUpdate> {
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

fn map_user_replay(
    _replay: &allthecodes_types::sdk::SdkUserReplay,
    counter: &mut MessageCounter,
) -> SessionUpdate {
    let msg_id = counter.next_user_message_id();
    SessionUpdate::UserMessage(UserMessage::new(msg_id))
}

fn map_assistant_message(
    assistant: &allthecodes_types::sdk::SdkAssistantMessage,
    counter: &mut MessageCounter,
) -> SessionUpdate {
    let msg_id = counter.next_agent_message_id();
    let msg_text: String = assistant
        .message
        .content
        .iter()
        .filter_map(|block| {
            if let allthecodes_types::message::ContentBlock::Text { text } = block {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content_blocks: Vec<ContentBlock> = if !msg_text.is_empty() {
        vec![ContentBlock::Text(
            agent_client_protocol_schema::v2::TextContent::new(msg_text),
        )]
    } else {
        vec![]
    };

    SessionUpdate::AgentMessage(AgentMessage::new(msg_id).content(content_blocks))
}

fn map_stream_event(
    event: &allthecodes_types::sdk::SdkStreamEvent,
    counter: &mut MessageCounter,
) -> SessionUpdate {
    let stream_event = &event.event;
    match stream_event {
        allthecodes_types::message::StreamEvent::ContentBlockDelta { index: _, delta } => {
            // Extract text from the delta value
            let delta_text = delta
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            if !delta_text.is_empty() {
                let msg_id = counter.next_agent_message_id();
                let content = ContentBlock::Text(
                    agent_client_protocol_schema::v2::TextContent::new(delta_text),
                );
                SessionUpdate::AgentMessageChunk(ContentChunk::new(content, msg_id))
            } else {
                SessionUpdate::AgentThought(AgentThought::new(counter.next_thought_message_id()))
            }
        }
        allthecodes_types::message::StreamEvent::MessageStart { .. } => {
            SessionUpdate::StateUpdate(StateUpdate::Running(RunningStateUpdate::new()))
        }
        _ => SessionUpdate::AgentThought(AgentThought::new(counter.next_thought_message_id())),
    }
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
