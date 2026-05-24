/// 消息工具函数
///
/// 对应 TypeScript 中散布在多处的消息处理辅助函数
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

/// 从消息中提取纯文本内容
pub fn get_text_content(message: &Message) -> String {
    match message {
        Message::User(m) => match &m.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        },
        Message::Assistant(m) => m
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Message::System(m) => m.content.clone(),
        Message::Progress(_) => String::new(),
        Message::Attachment(_) => String::new(),
    }
}

/// 估算消息的 token 数 (粗略: 4 chars ≈ 1 token)
pub fn estimate_tokens(message: &Message) -> u64 {
    let text = get_text_content(message);
    (text.len() as u64 / 4).max(1)
}

/// Count the number of tool calls (ToolUse blocks) with the given tool name
/// across all assistant messages.
pub fn count_tool_calls(messages: &[Message], tool_name: &str) -> usize {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => Some(&a.content),
            _ => None,
        })
        .flat_map(|content| content.iter())
        .filter(|block| matches!(block, ContentBlock::ToolUse { name, .. } if name == tool_name))
        .count()
}

/// Check if the last assistant message contains any tool use blocks.
///
/// Returns `false` if there are no assistant messages.
pub fn last_message_has_tool_use(messages: &[Message]) -> bool {
    messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant(a) => Some(a),
            _ => None,
        })
        .map(|a| {
            a.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        })
        .unwrap_or(false)
}

/// Extract text content from the last assistant message.
///
/// Concatenates all Text blocks in the last assistant message.
/// Returns `None` if there are no assistant messages or if the last
/// assistant message contains no text blocks.
pub fn get_last_assistant_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant(a) => Some(a),
            _ => None,
        })
        .and_then(|a| {
            let text_parts: Vec<&str> = a
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();

            if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join(""))
            }
        })
}

/// Get all tool use IDs from the last assistant message.
///
/// Returns an empty vec if there are no assistant messages.
pub fn get_last_assistant_tool_use_ids(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant(a) => Some(a),
            _ => None,
        })
        .map(|a| {
            a.content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Count total assistant messages.
pub fn count_assistant_messages(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m, Message::Assistant(_)))
        .count()
}

/// Count total user messages (excluding meta/system-injected ones).
pub fn count_user_messages(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m, Message::User(u) if !u.is_meta))
        .count()
}

// ---------------------------------------------------------------------------
// Formatting & truncation utilities
// ---------------------------------------------------------------------------

/// Truncate text to a maximum length, adding an ellipsis if truncated.
pub fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }
    let content_max = max_len - 3; // room for "..."
                                   // Find the last char boundary that fits within content_max bytes
    let mut boundary = content_max;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}...", &text[..boundary])
}

/// Format a message role as a display string.
pub fn format_role(message: &Message) -> &'static str {
    match message {
        Message::User(u) => {
            if u.is_meta {
                "system"
            } else {
                "user"
            }
        }
        Message::Assistant(_) => "assistant",
        Message::System(_) => "system",
        Message::Progress(_) => "progress",
        Message::Attachment(_) => "attachment",
    }
}

/// Summarize a message into a one-line preview.
///
/// Returns `(role, preview_text)` where preview_text is truncated.
pub fn summarize_message(message: &Message, max_preview: usize) -> (String, String) {
    let role = format_role(message).to_string();
    let text = get_text_content(message);
    let preview = truncate_text(&text.replace('\n', " "), max_preview);
    (role, preview)
}

/// Count total tokens across all messages (rough estimate).
pub fn estimate_total_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_tokens).sum()
}

/// Count tool use blocks across all messages.
pub fn count_all_tool_calls(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => Some(&a.content),
            _ => None,
        })
        .flat_map(|content| content.iter())
        .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
        .count()
}

/// Extract all unique tool names used across messages.
pub fn get_unique_tool_names(messages: &[Message]) -> Vec<String> {
    let mut names: Vec<String> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => Some(&a.content),
            _ => None,
        })
        .flat_map(|content| content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Get the role of the last message.
pub fn last_message_role(messages: &[Message]) -> Option<&'static str> {
    messages.last().map(format_role)
}

