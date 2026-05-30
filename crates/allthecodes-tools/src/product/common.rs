//! Shared product-tool message and text utilities.

use std::collections::HashMap;

use allthecodes_types::message::{ContentBlock, Message, MessageContent, ToolResultContent};

#[derive(Debug)]
pub(super) struct CapturedToolOutput {
    pub(super) tool_use_id: String,
    pub(super) source_tool: String,
    pub(super) content: String,
}

pub(super) fn shell_tool_use_ids(messages: &[Message]) -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for message in messages {
        let Message::Assistant(assistant) = message else {
            continue;
        };
        for block in &assistant.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                if matches!(name.as_str(), "Bash" | "PowerShell" | "REPL") {
                    ids.insert(id.clone(), name.clone());
                }
            }
        }
    }
    ids
}

pub(super) fn find_tool_result_text(
    messages: &[Message],
    requested_id: Option<&str>,
    allowed_ids: &HashMap<String, String>,
) -> Option<CapturedToolOutput> {
    for message in messages.iter().rev() {
        let Message::User(user) = message else {
            continue;
        };
        let MessageContent::Blocks(blocks) = &user.content else {
            continue;
        };
        for block in blocks.iter().rev() {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } = block
            else {
                continue;
            };
            if requested_id.is_some_and(|id| id != tool_use_id) {
                continue;
            }
            let source_tool = allowed_ids.get(tool_use_id)?;
            return Some(CapturedToolOutput {
                tool_use_id: tool_use_id.clone(),
                source_tool: source_tool.clone(),
                content: tool_result_text(content),
            });
        }
    }
    None
}

pub(super) fn tool_result_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .map(content_block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub(super) fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => {
            format!("{name}: {}", truncate_chars(&input.to_string(), 200))
        }
        ContentBlock::ToolResult { content, .. } => tool_result_text(content),
        ContentBlock::Thinking { thinking, .. } => thinking.clone(),
        ContentBlock::RedactedThinking { .. } => "[redacted thinking]".to_string(),
        ContentBlock::ConnectorText { connector_text, .. } => connector_text.clone(),
        ContentBlock::Image { source } => format!("[image {}]", source.media_type),
    }
}

pub(super) fn message_kind(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::System(_) => "system",
        Message::Progress(_) => "progress",
        Message::Attachment(_) => "attachment",
    }
}

pub(super) fn message_text(message: &Message) -> String {
    match message {
        Message::User(user) => match &user.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(content_block_text)
                .collect::<Vec<_>>()
                .join("\n"),
        },
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n"),
        Message::System(system) => system.content.clone(),
        Message::Progress(progress) => progress.data.to_string(),
        Message::Attachment(attachment) => serde_json::to_string(&attachment.attachment)
            .unwrap_or_else(|_| "attachment".to_string()),
    }
}

pub(super) fn tail_lines(content: &str, max_lines: usize) -> String {
    let mut lines = content.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

pub(super) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let take = max_chars.saturating_sub(3);
    let mut out = value.chars().take(take).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_lines_returns_requested_suffix() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }
}
