//! Result extraction and success-checking helpers.
//!
//! Corresponds to TypeScript: queryHelpers.ts (`isResultSuccessful`, etc.)

use crate::types::message::{ContentBlock, Message, MessageContent};

// ---------------------------------------------------------------------------
// is_result_successful
// ---------------------------------------------------------------------------

/// Check whether the query result should be considered successful.
///
/// Corresponds to TypeScript: `isResultSuccessful()` in queryHelpers.ts
///
/// Success conditions:
/// - Assistant message whose last content block is Text, Thinking, or
///   RedactedThinking; connector text is treated as text-like output.
/// - User message where **all** content blocks are ToolResult.
/// - `stop_reason == "end_turn"`.
pub fn is_result_successful(message: Option<&Message>, stop_reason: Option<&str>) -> bool {
    if stop_reason == Some("end_turn") {
        return true;
    }

    let msg = match message {
        Some(m) => m,
        None => return false,
    };

    match msg {
        Message::Assistant(assistant) => {
            if let Some(last) = assistant.content.last() {
                matches!(
                    last,
                    ContentBlock::Text { .. }
                        | ContentBlock::ConnectorText { .. }
                        | ContentBlock::Thinking { .. }
                        | ContentBlock::RedactedThinking { .. }
                )
            } else {
                false
            }
        }
        Message::User(user) => {
            let blocks = match &user.content {
                MessageContent::Blocks(blocks) => blocks,
                MessageContent::Text(_) => return false,
            };
            if blocks.is_empty() {
                return false;
            }
            blocks
                .iter()
                .all(|b| matches!(b, ContentBlock::ToolResult { .. }))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// extract_text_result
// ---------------------------------------------------------------------------

/// Extract a text result from the last assistant message in the conversation.
///
/// Returns `(text_result, is_api_error)`.
///
/// - `text_result`: the text of the last `ContentBlock::Text` in the terminal
///   assistant message, or an empty string if there is none.
/// - `is_api_error`: `true` when the terminal assistant message was
///   synthesised from an API error.
pub fn extract_text_result(messages: &[Message]) -> (String, bool) {
    let terminal = find_terminal_message(messages);

    match terminal {
        Some(Message::Assistant(assistant)) => {
            let text = assistant
                .content
                .iter()
                .rev()
                .find_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ConnectorText { connector_text, .. } => {
                        Some(connector_text.clone())
                    }
                    _ => None,
                })
                .unwrap_or_default();

            (text, assistant.is_api_error_message)
        }
        _ => (String::new(), false),
    }
}

// ---------------------------------------------------------------------------
// find_terminal_message
// ---------------------------------------------------------------------------

/// Find the last assistant or user message in the conversation, skipping
/// progress, attachment, and system messages.
pub fn find_terminal_message(messages: &[Message]) -> Option<&Message> {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m, Message::Assistant(_) | Message::User(_)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{
        AssistantMessage, ContentBlock, MessageContent, ToolResultContent, UserMessage,
    };
    use uuid::Uuid;

    fn make_assistant(content: Vec<ContentBlock>) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content,
            usage: None,
            stop_reason: Some("end_turn".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn make_user_text(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn make_user_tool_results() -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: ToolResultContent::Text("ok".to_string()),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn test_is_result_successful_assistant_text() {
        let msg = make_assistant(vec![ContentBlock::Text {
            text: "Hello".to_string(),
        }]);
        assert!(is_result_successful(Some(&msg), Some("end_turn")));
    }

    #[test]
    fn test_is_result_successful_assistant_thinking() {
        let msg = make_assistant(vec![ContentBlock::Thinking {
            thinking: "hmm".to_string(),
            signature: None,
        }]);
        assert!(is_result_successful(Some(&msg), None));
    }

    #[test]
    fn test_is_result_successful_user_tool_results() {
        let msg = make_user_tool_results();
        assert!(is_result_successful(Some(&msg), None));
    }

    #[test]
    fn test_is_result_successful_none() {
        assert!(!is_result_successful(None, None));
    }

    #[test]
    fn test_is_result_successful_end_turn_stop_reason() {
        let msg = make_user_text("just text");
        // "end_turn" stop_reason is always successful.
        assert!(is_result_successful(Some(&msg), Some("end_turn")));
    }

    #[test]
    fn test_extract_text_result_basic() {
        let messages = vec![
            make_user_text("hi"),
            make_assistant(vec![ContentBlock::Text {
                text: "Hello!".to_string(),
            }]),
        ];
        let (text, is_error) = extract_text_result(&messages);
        assert_eq!(text, "Hello!");
        assert!(!is_error);
    }

    #[test]
    fn test_extract_text_result_empty() {
        let messages: Vec<Message> = vec![];
        let (text, is_error) = extract_text_result(&messages);
        assert_eq!(text, "");
        assert!(!is_error);
    }

    #[test]
    fn test_find_terminal_message_skips_system() {
        let messages = vec![
            make_assistant(vec![ContentBlock::Text {
                text: "first".to_string(),
            }]),
            Message::System(crate::types::message::SystemMessage {
                uuid: Uuid::new_v4(),
                timestamp: 0,
                subtype: crate::types::message::SystemSubtype::Warning,
                content: "warning".to_string(),
            }),
        ];
        let terminal = find_terminal_message(&messages);
        assert!(matches!(terminal, Some(Message::Assistant(_))));
    }
}
