//! AskUser tool — prompts the user for input via terminal interaction.
//!
//! Corresponds to TypeScript: tools/AskUserQuestionTool/
//!
//! In interactive mode, reads a line from stdin. In non-interactive mode,
//! returns a message indicating the user cannot be prompted and the model
//! should proceed with its best judgment.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

use crate::tool::*;
use allthecodes_types::message::AssistantMessage;

/// AskUserQuestion tool — asks the user a question and waits for response.
pub struct AskUserQuestionTool;

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    async fn description(&self, _input: &Value) -> String {
        "Ask the user a question and wait for their response.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "choices": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional structured choices shown to the user"
                },
                "allow_free_text": {
                    "type": "boolean",
                    "description": "Whether the user may answer with free text in addition to choosing an option"
                }
            },
            "required": ["question"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false // User interaction is inherently serial
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("(no question provided)");
        let choices = input
            .get("choices")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let allow_free_text = input
            .get("allow_free_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if let Some(callback) = &ctx.ask_user_callback {
            debug!(
                question = question,
                "AskUser: routing question through callback"
            );
            let answer = callback(AskUserRequestPayload {
                question: question.to_string(),
                choices,
                allow_free_text,
            })
            .await;
            debug!(
                question = question,
                answer_len = answer.len(),
                "AskUser: received callback response"
            );

            let response = if answer.trim().is_empty() {
                "(User provided no response)".to_string()
            } else {
                answer
            };

            return Ok(ToolResult {
                data: json!(response),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Non-interactive sessions cannot prompt the user
        if ctx.options.is_non_interactive_session {
            debug!(
                question = question,
                "AskUser: non-interactive session, skipping prompt"
            );
            return Ok(ToolResult {
                data: json!(
                    "Cannot ask user in non-interactive session. \
                     Proceed with your best judgment."
                ),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Interactive mode: print the question and read from stdin.
        // We use eprintln for the question (so it goes to stderr / visible output)
        // and read the answer from stdin.
        eprintln!("\n{}", question);
        eprint!("> ");

        let answer = read_user_line().await;

        debug!(
            question = question,
            answer_len = answer.len(),
            "AskUser: received user response"
        );

        let response = if answer.trim().is_empty() {
            "(User provided no response)".to_string()
        } else {
            answer
        };

        Ok(ToolResult {
            data: json!(response),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Use AskUserQuestion when you need clarification or input from the user. Use this tool when:\n\
- You need to ask a question to clarify the user's request\n\
- You need the user to make a choice between options\n\
- You need confirmation before proceeding with a potentially risky action\n\
- You are genuinely stuck and need user guidance after investigation".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "AskUserQuestion".to_string()
    }
}

/// Read a single line from stdin asynchronously.
///
/// Falls back to an empty string if reading fails (e.g., stdin is closed).
async fn read_user_line() -> String {
    // Use tokio::task::spawn_blocking to avoid blocking the async runtime
    // since std::io::stdin().read_line() is a blocking operation.
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(_) => line
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string(),
            Err(e) => {
                debug!("AskUser: failed to read stdin: {}", e);
                String::new()
            }
        }
    })
    .await
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ask_user_tool_name() {
        let tool = AskUserQuestionTool;
        assert_eq!(tool.name(), "AskUserQuestion");
    }

    #[test]
    fn test_ask_user_schema() {
        let tool = AskUserQuestionTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("question"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("question")));
    }

    #[test]
    fn test_ask_user_not_concurrency_safe() {
        let tool = AskUserQuestionTool;
        assert!(!tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_ask_user_is_read_only() {
        let tool = AskUserQuestionTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_ask_user_user_facing_name() {
        let tool = AskUserQuestionTool;
        assert_eq!(tool.user_facing_name(None), "AskUserQuestion");
    }

    #[tokio::test]
    async fn test_ask_user_uses_callback_when_available() {
        let tool = AskUserQuestionTool;
        let callback: AskUserCallback = std::sync::Arc::new(|payload| {
            Box::pin(async move { format!("answer for: {}", payload.question) })
        });

        let ctx = ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "gpt-5.2".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: {
                let (_tx, rx) = tokio::sync::watch::channel(false);
                rx
            },
            read_file_state: FileStateCache::default(),
            get_app_state: std::sync::Arc::new(crate::tool::ToolAppState::default),
            set_app_state: std::sync::Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: Some(callback),
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: std::sync::Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: std::sync::Arc::new(
                allthecodes_types::commands::NoopCommandDispatcher::new(),
            ),
            available_tools: vec![],
            execute_deferred_tool: None,
            taint_context: Default::default(),
        };

        let parent = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };

        let result = tool
            .call(json!({"question": "Which project?"}), &ctx, &parent, None)
            .await
            .expect("callback ask user should succeed");

        assert_eq!(result.data, json!("answer for: Which project?"));
    }
}
