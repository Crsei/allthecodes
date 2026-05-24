//! Session Memory based compaction.
//!
//! This is the no-API path used when durable session insights already exist.
//! It replaces old conversation history with a compact session-memory summary
//! and preserves a bounded recent window for continuity.

use std::collections::HashSet;

use allthecodes_types::message::{ContentBlock, Message, MessageContent};
use allthecodes_utils::tokens;

use super::compaction::{create_compact_boundary_with_preserved_segment, create_preserved_segment};
use super::messages as compact_messages;

#[derive(Debug, Clone)]
pub struct SessionMemoryCompactConfig {
    pub min_tokens: u64,
    pub min_text_block_messages: usize,
    pub max_tokens: u64,
}

impl Default for SessionMemoryCompactConfig {
    fn default() -> Self {
        Self {
            min_tokens: 10_000,
            min_text_block_messages: 5,
            max_tokens: 40_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionMemoryCompactResult {
    pub messages: Vec<Message>,
    pub tokens_freed: u64,
    pub kept_start_index: usize,
}

pub fn session_memory_compact_if_needed(
    messages: Vec<Message>,
    session_memory_context: &str,
) -> Option<SessionMemoryCompactResult> {
    session_memory_compact_with_config(
        messages,
        session_memory_context,
        &SessionMemoryCompactConfig::default(),
    )
}

pub fn session_memory_compact_with_config(
    messages: Vec<Message>,
    session_memory_context: &str,
    config: &SessionMemoryCompactConfig,
) -> Option<SessionMemoryCompactResult> {
    if !crate::gates::CompactionFeatureGates::from_env().session_memory_compact {
        return None;
    }

    let session_memory_context = session_memory_context.trim();
    if session_memory_context.is_empty() || messages.len() <= 1 {
        return None;
    }

    let pre_tokens = tokens::estimate_messages_tokens(&messages);
    let mut keep_start = calculate_messages_to_keep_index(&messages, config);
    keep_start = adjust_index_to_preserve_api_invariants(&messages, keep_start);
    if keep_start == 0 {
        return None;
    }

    let mut compacted_without_boundary = Vec::with_capacity(messages.len() - keep_start + 1);
    compacted_without_boundary.push(compact_messages::create_user_message(
        &format!(
            "<session_memory_compaction>\n\
             The earlier conversation was compacted using persisted session insights. \
             Use these insights as the summary of prior work, then continue from the \
             preserved recent messages.\n\n\
             {}\n\
             </session_memory_compaction>",
            session_memory_context
        ),
        true,
    ));
    compacted_without_boundary.extend(messages[keep_start..].iter().cloned());

    let post_tokens = tokens::estimate_messages_tokens(&compacted_without_boundary);
    if post_tokens >= pre_tokens {
        return None;
    }

    let preserved_segment = create_preserved_segment(
        compacted_without_boundary.first(),
        compacted_without_boundary.get(1..).unwrap_or(&[]),
    );
    let boundary = create_compact_boundary_with_preserved_segment(
        pre_tokens,
        post_tokens,
        Some(preserved_segment),
    );

    let mut compacted = Vec::with_capacity(compacted_without_boundary.len() + 1);
    compacted.push(boundary);
    compacted.append(&mut compacted_without_boundary);

    Some(SessionMemoryCompactResult {
        messages: compacted,
        tokens_freed: pre_tokens.saturating_sub(post_tokens),
        kept_start_index: keep_start,
    })
}

fn calculate_messages_to_keep_index(
    messages: &[Message],
    config: &SessionMemoryCompactConfig,
) -> usize {
    if messages.is_empty() {
        return 0;
    }

    let mut start = messages.len();
    let mut total_tokens = 0_u64;
    let mut text_messages = 0_usize;

    for index in (0..messages.len()).rev() {
        start = index;
        total_tokens = total_tokens.saturating_add(tokens::estimate_messages_tokens(
            std::slice::from_ref(&messages[index]),
        ));
        if has_text_blocks(&messages[index]) {
            text_messages += 1;
        }

        if total_tokens >= config.max_tokens {
            break;
        }
        if total_tokens >= config.min_tokens && text_messages >= config.min_text_block_messages {
            break;
        }
    }

    start
}

fn adjust_index_to_preserve_api_invariants(messages: &[Message], mut start: usize) -> usize {
    loop {
        let retained_tool_uses = tool_use_ids_in(&messages[start..]);
        let missing_tool_use = tool_result_ids_in(&messages[start..])
            .into_iter()
            .find(|tool_use_id| !retained_tool_uses.contains(tool_use_id));

        let Some(tool_use_id) = missing_tool_use else {
            return start;
        };

        let Some(index) = messages[..start]
            .iter()
            .rposition(|message| assistant_has_tool_use(message, &tool_use_id))
        else {
            return start;
        };

        if index == start {
            return start;
        }
        start = index;
    }
}

fn has_text_blocks(message: &Message) -> bool {
    match message {
        Message::User(user) => match &user.content {
            MessageContent::Text(text) => !text.trim().is_empty(),
            MessageContent::Blocks(blocks) => blocks.iter().any(
                |block| matches!(block, ContentBlock::Text { text } if !text.trim().is_empty()),
            ),
        },
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text } if !text.trim().is_empty())),
        _ => false,
    }
}

