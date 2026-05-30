//! Context inspection product tool.

use std::collections::HashMap;

use allthecodes_types::message::AssistantMessage;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext};

use super::common::{message_kind, message_text, truncate_chars};

pub(super) struct CtxInspectTool;

#[async_trait]
impl Tool for CtxInspectTool {
    fn name(&self) -> &str {
        "CtxInspect"
    }

    async fn description(&self, _input: &Value) -> String {
        "Inspect the current conversation context and estimated token usage.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Optional text filter." }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
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
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let app_state = (ctx.get_app_state)();
        let total_tokens = allthecodes_utils::messages::estimate_total_tokens(&ctx.messages);
        let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
        let mut matches = Vec::new();

        for message in &ctx.messages {
            *by_kind.entry(message_kind(message)).or_default() += 1;
            if let Some(query) = query.as_deref() {
                let text = message_text(message);
                if text.to_ascii_lowercase().contains(query) {
                    matches.push(json!({
                        "uuid": message.uuid().to_string(),
                        "kind": message_kind(message),
                        "timestamp": message.timestamp(),
                        "preview": truncate_chars(&text, 240),
                    }));
                }
            }
        }

        let prompt_caching_enabled = !app_state.main_loop_model.starts_with("openai/")
            && !app_state.main_loop_model.starts_with("grok/")
            && !app_state.main_loop_model.starts_with("gemini/");
        let summary = format!(
            "Model context: {}\nEstimated tokens: {total_tokens}\nMessages: {}",
            app_state.main_loop_model,
            ctx.messages.len()
        );
        Ok(ToolResult {
            data: json!({
                "total_tokens": total_tokens,
                "message_count": ctx.messages.len(),
                "by_kind": by_kind,
                "context_window_model": app_state.main_loop_model,
                "prompt_caching_enabled": prompt_caching_enabled,
                "session_memory_enabled": !app_state.surfaced_memory_keys.is_empty(),
                "context_collapse_enabled": true,
                "summary": summary,
                "matches": matches,
            }),
            display_preview: Some(format!(
                "Context: {total_tokens} tokens, {} messages",
                ctx.messages.len()
            )),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Inspect current context usage, message counts, and optional text matches before deciding whether to snip or compact history.".to_string()
    }
}
