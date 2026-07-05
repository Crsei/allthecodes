//! SendUserMessage tool -- sends a brief message to the user.
//!
//! This is a simple tool that packages a message with an optional severity
//! level (info, warning, error) into a ToolResult. It does not perform any
//! I/O beyond returning the message data.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::brief::SEND_USER_MESSAGE_TOOL_NAME;
use allthecodes_types::message::AssistantMessage;

/// SendUserMessageTool -- send a brief message to the user.
pub struct SendUserMessageTool;

#[async_trait]
impl Tool for SendUserMessageTool {
    fn name(&self) -> &str {
        SEND_USER_MESSAGE_TOOL_NAME
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
                "is_brief_message": true,
                "message": message,
                "status": "normal",
                "level": level,
            }),
            display_preview: Some(message),
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
    use crate::tool::ToolAppState as AppState;
    use crate::tool::{FileStateCache, ToolUseOptions};
    use allthecodes_types::message::ContentBlock;
    use std::sync::Arc;
    use uuid::Uuid;

    fn create_test_context() -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolUseContext {
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".into(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(AppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: Vec::<ContentBlock>::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

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

    #[tokio::test]
    async fn test_send_user_message_returns_brief_payload_and_preview() {
        let tool = SendUserMessageTool;
        let ctx = create_test_context();
        let parent = parent_message();
        let result = tool
            .call(
                json!({
                    "message": "status update",
                    "level": "warning",
                }),
                &ctx,
                &parent,
                None,
            )
            .await
            .expect("send user message tool call should succeed");

        assert_eq!(result.data["is_brief_message"], true);
        assert_eq!(result.data["message"], "status update");
        assert_eq!(result.data["status"], "normal");
        assert_eq!(result.data["level"], "warning");
        assert_eq!(result.display_preview.as_deref(), Some("status update"));
    }
}
