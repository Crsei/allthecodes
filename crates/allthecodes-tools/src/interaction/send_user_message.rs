//! SendUserMessage tool -- sends a brief message to the user.
//!
//! This is a simple tool that packages a message with an optional severity
//! level (info, warning, error) into a ToolResult. It does not perform any
//! I/O beyond returning the message data.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;

/// SendUserMessageTool -- send a brief message to the user.
pub struct SendUserMessageTool;

#[async_trait]
impl Tool for SendUserMessageTool {
    fn name(&self) -> &str {
        "SendUserMessage"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a brief message to the user.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to send to the user"
                },
                "level": {
                    "type": "string",
                    "enum": ["info", "warning", "error"],
                    "description": "Message severity level (default: info)"
                }
            },
            "required": ["message"]
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
        if message.is_empty() {
            return ValidationResult::Error {
                message: "\"message\" must not be empty".to_string(),
                error_code: 1,
            };
        }

        // Validate level if provided
        if let Some(level) = input.get("level").and_then(|v| v.as_str()) {
            if !matches!(level, "info" | "warning" | "error") {
                return ValidationResult::Error {
                    message: format!(
                        "Unknown level \"{}\". Must be info, warning, or error.",
                        level
                    ),
                    error_code: 1,
                };
            }
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let level = input
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string();

        Ok(ToolResult {
            data: json!({
                "message": message,
                "level": level,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Use SendUserMessage to send a brief notification to the user.\n\n\
Supports three severity levels:\n\
- \"info\" (default): General information.\n\
- \"warning\": Something that may need attention.\n\
- \"error\": An error or failure notification.\n\n\
The message is displayed to the user as-is."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "SendUserMessage".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_user_message_tool_name() {
        let tool = SendUserMessageTool;
        assert_eq!(tool.name(), "SendUserMessage");
    }

    #[test]
    fn test_send_user_message_schema() {
        let tool = SendUserMessageTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("message"));
        assert!(props.contains_key("level"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("message")));
    }

    #[test]
    fn test_send_user_message_is_read_only() {
        let tool = SendUserMessageTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_send_user_message_is_concurrency_safe() {
        let tool = SendUserMessageTool;
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_send_user_message_user_facing_name() {
        let tool = SendUserMessageTool;
        assert_eq!(tool.user_facing_name(None), "SendUserMessage");
    }
}
