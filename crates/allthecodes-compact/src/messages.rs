use chrono::Utc;
use uuid::Uuid;

use allthecodes_types::message::{
    AssistantMessage, Attachment, ContentBlock, InfoLevel, Message, MessageContent, SystemMessage,
    SystemSubtype, ToolResultContent, UserMessage,
};

/// Normalize messages for the API by:
/// - Filtering out progress messages
/// - Filtering out attachment messages (converting relevant ones to user messages)
/// - Ensuring alternating user/assistant pattern
/// - Stripping system-only fields
pub fn normalize_messages_for_api(messages: &[Message]) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::new();

    for msg in messages {
        match msg {
            // Progress messages are never sent to the API
            Message::Progress(_) => continue,

            // System messages are never sent directly to the API
            // (they are injected into the system prompt or handled separately)
            Message::System(_) => continue,

            // Attachment messages: convert relevant ones to user messages
            Message::Attachment(att) => {
                match &att.attachment {
                    // Queued commands become user messages
                    Attachment::QueuedCommand { prompt, .. } => {
                        let user_msg = create_user_message(prompt, true);
                        result.push(user_msg);
                    }
                    // Nested memory becomes a user message with the memory content
                    Attachment::NestedMemory { path, content } => {
                        let text = format!("[Memory from {}]:\n{}", path, content);
                        let user_msg = create_user_message(&text, true);
                        result.push(user_msg);
                    }
                    // Other attachment types are filtered out
                    _ => continue,
                }
            }

            // User and Assistant messages pass through
            Message::User(_) | Message::Assistant(_) => {
                result.push(msg.clone());
            }
        }
    }

    // Ensure alternating user/assistant pattern
    ensure_alternating_pattern(&mut result);

    result
}

/// Ensure messages alternate between user and assistant roles.
/// If two consecutive messages have the same role, insert a synthetic
/// message of the opposite role to maintain the pattern.
fn ensure_alternating_pattern(messages: &mut Vec<Message>) {
    if messages.len() < 2 {
        return;
    }

    let mut i = 0;
    while i + 1 < messages.len() {
        let current_is_user = matches!(&messages[i], Message::User(_));
        let next_is_user = matches!(&messages[i + 1], Message::User(_));

        if current_is_user == next_is_user {
            // Need to insert a synthetic message
            let synthetic = if current_is_user {
                // Insert a synthetic assistant message between two user messages
                Message::Assistant(AssistantMessage {
                    uuid: Uuid::new_v4(),
                    timestamp: Utc::now().timestamp_millis(),
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "[continued]".to_string(),
                    }],
                    usage: None,
                    stop_reason: Some("end_turn".to_string()),
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: 0.0,
                })
            } else {
                // Insert a synthetic user message between two assistant messages
                create_user_message("[continued]", true)
            };
            messages.insert(i + 1, synthetic);
        }
        i += 1;
    }

    // Ensure first message is a user message (API requirement)
    if !messages.is_empty() && !matches!(&messages[0], Message::User(_)) {
        let synthetic_user = create_user_message("[start]", true);
        messages.insert(0, synthetic_user);
    }
}

/// Get messages after the last compact boundary.
/// Returns a slice starting after the most recent CompactBoundary system message.
/// If no compact boundary exists, returns all messages.
pub fn get_messages_after_compact_boundary(messages: &[Message]) -> &[Message] {
    // Find the last compact boundary
    let mut last_boundary_idx: Option<usize> = None;
    for (i, msg) in messages.iter().enumerate() {
        if let Message::System(sys) = msg {
            if matches!(&sys.subtype, SystemSubtype::CompactBoundary { .. }) {
                last_boundary_idx = Some(i);
            }
        }
    }

    match last_boundary_idx {
        Some(idx) => {
            if idx + 1 < messages.len() {
                &messages[idx + 1..]
            } else {
                &[]
            }
        }
        None => messages,
    }
}

