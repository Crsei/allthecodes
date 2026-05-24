//! Base protocol types shared across all domains.

use serde::{Deserialize, Serialize};

use allthecodes_types::message::ContentBlock;

/// Lightweight description of a content block in a tool result,
/// suitable for forwarding to the frontend.
///
/// For images we now forward the base64 data in addition to metadata so that
/// the frontend can render screenshots inline (needed for the browser MCP
/// viewing loop — see `docs/reference/browser-mcp-config.md`). The data field
/// is skipped when the image originated from a source where the frontend
/// shouldn't re-render it (rare — kept optional for that future path).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContentInfo {
    /// Plain text content.
    Text { text: String },
    /// An image was returned. The `data` field carries base64-encoded bytes
    /// when available; `media_type` and `size_bytes` are always present for
    /// frontends that only want to show a placeholder.
    Image {
        /// MIME type (e.g. "image/png").
        media_type: String,
        /// Approximate byte size of the base64-decoded image data.
        #[serde(skip_serializing_if = "Option::is_none")]
        size_bytes: Option<usize>,
        /// Base64-encoded image payload (omitted when caller chose not to
        /// forward the bytes — e.g. for very large images behind a feature flag).
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
}

/// A single conversation message as seen by the frontend.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConversationMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::ContentBlock;

    #[test]
    fn conversation_message_serializes_content_blocks_when_present() {
        let message = ConversationMessage {
            id: "assistant-1".to_string(),
            role: "assistant".to_string(),
            content: "summary".to_string(),
            timestamp: 1,
            content_blocks: Some(vec![
                ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({ "file_path": "/tmp/a.ts" }),
                },
                ContentBlock::Text {
                    text: "summary".to_string(),
                },
            ]),
            cost_usd: Some(0.01),
            thinking: None,
            level: None,
        };

        let value = serde_json::to_value(&message).expect("serialize conversation message");
        let blocks = value
            .get("content_blocks")
            .and_then(|v| v.as_array())
            .expect("content_blocks array");

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "tool_use");
        assert_eq!(blocks[1]["type"], "text");
    }

    #[test]
    fn conversation_message_omits_content_blocks_when_absent() {
        let message = ConversationMessage {
            id: "system-1".to_string(),
            role: "system".to_string(),
            content: "info".to_string(),
            timestamp: 1,
            content_blocks: None,
            cost_usd: None,
            thinking: None,
            level: Some("info".to_string()),
        };

        let value = serde_json::to_value(&message).expect("serialize conversation message");
        assert!(value.get("content_blocks").is_none());
    }

    #[test]
    fn backend_question_request_serializes() {
        use super::super::BackendMessage;
        let msg = BackendMessage::QuestionRequest {
            id: "q-1".into(),
            text: "Continue?".into(),
            choices: vec!["Yes".into(), "No".into()],
            allow_free_text: false,
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "question_request");
        assert_eq!(json["id"], "q-1");
        assert_eq!(json["text"], "Continue?");
        assert_eq!(json["choices"][0], "Yes");
        assert_eq!(json["allow_free_text"], false);
    }

    #[test]
    fn backend_tombstone_serializes() {
        use super::super::BackendMessage;
        let msg = BackendMessage::Tombstone {
            message_id: "assistant-1".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "tombstone");
        assert_eq!(json["message_id"], "assistant-1");
    }

    #[test]
    fn frontend_question_response_deserializes() {
        use super::super::FrontendMessage;
        let json = r#"{"type":"question_response","id":"q-1","text":"yes"}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        match msg {
            FrontendMessage::QuestionResponse { id, text, .. } => {
                assert_eq!(id, "q-1");
                assert_eq!(text, "yes");
            }
            _ => panic!("expected QuestionResponse"),
        }
    }

    #[test]
    fn frontend_control_commands_deserialize() {
        use super::super::FrontendMessage;

        let submit: FrontendMessage =
            serde_json::from_str(r#"{"type":"submit_prompt","text":"hello","id":"ui-1"}"#).unwrap();
        assert!(matches!(
            submit,
            FrontendMessage::SubmitPrompt { text, id } if text == "hello" && id == "ui-1"
        ));

        let slash: FrontendMessage =
            serde_json::from_str(r#"{"type":"slash_command","raw":"/help"}"#).unwrap();
        assert!(matches!(
            slash,
            FrontendMessage::SlashCommand { raw } if raw == "/help"
        ));

        let permission: FrontendMessage = serde_json::from_str(
            r#"{"type":"permission_response","tool_use_id":"tool-1","decision":"allow"}"#,
        )
        .unwrap();
        assert!(matches!(
            permission,
            FrontendMessage::PermissionResponse {
                tool_use_id,
                decision,
                feedback,
                ..
            } if tool_use_id == "tool-1" && decision == "allow" && feedback.is_none()
        ));

        let abort: FrontendMessage = serde_json::from_str(r#"{"type":"abort_query"}"#).unwrap();
        assert!(matches!(abort, FrontendMessage::AbortQuery));

        let quit: FrontendMessage = serde_json::from_str(r#"{"type":"quit"}"#).unwrap();
        assert!(matches!(quit, FrontendMessage::Quit));
    }
}
