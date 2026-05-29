use chrono::Utc;
use uuid::Uuid;

use allthecodes_types::message::{
    CompactMetadata, ContentBlock, Message, MessageContent, SystemMessage, SystemSubtype,
    ToolResultContent,
};
use allthecodes_utils::tokens;

use super::auto_compact;
use crate::compaction::create_preserved_segment;

#[derive(Debug)]
pub struct ContextCollapseResult {
    pub messages: Vec<Message>,
    pub tokens_freed: u64,
    pub collapsed_messages: usize,
    pub boundary_message: Option<Message>,
}

const MAX_TURNS_BEFORE_COLLAPSE: usize = 40;
const KEEP_RECENT_TURNS: usize = 20;
const CONTEXT_WINDOW_TRIGGER_RATIO: f64 = 0.6;
const SUMMARY_PREVIEW_LIMIT_CHARS: usize = 2_000;

pub fn context_collapse_if_needed(messages: Vec<Message>, model: &str) -> ContextCollapseResult {
    if messages.is_empty() {
        return unchanged(messages);
    }

    let turn_starts = identify_turn_starts(&messages);
    let initial_tokens = tokens::estimate_messages_tokens(&messages);
    let trigger_tokens =
        (auto_compact::get_context_window_size(model) as f64 * CONTEXT_WINDOW_TRIGGER_RATIO) as u64;

    if turn_starts.len() <= MAX_TURNS_BEFORE_COLLAPSE && initial_tokens < trigger_tokens {
        return unchanged(messages);
    }
    if turn_starts.len() <= KEEP_RECENT_TURNS {
        return unchanged(messages);
    }

    let keep_from_turn = turn_starts.len() - KEEP_RECENT_TURNS;
    let cut_index = turn_starts[keep_from_turn];
    if cut_index == 0 {
        return unchanged(messages);
    }

    let preserve_first = should_preserve_first_message(messages.first());
    let collapsed_start = usize::from(preserve_first);
    if collapsed_start >= cut_index {
        return unchanged(messages);
    }

    let collapsed = &messages[collapsed_start..cut_index];
    let summary = summarize_collapsed_messages(collapsed);

    let mut result = Vec::new();
    if preserve_first {
        result.push(messages[0].clone());
    }

    let mut preserved_messages = Vec::new();
    if preserve_first {
        preserved_messages.push(messages[0].clone());
    }
    preserved_messages.extend(messages[cut_index..].iter().cloned());

    let mut boundary = Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        subtype: SystemSubtype::CompactBoundary {
            compact_metadata: Some(CompactMetadata {
                pre_compact_token_count: initial_tokens,
                post_compact_token_count: 0,
                preserved_segment: None,
                pre_compact_discovered_tools: None,
            }),
        },
        content: format!("<context_collapse>\n{}\n</context_collapse>", summary),
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
    result.push(boundary.clone());
    result.extend(messages[cut_index..].iter().cloned());

    let final_tokens = tokens::estimate_messages_tokens(&result);
    let tokens_freed = initial_tokens.saturating_sub(final_tokens);

    let boundary = match boundary {
        Message::System(mut system) => {
            if let SystemSubtype::CompactBoundary {
                compact_metadata: Some(metadata),
            } = &mut system.subtype
            {
                metadata.post_compact_token_count = final_tokens;
            }
            Message::System(system)
        }
        other => other,
    };
    if preserve_first {
        result[1] = boundary.clone();
    } else {
        result[0] = boundary.clone();
    }

    ContextCollapseResult {
        messages: result,
        tokens_freed,
        collapsed_messages: collapsed.len(),
        boundary_message: Some(boundary),
    }
}

fn unchanged(messages: Vec<Message>) -> ContextCollapseResult {
    ContextCollapseResult {
        messages,
        tokens_freed: 0,
        collapsed_messages: 0,
        boundary_message: None,
    }
}

fn should_preserve_first_message(message: Option<&Message>) -> bool {
    match message {
        Some(Message::System(_)) => true,
        Some(Message::User(user)) => {
            user.tool_use_result.is_none() && user.source_tool_assistant_uuid.is_none()
        }
        _ => false,
    }
}

fn identify_turn_starts(messages: &[Message]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| match message {
            Message::User(user)
                if user.tool_use_result.is_none() && user.source_tool_assistant_uuid.is_none() =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

fn summarize_collapsed_messages(messages: &[Message]) -> String {
    let mut user_messages = 0usize;
    let mut assistant_messages = 0usize;
    let mut tool_uses = 0usize;
    let mut tool_results = 0usize;
    let mut previews = Vec::new();

    for message in messages {
        match message {
            Message::User(user) => {
                user_messages += 1;
                if message_has_tool_result(message) {
                    tool_results += 1;
                } else if previews.len() < 6 {
                    previews.push(format!("user: {}", preview_message_content(&user.content)));
                }
            }
            Message::Assistant(assistant) => {
                assistant_messages += 1;
                tool_uses += assistant
                    .content
                    .iter()
                    .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
                    .count();
                if previews.len() < 6 {
                    previews.push(format!(
                        "assistant: {}",
                        preview_content_blocks(&assistant.content)
                    ));
                }
            }
            Message::System(system) if previews.len() < 6 => {
                previews.push(format!("system: {}", truncate(&system.content, 160)));
            }
            _ => {}
        }
    }

    let preview_text = if previews.is_empty() {
        "No text preview was available.".to_string()
    } else {
        previews.join("\n")
    };

    truncate(
        &format!(
            "Collapsed {} older messages ({} user, {} assistant, {} tool_use, {} tool_result).\n{}",
            messages.len(),
            user_messages,
            assistant_messages,
            tool_uses,
            tool_results,
            preview_text
        ),
        SUMMARY_PREVIEW_LIMIT_CHARS,
    )
}

fn message_has_tool_result(message: &Message) -> bool {
    match message {
        Message::User(user) => match &user.content {
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolResult { .. })),
            MessageContent::Text(_) => false,
        },
        _ => false,
    }
}