/// Create a user message with text content.
pub fn create_user_message(content: &str, is_meta: bool) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(content.to_string()),
        is_meta,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

/// Create a user message with a tool result.
pub fn create_tool_result_message(tool_use_id: &str, content: &str, is_error: bool) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: ToolResultContent::Text(content.to_string()),
            is_error,
        }]),
        is_meta: false,
        tool_use_result: Some(content.to_string()),
        source_tool_assistant_uuid: None,
    })
}

/// Create an assistant API error message.
/// These are synthetic assistant messages that represent API errors,
/// allowing the conversation to continue with error context.
pub fn create_assistant_api_error_message(content: &str, error_type: Option<&str>) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: content.to_string(),
        }],
        usage: None,
        stop_reason: Some("error".to_string()),
        is_api_error_message: true,
        api_error: error_type.map(|s| s.to_string()),
        cost_usd: 0.0,
    })
}

/// Create a system message with the given level.
pub fn create_system_message(content: &str, level: &str) -> Message {
    let info_level = match level {
        "warning" => InfoLevel::Warning,
        "error" => InfoLevel::Error,
        _ => InfoLevel::Info,
    };

    Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp_millis(),
        subtype: SystemSubtype::Informational { level: info_level },
        content: content.to_string(),
    })
}

/// Create a user interruption message.
/// If `tool_use` is true, the interruption happened during tool execution;
/// otherwise it happened during streaming.
pub fn create_user_interruption_message(tool_use: bool) -> Message {
    let text = if tool_use {
        "User interrupted tool execution. The tool call was cancelled. \
         Please acknowledge the interruption and ask how to proceed."
    } else {
        "User interrupted the response. Please acknowledge the interruption \
         and ask how to proceed."
    };

    create_user_message(text, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_user_message() {
        let msg = create_user_message("hello", false);
        if let Message::User(u) = &msg {
            assert_eq!(u.role, "user");
            assert!(!u.is_meta);
            if let MessageContent::Text(t) = &u.content {
                assert_eq!(t, "hello");
            } else {
                panic!("expected text content");
            }
        } else {
            panic!("expected user message");
        }
    }

    #[test]
    fn test_create_tool_result_message() {
        let msg = create_tool_result_message("tu_123", "result data", false);
        if let Message::User(u) = &msg {
            if let MessageContent::Blocks(blocks) = &u.content {
                assert_eq!(blocks.len(), 1);
                if let ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } = &blocks[0]
                {
                    assert_eq!(tool_use_id, "tu_123");
                    assert!(!is_error);
                } else {
                    panic!("expected ToolResult block");
                }
            } else {
                panic!("expected blocks content");
            }
        } else {
            panic!("expected user message");
        }
    }

    #[test]
    fn test_normalize_filters_progress() {
        let messages = vec![
            create_user_message("hi", false),
            Message::Progress(allthecodes_types::message::ProgressMessage {
                uuid: Uuid::new_v4(),
                timestamp: 0,
                tool_use_id: "x".into(),
                data: serde_json::Value::Null,
            }),
        ];
        let normalized = normalize_messages_for_api(&messages);
        // Progress messages should be filtered out
        assert!(normalized
            .iter()
            .all(|m| !matches!(m, Message::Progress(_))));
    }

    #[test]
    fn test_get_messages_after_compact_boundary() {
        use allthecodes_types::message::CompactMetadata;

        let messages = vec![
            create_user_message("old message", false),
            Message::System(SystemMessage {
                uuid: Uuid::new_v4(),
                timestamp: 0,
                subtype: SystemSubtype::CompactBoundary {
                    compact_metadata: Some(CompactMetadata {
                        pre_compact_token_count: 1000,
                        post_compact_token_count: 200,
                        preserved_segment: None,
                        pre_compact_discovered_tools: None,
                    }),
                },
                content: "compacted".into(),
            }),
            create_user_message("new message", false),
        ];

        let after = get_messages_after_compact_boundary(&messages);
        assert_eq!(after.len(), 1);
    }
}