fn tool_use_ids_in(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .flat_map(|message| match message {
            Message::Assistant(assistant) => assistant
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

fn tool_result_ids_in(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .flat_map(|message| match message {
            Message::User(user) => match &user.content {
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        })
        .collect()
}

fn assistant_has_tool_use(message: &Message, expected_id: &str) -> bool {
    match message {
        Message::Assistant(assistant) => assistant.content.iter().any(|block| match block {
            ContentBlock::ToolUse { id, .. } => id == expected_id,
            _ => false,
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        AssistantMessage, MessageContent, SystemSubtype, ToolResultContent, UserMessage,
    };
    use uuid::Uuid;

    fn make_user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn make_assistant(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn make_tool_use(id: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "src/main.rs"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn make_tool_result(id: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: id.into(),
                content: ToolResultContent::Text("result".into()),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: Some("result".into()),
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn session_memory_compact_uses_summary_and_keeps_recent_window() {
        let mut messages = Vec::new();
        for index in 0..12 {
            messages.push(make_user(&format!(
                "old question {index} {}",
                "x".repeat(120)
            )));
            messages.push(make_assistant(&format!(
                "old answer {index} {}",
                "y".repeat(120)
            )));
        }
        messages.push(make_user("recent request"));
        messages.push(make_assistant("recent answer"));

        let config = SessionMemoryCompactConfig {
            min_tokens: 1,
            min_text_block_messages: 2,
            max_tokens: 80,
        };
        let result = session_memory_compact_with_config(
            messages,
            "<session-insights>Important prior context.</session-insights>",
            &config,
        )
        .unwrap();

        let rendered = format!("{:?}", result.messages);
        assert!(rendered.contains("session_memory_compaction"));
        assert!(rendered.contains("Important prior context"));
        assert!(rendered.contains("recent request"));
        assert!(!rendered.contains("old question 0"));
        assert!(result.tokens_freed > 0);

        let Message::System(system) = &result.messages[0] else {
            panic!("expected compact boundary");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected compact metadata");
        };
        let segment = metadata.preserved_segment.as_ref().unwrap();
        assert_eq!(
            segment.summary_message_uuid,
            Some(result.messages[1].uuid().to_string())
        );
        assert_eq!(
            segment.preserved_message_uuids,
            result.messages[2..]
                .iter()
                .map(|message| message.uuid().to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn session_memory_compact_returns_none_without_context() {
        let result = session_memory_compact_if_needed(vec![make_user("hello")], " ");
        assert!(result.is_none());
    }

    #[test]
    fn adjust_index_keeps_tool_use_for_retained_tool_result() {
        let messages = vec![
            make_tool_use("toolu_1"),
            make_tool_result("toolu_1"),
            make_user("continue"),
        ];

        assert_eq!(adjust_index_to_preserve_api_invariants(&messages, 1), 0);
    }
}
