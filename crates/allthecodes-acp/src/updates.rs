//! SdkMessage -> ACP SessionUpdate mapping.
//!
//! Converts engine stream events into ACP v2 SessionUpdate notifications
//! that the ACP client can consume.

use agent_client_protocol_schema::v2::{
    AgentMessage, AgentThought, ContentChunk, ContentBlock, IdleStateUpdate,
    MessageId, RunningStateUpdate, SessionUpdate, StateUpdate, StopReason,
    UsageUpdate, UserMessage,
};
use allthecodes_types::sdk::{
    SdkMessage, ResultSubtype,
};

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
        SdkMessage::Result(result) => {
            sdk_result_to_updates(result, None)
        }
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
        _ => {
            SessionUpdate::AgentThought(AgentThought::new(counter.next_thought_message_id()))
        }
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
