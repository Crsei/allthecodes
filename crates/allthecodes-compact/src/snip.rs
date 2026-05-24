use chrono::Utc;
use uuid::Uuid;

use allthecodes_types::message::{
    CompactMetadata, ContentBlock, Message, SystemMessage, SystemSubtype,
};

use crate::compaction::create_preserved_segment;

/// Result of history snipping.
#[derive(Debug)]
pub struct SnipResult {
    /// The resulting messages after snipping.
    pub messages: Vec<Message>,
    /// Estimated tokens freed by snipping.
    pub tokens_freed: u64,
    /// A compact boundary message inserted at the snip point, if snipping occurred.
    pub boundary_message: Option<Message>,
}

/// Snip old conversation history if it exceeds the threshold.
///
/// Keeps the first message (system context) and the most recent `max_turns` turns.
/// A "turn" = one user message + one assistant response + any associated tool results
/// between them.
///
/// If the number of turns does not exceed `max_turns`, the messages are returned
/// unchanged.
pub fn snip_compact_if_needed(messages: Vec<Message>, max_turns: usize) -> SnipResult {
    if messages.is_empty() || max_turns == 0 {
        return SnipResult {
            messages,
            tokens_freed: 0,
            boundary_message: None,
        };
    }

    // Identify turn boundaries. A turn starts at each user message that is
    // not a tool result continuation (i.e., not carrying tool_use_result).
    let turn_starts = identify_turn_starts(&messages);

    if turn_starts.len() <= max_turns {
        // No need to snip
        return SnipResult {
            messages,
            tokens_freed: 0,
            boundary_message: None,
        };
    }

    // We want to keep the most recent `max_turns` turns.
    // The cut point is the start index of the (turns.len() - max_turns)th turn.
    let keep_from_turn = turn_starts.len() - max_turns;
    let cut_index = turn_starts[keep_from_turn];

    // Also keep the very first message if it is a system context or user setup.
    // We always preserve messages[0] as it typically contains initial context.
    let preserve_first = if cut_index > 0 { 1 } else { 0 };

    // Estimate tokens in the removed section
    let removed_messages = &messages[preserve_first..cut_index];
    let tokens_freed = estimate_tokens_for_messages(removed_messages);

    let mut preserved_messages = Vec::new();
    if preserve_first > 0 {
        preserved_messages.push(messages[0].clone());
    }
    preserved_messages.extend(messages[cut_index..].iter().cloned());

    // Build the boundary message
    let mut boundary = Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        subtype: SystemSubtype::CompactBoundary {
            compact_metadata: Some(CompactMetadata {
                pre_compact_token_count: tokens_freed,
                post_compact_token_count: 0,
                preserved_segment: None,
            }),
        },
        content: format!(
            "[History snipped: removed {} messages ({} estimated tokens) to stay within turn limit]",
            removed_messages.len(),
            tokens_freed
        ),
    });
    let preserved_segment = create_preserved_segment(Some(&boundary), &preserved_messages);
    if let Message::System(system) = &mut boundary {
        if let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &mut system.subtype
        {
            metadata.preserved_segment = Some(preserved_segment);
        }
    }

    // Construct result: first message + boundary + recent turns
    let mut result = Vec::new();
    if preserve_first > 0 {
        result.push(messages[0].clone());
    }
    result.push(boundary.clone());
    for msg in &messages[cut_index..] {
        result.push(msg.clone());
    }

    SnipResult {
        messages: result,
        tokens_freed,
        boundary_message: Some(boundary),
    }
}

/// Identify the starting indices of each "turn" in the message list.
/// A turn starts at each User message that is a fresh user input
/// (not a tool-result-only message).
fn identify_turn_starts(messages: &[Message]) -> Vec<usize> {
    let mut starts = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        if let Message::User(user) = msg {
            // A turn starts at a user message that is NOT solely a tool result.
            // If it has tool_use_result set, it's a continuation of the previous
            // assistant's tool call, not a new turn.
            if user.tool_use_result.is_none() {
                starts.push(i);
            }
        }
    }

    starts
}

/// Rough token estimate for a slice of messages.
/// Uses ~4 characters per token heuristic.
fn estimate_tokens_for_messages(messages: &[Message]) -> u64 {
    let total_chars: u64 = messages
        .iter()
        .map(|m| estimate_message_chars(m) as u64)
        .sum();
    total_chars / 4
}

/// Estimate the number of characters in a message.
fn estimate_message_chars(msg: &Message) -> usize {
    match msg {
        Message::User(u) => match &u.content {
            allthecodes_types::message::MessageContent::Text(t) => t.len(),
            allthecodes_types::message::MessageContent::Blocks(blocks) => {
                blocks.iter().map(content_block_chars).sum()
            }
        },
        Message::Assistant(a) => a.content.iter().map(content_block_chars).sum(),
        Message::System(s) => s.content.len(),
        Message::Progress(p) => p.data.to_string().len(),
        Message::Attachment(_) => 100, // rough estimate
    }
}

/// Estimate character count for a content block.
fn content_block_chars(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text } => text.len(),
        ContentBlock::ToolUse { input, .. } | ContentBlock::ServerToolUse { input, .. } => {
            input.to_string().len() + 50
        }
        ContentBlock::ToolResult { content, .. } => match content {
            allthecodes_types::message::ToolResultContent::Text(t) => t.len(),
            allthecodes_types::message::ToolResultContent::Blocks(bs) => {
                bs.iter().map(content_block_chars).sum()
            }
        },
        ContentBlock::Thinking { thinking, .. } => thinking.len(),
        ContentBlock::RedactedThinking { data } => data.len(),
        ContentBlock::ConnectorText { connector_text, .. } => connector_text.len(),
        ContentBlock::Image { source } => source.data.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::create_user_message;
    use allthecodes_types::message::AssistantMessage;

    fn make_assistant_text(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: None,
            stop_reason: Some("end_turn".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[test]
    fn test_no_snip_when_under_limit() {
        let messages = vec![
            create_user_message("hello", false),
            make_assistant_text("hi"),
            create_user_message("how are you", false),
            make_assistant_text("fine"),
        ];
        let result = snip_compact_if_needed(messages.clone(), 5);
        assert_eq!(result.messages.len(), messages.len());
        assert_eq!(result.tokens_freed, 0);
        assert!(result.boundary_message.is_none());
    }

    #[test]
    fn test_snip_when_over_limit() {
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(create_user_message(&format!("turn {}", i), false));
            messages.push(make_assistant_text(&format!("response {}", i)));
        }
        let first_uuid = messages[0].uuid().to_string();
        let removed_uuid = messages[2].uuid().to_string();

        let result = snip_compact_if_needed(messages, 3);
        // Should keep first message + boundary + last 3 turns (6 messages)
        assert!(result.tokens_freed > 0);
        assert!(result.boundary_message.is_some());
        // First message preserved, boundary inserted, then 3 turns * 2 messages
        assert_eq!(result.messages.len(), 1 + 1 + 6);

        let boundary = result.boundary_message.as_ref().unwrap();
        let Message::System(system) = boundary else {
            panic!("expected snip boundary system message");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected compact metadata");
        };
        let segment = metadata.preserved_segment.as_ref().unwrap();
        let boundary_uuid = boundary.uuid().to_string();
        assert_eq!(
            segment.summary_message_uuid.as_deref(),
            Some(boundary_uuid.as_str())
        );
        assert!(segment.preserved_message_uuids.contains(&first_uuid));
        assert!(!segment.preserved_message_uuids.contains(&removed_uuid));
    }
}
