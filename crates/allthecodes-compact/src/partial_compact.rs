//! Partial conversation compaction primitives.
//!
//! `up_to(anchor)` summarizes messages before an anchor and preserves the
//! anchor plus recent context. `from(anchor)` preserves early context through
//! the anchor and summarizes messages after it.

use std::collections::HashSet;

use allthecodes_types::message::{ContentBlock, Message, MessageContent};
use allthecodes_utils::tokens;

use super::compaction::{create_compact_boundary_with_preserved_segment, create_preserved_segment};
use super::messages as compact_messages;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialCompactDirection {
    UpTo,
    From,
}

impl PartialCompactDirection {
    fn as_str(self) -> &'static str {
        match self {
            PartialCompactDirection::UpTo => "up_to",
            PartialCompactDirection::From => "from",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PartialCompactConfig {
    pub anchor_uuid: String,
    pub direction: PartialCompactDirection,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct PartialCompactResult {
    pub messages: Vec<Message>,
    pub tokens_freed: u64,
    pub compacted_start: usize,
    pub compacted_end: usize,
    pub boundary_index: usize,
}

pub fn partial_compact(
    messages: Vec<Message>,
    config: &PartialCompactConfig,
) -> Option<PartialCompactResult> {
    if !crate::gates::CompactionFeatureGates::from_env().partial_compact {
        return None;
    }

    if messages.len() <= 1 || config.summary.trim().is_empty() {
        return None;
    }

    let anchor_index = messages
        .iter()
        .position(|message| message.uuid().to_string() == config.anchor_uuid)?;

    match config.direction {
        PartialCompactDirection::UpTo => partial_compact_up_to(messages, config, anchor_index),
        PartialCompactDirection::From => partial_compact_from(messages, config, anchor_index),
    }
}

fn partial_compact_up_to(
    messages: Vec<Message>,
    config: &PartialCompactConfig,
    anchor_index: usize,
) -> Option<PartialCompactResult> {
    let keep_start = adjust_start_to_preserve_api_invariants(&messages, anchor_index);
    if keep_start == 0 {
        return None;
    }

    let pre_tokens = tokens::estimate_messages_tokens(&messages);
    let summary = create_partial_summary_message(config);
    let preserved = messages[keep_start..].to_vec();
    let post_tokens = tokens::estimate_messages_tokens(std::slice::from_ref(&summary))
        .saturating_add(tokens::estimate_messages_tokens(&preserved));
    let preserved_segment = create_preserved_segment(Some(&summary), &preserved);
    let boundary = create_compact_boundary_with_preserved_segment(
        pre_tokens,
        post_tokens,
        Some(preserved_segment),
    );

    let mut result_messages = Vec::with_capacity(preserved.len() + 2);
    result_messages.push(boundary);
    result_messages.push(summary);
    result_messages.extend(preserved);

    Some(PartialCompactResult {
        messages: result_messages,
        tokens_freed: pre_tokens.saturating_sub(post_tokens),
        compacted_start: 0,
        compacted_end: keep_start,
        boundary_index: 0,
    })
}

fn partial_compact_from(
    messages: Vec<Message>,
    config: &PartialCompactConfig,
    anchor_index: usize,
) -> Option<PartialCompactResult> {
    let keep_end = adjust_end_to_preserve_api_invariants(&messages, anchor_index + 1);
    if keep_end >= messages.len() {
        return None;
    }

    let pre_tokens = tokens::estimate_messages_tokens(&messages);
    let preserved = messages[..keep_end].to_vec();
    let summary = create_partial_summary_message(config);
    let post_tokens = tokens::estimate_messages_tokens(&preserved).saturating_add(
        tokens::estimate_messages_tokens(std::slice::from_ref(&summary)),
    );
    let preserved_segment = create_preserved_segment(Some(&summary), &preserved);
    let boundary = create_compact_boundary_with_preserved_segment(
        pre_tokens,
        post_tokens,
        Some(preserved_segment),
    );

    let boundary_index = preserved.len();
    let mut result_messages = Vec::with_capacity(preserved.len() + 2);
    result_messages.extend(preserved);
    result_messages.push(boundary);
    result_messages.push(summary);

    Some(PartialCompactResult {
        messages: result_messages,
        tokens_freed: pre_tokens.saturating_sub(post_tokens),
        compacted_start: keep_end,
        compacted_end: messages.len(),
        boundary_index,
    })
}

fn create_partial_summary_message(config: &PartialCompactConfig) -> Message {
    compact_messages::create_user_message(
        &format!(
            "<partial_compaction direction=\"{}\" anchor=\"{}\">\n\
             The selected conversation segment was compacted. Use this summary \
             as the canonical record of that segment:\n\n{}\n\
             </partial_compaction>",
            config.direction.as_str(),
            config.anchor_uuid,
            config.summary.trim()
        ),
        true,
    )
}

fn adjust_start_to_preserve_api_invariants(messages: &[Message], mut start: usize) -> usize {
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

fn adjust_end_to_preserve_api_invariants(messages: &[Message], mut end: usize) -> usize {
    loop {
        let retained_tool_results = tool_result_ids_in(&messages[..end]);
        let missing_tool_result = tool_use_ids_in(&messages[..end])
            .into_iter()
            .find(|tool_use_id| !retained_tool_results.contains(tool_use_id));

        let Some(tool_use_id) = missing_tool_result else {
            return end;
        };
        let Some(offset) = messages[end..]
            .iter()
            .position(|message| user_has_tool_result(message, &tool_use_id))
        else {
            return end;
        };
        end += offset + 1;
        if end >= messages.len() {
            return end;
        }
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
                    ContentBlock::ToolUse { id, .. } | ContentBlock::ServerToolUse { id, .. } => {
                        Some(id.clone())
                    }
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
            ContentBlock::ToolUse { id, .. } | ContentBlock::ServerToolUse { id, .. } => {
                id == expected_id
            }
            _ => false,
        }),
        _ => false,
    }
}

fn user_has_tool_result(message: &Message, expected_id: &str) -> bool {
    match message {
        Message::User(user) => match &user.content {
            MessageContent::Blocks(blocks) => blocks.iter().any(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => tool_use_id == expected_id,
                _ => false,
            }),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        AssistantMessage, CompactMetadata, SystemSubtype, ToolResultContent, UserMessage,
    };
    use uuid::Uuid;

    fn make_user(text: &str) -> Message {
        compact_messages::create_user_message(text, false)
    }

    fn make_long_user(text: &str) -> Message {
        compact_messages::create_user_message(&format!("{text} {}", "x ".repeat(500)), false)
    }

    fn make_assistant_tool(id: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "src/lib.rs"}),
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

    fn config(anchor: &Message, direction: PartialCompactDirection) -> PartialCompactConfig {
        PartialCompactConfig {
            anchor_uuid: anchor.uuid().to_string(),
            direction,
            summary: "Summary of compacted messages.".into(),
        }
    }

    fn compact_metadata(message: &Message) -> &CompactMetadata {
        let Message::System(system) = message else {
            panic!("expected system boundary");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected compact metadata");
        };
        metadata
    }

    #[test]
    fn partial_compact_up_to_preserves_anchor_and_recent_messages() {
        let anchor = make_user("anchor");
        let tail = make_user("tail");
        let messages = vec![
            make_long_user("old one"),
            make_long_user("old two"),
            anchor.clone(),
            tail.clone(),
        ];

        let result = partial_compact(messages, &config(&anchor, PartialCompactDirection::UpTo))
            .expect("partial compact should apply");

        assert_eq!(result.compacted_start, 0);
        assert_eq!(result.compacted_end, 2);
        assert_eq!(result.boundary_index, 0);
        assert_eq!(result.messages[2].uuid(), anchor.uuid());
        assert_eq!(result.messages[3].uuid(), tail.uuid());
        assert!(result.tokens_freed > 0);
    }

    #[test]
    fn partial_compact_up_to_keeps_tool_use_for_retained_tool_result() {
        let tool_use = make_assistant_tool("tu_1");
        let tool_result = make_tool_result("tu_1");
        let messages = vec![make_long_user("old"), tool_use.clone(), tool_result.clone()];

        let result = partial_compact(
            messages,
            &config(&tool_result, PartialCompactDirection::UpTo),
        )
        .expect("partial compact should apply");

        assert_eq!(result.compacted_end, 1);
        assert_eq!(result.messages[2].uuid(), tool_use.uuid());
        assert_eq!(result.messages[3].uuid(), tool_result.uuid());
    }

    #[test]
    fn partial_compact_from_extends_past_tool_result_for_preserved_tool_use() {
        let tool_use = make_assistant_tool("tu_1");
        let tool_result = make_tool_result("tu_1");
        let messages = vec![
            make_user("start"),
            tool_use.clone(),
            tool_result.clone(),
            make_long_user("later"),
        ];

        let result = partial_compact(messages, &config(&tool_use, PartialCompactDirection::From))
            .expect("partial compact should apply");

        assert_eq!(result.compacted_start, 3);
        assert_eq!(result.boundary_index, 3);
        assert_eq!(result.messages[1].uuid(), tool_use.uuid());
        assert_eq!(result.messages[2].uuid(), tool_result.uuid());
        assert!(result.tokens_freed > 0);
    }

    #[test]
    fn partial_compact_boundary_tracks_summary_and_preserved_messages() {
        let anchor = make_user("anchor");
        let messages = vec![make_long_user("old"), anchor.clone()];

        let result = partial_compact(messages, &config(&anchor, PartialCompactDirection::UpTo))
            .expect("partial compact should apply");
        let metadata = compact_metadata(&result.messages[result.boundary_index]);
        let segment = metadata
            .preserved_segment
            .as_ref()
            .expect("preserved segment");

        assert_eq!(
            segment.summary_message_uuid,
            Some(result.messages[1].uuid().to_string())
        );
        assert_eq!(
            segment.preserved_message_uuids,
            vec![anchor.uuid().to_string()]
        );
    }
}
