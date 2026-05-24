use allthecodes_types::message::{
    ContentBlock, Message, MessageContent, MicrocompactMetadata, SystemMessage, SystemSubtype,
    ToolResultContent,
};
use allthecodes_utils::tokens;
use uuid::Uuid;

/// Result of microcompaction.
#[derive(Debug)]
pub struct MicrocompactResult {
    pub messages: Vec<Message>,
    pub tokens_freed: u64,
    pub compacted_tool_ids: Vec<String>,
}

/// The number of most-recent tool results to always preserve intact.
const KEEP_RECENT_TOOL_RESULTS: usize = 10;

/// Tool results larger than this threshold (in characters) are candidates
/// for replacement with a summary.
const SIZE_THRESHOLD_CHARS: usize = 1000;

/// Microcompact messages by removing old, large tool results
/// that are unlikely to be needed.
///
/// Rules:
/// - Keep the last N tool results (N = KEEP_RECENT_TOOL_RESULTS)
/// - For older results, if size > SIZE_THRESHOLD_CHARS, replace with summary
/// - Never remove tool results from the most recent assistant turn
pub fn microcompact_messages(messages: Vec<Message>) -> MicrocompactResult {
    if messages.is_empty() {
        return MicrocompactResult {
            messages,
            tokens_freed: 0,
            compacted_tool_ids: Vec::new(),
        };
    }
    let pre_tokens = tokens::estimate_messages_tokens(&messages);

    // First, identify the index of the last assistant message so we can
    // protect its associated tool results.
    let last_assistant_idx = messages
        .iter()
        .rposition(|m| matches!(m, Message::Assistant(_)));

    // Collect indices of all tool-result-carrying user messages.
    let tool_result_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| message_has_tool_result(m))
        .map(|(i, _)| i)
        .collect();

    // The set of indices that are "recent" and should be preserved.
    let recent_start = if tool_result_indices.len() > KEEP_RECENT_TOOL_RESULTS {
        tool_result_indices.len() - KEEP_RECENT_TOOL_RESULTS
    } else {
        0
    };
    let recent_indices: std::collections::HashSet<usize> = tool_result_indices[recent_start..]
        .iter()
        .copied()
        .collect();

    // Determine which tool result indices belong to the last assistant turn.
    // The last assistant turn's tool results are all user messages that come
    // after the last assistant message.
    let last_turn_indices: std::collections::HashSet<usize> = match last_assistant_idx {
        Some(ai) => messages
            .iter()
            .enumerate()
            .filter(|(i, m)| *i > ai && message_has_tool_result(m))
            .map(|(i, _)| i)
            .collect(),
        None => std::collections::HashSet::new(),
    };

    let mut tokens_freed: u64 = 0;
    let mut compacted_tool_ids = Vec::new();
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());

    for (i, msg) in messages.into_iter().enumerate() {
        // If this message has tool results and is NOT recent and NOT in the
        // last assistant turn, consider compacting it.
        if message_has_tool_result(&msg)
            && !recent_indices.contains(&i)
            && !last_turn_indices.contains(&i)
        {
            let (compacted, freed, tool_ids) = compact_tool_result_message(msg);
            tokens_freed += freed;
            extend_unique(&mut compacted_tool_ids, tool_ids);
            result.push(compacted);
        } else {
            result.push(msg);
        }
    }

    if tokens_freed > 0 {
        result.push(create_microcompact_boundary(
            pre_tokens,
            tokens_freed,
            compacted_tool_ids.clone(),
        ));
    }

    MicrocompactResult {
        messages: result,
        tokens_freed,
        compacted_tool_ids,
    }
}

/// Check if a message contains at least one ToolResult content block.
fn message_has_tool_result(msg: &Message) -> bool {
    match msg {
        Message::User(user) => match &user.content {
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. })),
            MessageContent::Text(_) => false,
        },
        _ => false,
    }
}

/// Compact a single tool-result-carrying user message.
/// For each ToolResult block whose content exceeds SIZE_THRESHOLD_CHARS,
/// replace the content with a truncated summary.
/// Returns the modified message and the number of estimated tokens freed.
fn compact_tool_result_message(msg: Message) -> (Message, u64, Vec<String>) {
    let Message::User(mut user) = msg else {
        return (msg, 0, Vec::new());
    };

    let mut freed: u64 = 0;
    let mut compacted_tool_ids = Vec::new();

    match &mut user.content {
        MessageContent::Blocks(blocks) => {
            for block in blocks.iter_mut() {
                if let ContentBlock::ToolResult {
                    tool_use_id,
                    ref mut content,
                    ..
                } = block
                {
                    let original_len = tool_result_content_len(content);
                    if original_len > SIZE_THRESHOLD_CHARS {
                        let summary = make_tool_result_summary(content, original_len);
                        let new_len = summary.len();
                        *content = ToolResultContent::Text(summary);
                        // Rough token estimate: ~4 chars per token
                        let chars_saved = original_len.saturating_sub(new_len);
                        freed += (chars_saved as u64) / 4;
                        compacted_tool_ids.push(tool_use_id.clone());
                    }
                }
            }
        }
        MessageContent::Text(_) => {}
    }

    (Message::User(user), freed, compacted_tool_ids)
}