fn preview_message_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => truncate(text, 160),
        MessageContent::Blocks(blocks) => preview_content_blocks(blocks),
    }
}

fn preview_content_blocks(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks.iter().take(4) {
        match block {
            ContentBlock::Text { text } => parts.push(truncate(text, 120)),
            ContentBlock::ToolUse { name, .. } => parts.push(format!("tool_use:{name}")),
            ContentBlock::ServerToolUse { name, .. } => {
                parts.push(format!("server_tool_use:{name}"))
            }
            ContentBlock::ToolResult { content, .. } => {
                parts.push(format!("tool_result:{}", preview_tool_result(content)))
            }
            ContentBlock::Thinking { .. } => parts.push("thinking".to_string()),
            ContentBlock::RedactedThinking { .. } => parts.push("redacted_thinking".to_string()),
            ContentBlock::ConnectorText { connector_text, .. } => {
                parts.push(truncate(connector_text, 120))
            }
            ContentBlock::Image { .. } => parts.push("image".to_string()),
        }
    }
    if parts.is_empty() {
        "(empty)".to_string()
    } else {
        parts.join(", ")
    }
}

fn preview_tool_result(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => truncate(text, 80),
        ToolResultContent::Blocks(blocks) => preview_content_blocks(blocks),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, ToolResultContent, UserMessage};

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

    fn make_tool_result(tool_use_id: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: ToolResultContent::Text("result".to_string()),
                is_error: false,
            }]),
            is_meta: true,
            tool_use_result: Some("result".to_string()),
            source_tool_assistant_uuid: Some(Uuid::new_v4()),
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

    fn make_tool_assistant(tool_use_id: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: tool_use_id.to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "src/main.rs"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[test]
    fn context_collapse_replaces_old_turns_with_summary_boundary() {
        let mut messages = vec![make_user("initial context")];
        for turn in 0..50 {
            messages.push(make_user(&format!("question {turn} {}", "x".repeat(120))));
            messages.push(make_assistant(&format!(
                "answer {turn} {}",
                "y".repeat(120)
            )));
        }
        let first_uuid = messages[0].uuid().to_string();
        let removed_uuid = messages[2].uuid().to_string();

        let result = context_collapse_if_needed(messages, "claude-sonnet-4-20250514");

        assert!(result.tokens_freed > 0);
        assert!(result.collapsed_messages > 0);
        assert!(result.messages.len() < 70);
        assert!(matches!(result.messages[1], Message::System(_)));
        match &result.messages[1] {
            Message::System(system) => {
                assert!(system.content.contains("Collapsed"));
                assert!(matches!(
                    system.subtype,
                    SystemSubtype::CompactBoundary { .. }
                ));
                let SystemSubtype::CompactBoundary {
                    compact_metadata: Some(metadata),
                } = &system.subtype
                else {
                    panic!("expected compact metadata");
                };
                let segment = metadata.preserved_segment.as_ref().unwrap();
                let boundary_uuid = result.messages[1].uuid().to_string();
                assert_eq!(
                    segment.summary_message_uuid.as_deref(),
                    Some(boundary_uuid.as_str())
                );
                assert!(segment.preserved_message_uuids.contains(&first_uuid));
                assert!(!segment.preserved_message_uuids.contains(&removed_uuid));
            }
            other => panic!("expected compact boundary, got {other:?}"),
        }
    }

    #[test]
    fn context_collapse_preserves_tool_use_result_pairs_in_tail() {
        let mut messages = vec![make_user("initial context")];
        for turn in 0..45 {
            let tool_id = format!("toolu_{turn}");
            messages.push(make_user(&format!("read file {turn}")));
            messages.push(make_tool_assistant(&tool_id));
            messages.push(make_tool_result(&tool_id));
        }

        let result = context_collapse_if_needed(messages, "claude-sonnet-4-20250514");
        let mut seen_tool_uses = std::collections::HashSet::new();

        for message in &result.messages {
            match message {
                Message::Assistant(assistant) => {
                    for block in &assistant.content {
                        if let ContentBlock::ToolUse { id, .. } = block {
                            seen_tool_uses.insert(id.clone());
                        }
                    }
                }
                Message::User(user) => {
                    if let MessageContent::Blocks(blocks) = &user.content {
                        for block in blocks {
                            if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                                assert!(
                                    seen_tool_uses.contains(tool_use_id),
                                    "orphaned tool_result remained after collapse: {tool_use_id}"
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