/// Extract all text from all assistant messages, concatenated.
pub fn get_all_assistant_text(messages: &[Message]) -> String {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(a) => Some(a),
            _ => None,
        })
        .flat_map(|a| a.content.iter())
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a conversation summary: message counts, tool usage, token estimate.
pub fn conversation_summary(messages: &[Message]) -> String {
    let user_count = count_user_messages(messages);
    let assistant_count = count_assistant_messages(messages);
    let tool_count = count_all_tool_calls(messages);
    let token_est = estimate_total_tokens(messages);
    let tools = get_unique_tool_names(messages);

    let mut lines = vec![
        format!(
            "Messages: {} user, {} assistant",
            user_count, assistant_count
        ),
        format!("Tool calls: {}", tool_count),
        format!("Estimated tokens: {}", token_est),
    ];

    if !tools.is_empty() {
        lines.push(format!("Tools used: {}", tools.join(", ")));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, MessageContent, UserMessage};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_user_message(text: &str, is_meta: bool) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn make_assistant_with_tool_use(tool_name: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me run that.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: format!("tu_{}", tool_name),
                    name: tool_name.to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn make_assistant_text_only(text: &str) -> Message {
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
    fn test_count_tool_calls() {
        let messages = vec![
            make_assistant_with_tool_use("bash"),
            make_assistant_with_tool_use("bash"),
            make_assistant_with_tool_use("file_read"),
        ];

        assert_eq!(count_tool_calls(&messages, "bash"), 2);
        assert_eq!(count_tool_calls(&messages, "file_read"), 1);
        assert_eq!(count_tool_calls(&messages, "file_write"), 0);
    }

    #[test]
    fn test_last_message_has_tool_use() {
        let messages = vec![
            make_assistant_with_tool_use("bash"),
            make_assistant_text_only("Done!"),
        ];
        // Last assistant message is text-only
        assert!(!last_message_has_tool_use(&messages));

        let messages2 = vec![
            make_assistant_text_only("Let me check"),
            make_assistant_with_tool_use("bash"),
        ];
        // Last assistant message has tool use
        assert!(last_message_has_tool_use(&messages2));
    }

    #[test]
    fn test_last_message_has_tool_use_empty() {
        let messages: Vec<Message> = vec![];
        assert!(!last_message_has_tool_use(&messages));
    }

    #[test]
    fn test_get_last_assistant_text() {
        let messages = vec![
            make_assistant_with_tool_use("bash"),
            make_assistant_text_only("The result is 42."),
        ];
        assert_eq!(
            get_last_assistant_text(&messages),
            Some("The result is 42.".to_string())
        );
    }

    #[test]
    fn test_get_last_assistant_text_none() {
        let messages: Vec<Message> = vec![];
        assert_eq!(get_last_assistant_text(&messages), None);
    }

    #[test]
    fn test_get_last_assistant_tool_use_ids() {
        let messages = vec![make_assistant_with_tool_use("bash")];
        let ids = get_last_assistant_tool_use_ids(&messages);
        assert_eq!(ids, vec!["tu_bash".to_string()]);
    }

    #[test]
    fn test_count_user_messages_excludes_meta() {
        let messages = vec![
            make_user_message("real input", false),
            make_user_message("system injected", true),
            make_user_message("another real", false),
        ];

        assert_eq!(count_user_messages(&messages), 2);
    }

    #[test]
    fn test_get_text_content_user() {
        let msg = make_user_message("hello world", false);
        assert_eq!(get_text_content(&msg), "hello world");
    }

    #[test]
    fn test_get_text_content_assistant() {
        let msg = make_assistant_text_only("response text");
        assert_eq!(get_text_content(&msg), "response text");
    }

    // ── New formatting & truncation tests ──────────────────────────

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(truncate_text("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_text_exact() {
        assert_eq!(truncate_text("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_text_long() {
        let result = truncate_text("hello world, this is a long text", 20);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 20);
        // Original text was 32 chars, truncated should be shorter
        assert!(result.len() < 32);
    }

    #[test]
    fn test_truncate_text_tiny_max() {
        assert_eq!(truncate_text("hello", 3), "...");
    }

    #[test]
    fn test_format_role() {
        let msg = make_assistant_text_only("hi");
        assert_eq!(format_role(&msg), "assistant");
    }

    #[test]
    fn test_summarize_message() {
        let msg = make_assistant_text_only("The quick brown fox jumps over the lazy dog");
        let (role, preview) = summarize_message(&msg, 20);
        assert_eq!(role, "assistant");
        assert!(preview.len() <= 23); // 20 + "..."
    }

    #[test]
    fn test_estimate_total_tokens() {
        let messages = vec![
            make_assistant_text_only("short"),
            make_assistant_text_only("another message here"),
        ];
        let total = estimate_total_tokens(&messages);
        assert!(total > 0);
    }

    #[test]
    fn test_count_all_tool_calls() {
        let messages = vec![
            make_assistant_with_tool_use("bash"),
            make_assistant_with_tool_use("grep"),
            make_assistant_text_only("done"),
        ];
        assert_eq!(count_all_tool_calls(&messages), 2);
    }

    #[test]
    fn test_get_unique_tool_names() {
        let messages = vec![
            make_assistant_with_tool_use("bash"),
            make_assistant_with_tool_use("grep"),
            make_assistant_with_tool_use("bash"),
        ];
        let names = get_unique_tool_names(&messages);
        assert_eq!(names, vec!["bash", "grep"]);
    }

    #[test]
    fn test_last_message_role() {
        let messages = vec![make_assistant_text_only("hi")];
        assert_eq!(last_message_role(&messages), Some("assistant"));
        assert_eq!(last_message_role(&[]), None);
    }

    #[test]
    fn test_conversation_summary() {
        let messages = vec![
            make_user_message("hello", false),
            make_assistant_with_tool_use("bash"),
        ];
        let summary = conversation_summary(&messages);
        assert!(summary.contains("1 user"));
        assert!(summary.contains("1 assistant"));
        assert!(summary.contains("Tool calls: 1"));
        assert!(summary.contains("bash"));
    }
}