fn create_microcompact_boundary(
    pre_tokens: u64,
    tokens_saved: u64,
    compacted_tool_ids: Vec<String>,
) -> Message {
    let post_tokens = pre_tokens.saturating_sub(tokens_saved);
    Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        subtype: SystemSubtype::MicrocompactBoundary {
            microcompact_metadata: Some(MicrocompactMetadata {
                trigger: "auto".to_string(),
                pre_tokens,
                tokens_saved,
                compacted_tool_ids,
                cleared_attachment_uuids: Vec::new(),
            }),
        },
        content: format!(
            "Context microcompacted: {} -> {} tokens ({} tokens saved)",
            pre_tokens, post_tokens, tokens_saved
        ),
    })
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

/// Get the character length of a ToolResultContent.
fn tool_result_content_len(content: &ToolResultContent) -> usize {
    match content {
        ToolResultContent::Text(s) => s.len(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => text.len(),
                _ => 0,
            })
            .sum(),
    }
}

/// Create a summary string for a tool result, preserving the first and last
/// portions of the content.
fn make_tool_result_summary(content: &ToolResultContent, original_len: usize) -> String {
    let full_text = match content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };

    let preview_len = 200.min(full_text.len());
    let tail_len = 100.min(full_text.len().saturating_sub(preview_len));

    let head = &full_text[..preview_len];
    let tail = if tail_len > 0 {
        &full_text[full_text.len() - tail_len..]
    } else {
        ""
    };

    format!(
        "{}\n\n[... {} characters omitted (microcompacted) ...]\n\n{}",
        head,
        original_len - preview_len - tail_len,
        tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::create_tool_result_message;
    use allthecodes_types::message::AssistantMessage;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_assistant() -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: serde_json::json!({}),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[test]
    fn test_microcompact_preserves_recent() {
        // Create messages with a few small tool results - they should not be compacted
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(make_assistant());
            messages.push(create_tool_result_message(
                &format!("tu_{}", i),
                "short result",
                false,
            ));
        }

        let result = microcompact_messages(messages);
        assert_eq!(result.tokens_freed, 0);
        assert!(result.compacted_tool_ids.is_empty());
        assert_eq!(result.messages.len(), 10);
    }

    #[test]
    fn test_microcompact_compacts_old_large_results() {
        let mut messages = Vec::new();

        // Create an old, large tool result
        let large_content = "x".repeat(2000);
        messages.push(make_assistant());
        messages.push(create_tool_result_message("tu_old", &large_content, false));

        // Then add KEEP_RECENT_TOOL_RESULTS + 1 more recent ones
        for i in 0..KEEP_RECENT_TOOL_RESULTS + 1 {
            messages.push(make_assistant());
            messages.push(create_tool_result_message(
                &format!("tu_recent_{}", i),
                "small result",
                false,
            ));
        }

        let result = microcompact_messages(messages);
        assert!(result.tokens_freed > 0);
        assert_eq!(result.compacted_tool_ids, vec!["tu_old"]);
        assert!(matches!(
            result.messages.last(),
            Some(Message::System(SystemMessage {
                subtype: SystemSubtype::MicrocompactBoundary { .. },
                ..
            }))
        ));
    }

    #[test]
    fn test_microcompact_boundary_records_metadata() {
        let large_content = "x".repeat(2000);
        let mut messages = vec![
            make_assistant(),
            create_tool_result_message("tu_old", &large_content, false),
        ];
        for i in 0..KEEP_RECENT_TOOL_RESULTS + 1 {
            messages.push(make_assistant());
            messages.push(create_tool_result_message(
                &format!("tu_recent_{}", i),
                "small result",
                false,
            ));
        }

        let result = microcompact_messages(messages);
        let Some(Message::System(system)) = result.messages.last() else {
            panic!("expected microcompact boundary");
        };
        let SystemSubtype::MicrocompactBoundary {
            microcompact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected microcompact metadata");
        };

        assert_eq!(metadata.trigger, "auto");
        assert_eq!(metadata.tokens_saved, result.tokens_freed);
        assert_eq!(metadata.compacted_tool_ids, vec!["tu_old"]);
        assert!(metadata.cleared_attachment_uuids.is_empty());
    }
}
