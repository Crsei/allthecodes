//! BriefTool -- structured output for Brief mode.
//!
//! In Brief mode this is the ONLY way the model communicates with the user.
//! Plain text output is treated as internal reasoning. The daemon routes
//! BriefTool results as `brief_message` SSE events.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_config::features::{self, Feature};
use allthecodes_types::brief::BRIEF_TOOL_NAME;
use allthecodes_types::message::AssistantMessage;

/// BriefTool -- send a structured brief message to the user.
pub struct BriefTool;

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &str {
        BRIEF_TOOL_NAME
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a structured brief message to the user in Brief mode.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Markdown formatted message to send to the user"
                },
                "attachments": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional file paths to attach"
                },
                "status": {
                    "type": "string",
                    "enum": ["normal", "proactive"],
                    "description": "Message status (default: normal)"
                }
            },
            "required": ["message"]
        })
    }

    fn is_enabled(&self) -> bool {
        features::enabled(Feature::KairosBrief)
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

        // Validate status if provided
        if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
            if !matches!(status, "normal" | "proactive") {
                return ValidationResult::Error {
                    message: format!(
                        "Unknown status \"{}\". Must be normal or proactive.",
                        status
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
        let status = input
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("normal")
            .to_string();
        let attachments = input
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ToolResult {
            data: json!({
                "is_brief_message": true,
                "message": message,
                "status": status,
                "attachments": attachments,
            }),
            display_preview: Some(message),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Brief is the ONLY way to communicate with the user in Brief mode.\n\n\
All plain text output is treated as internal reasoning and will NOT be shown \
to the user. You MUST use the Brief tool for every message you want the user \
to see.\n\n\
Parameters:\n\
- \"message\" (required): Markdown formatted text to display.\n\
- \"attachments\" (optional): Array of file paths to attach.\n\
- \"status\" (optional): \"normal\" (default) or \"proactive\"."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Brief".to_string()
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
    fn test_brief_tool_name() {
        let tool = BriefTool;
        assert_eq!(tool.name(), "Brief");
    }

    #[test]
    fn test_brief_tool_schema() {
        let tool = BriefTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("message"));
        assert!(props.contains_key("attachments"));
        assert!(props.contains_key("status"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("message")));
    }

    #[tokio::test]
    async fn test_brief_tool_validates_empty_message() {
        let tool = BriefTool;
        let ctx = create_test_context();
        let input = json!({ "message": "" });
        let result = tool.validate_input(&input, &ctx).await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("must not be empty"));
            }
            ValidationResult::Ok => panic!("expected error for empty message"),
        }
    }

    #[tokio::test]
    async fn test_brief_tool_validates_bad_status() {
        let tool = BriefTool;
        let ctx = create_test_context();
        let input = json!({ "message": "hello", "status": "bad" });
        let result = tool.validate_input(&input, &ctx).await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("Unknown status"));
            }
            ValidationResult::Ok => panic!("expected error for bad status"),
        }
    }

    #[test]
    fn test_brief_tool_is_read_only() {
        let tool = BriefTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_brief_tool_is_concurrency_safe() {
        let tool = BriefTool;
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_brief_tool_user_facing_name() {
        let tool = BriefTool;
        assert_eq!(tool.user_facing_name(None), "Brief");
    }

    #[tokio::test]
    async fn test_brief_tool_returns_structured_payload_and_preview() {
        let tool = BriefTool;
        let ctx = create_test_context();
        let parent = parent_message();
        let result = tool
            .call(
                json!({
                    "message": "brief body",
                    "status": "proactive",
                    "attachments": ["docs/brief.md"],
                }),
                &ctx,
                &parent,
                None,
            )
            .await
            .expect("brief tool call should succeed");

        assert_eq!(result.data["is_brief_message"], true);
        assert_eq!(result.data["message"], "brief body");
        assert_eq!(result.data["status"], "proactive");
        assert_eq!(result.data["attachments"], json!(["docs/brief.md"]));
        assert_eq!(result.display_preview.as_deref(), Some("brief body"));
    }
}
