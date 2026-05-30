//! Conversation snipping product tool.

use std::collections::HashSet;

use allthecodes_types::message::{
    AssistantMessage, InfoLevel, Message, SystemMessage, SystemSubtype,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};

pub(super) struct SnipTool;

#[async_trait]
impl Tool for SnipTool {
    fn name(&self) -> &str {
        "Snip"
    }

    async fn description(&self, _input: &Value) -> String {
        "Record intent to snip selected conversation messages from future context.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message_ids": { "type": "array", "items": { "type": "string" } },
                "reason": { "type": "string" }
            },
            "required": ["message_ids"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let valid = input
            .get("message_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| !ids.is_empty() && ids.iter().all(Value::is_string));
        if !valid {
            return ValidationResult::Error {
                message: "'message_ids' must be a non-empty array of strings".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Snip {} message(s) from future context?",
                input
                    .get("message_ids")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let requested: HashSet<String> = input
            .get("message_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
        let matched = ctx
            .messages
            .iter()
            .filter(|message| requested.contains(&message.uuid().to_string()))
            .count();
        let summary = input
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Snipped messages")
            .to_string();
        let marker = Message::System(SystemMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            subtype: SystemSubtype::Informational {
                level: InfoLevel::Info,
            },
            content: format!(
                "Snip applied to {matched} message(s): {summary}. Future model context will omit the selected messages."
            ),
        });
        Ok(ToolResult {
            data: json!({
                "snipped_count": matched,
                "requested_count": requested.len(),
                "message_ids": requested.iter().cloned().collect::<Vec<_>>(),
                "summary": summary,
                "projection_applied": true,
            }),
            display_preview: Some(format!("Snip recorded for {matched} message(s)")),
            new_messages: vec![marker],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Mark selected conversation messages for snipping to reduce future context pressure. Include a concise reason that preserves important facts.".to_string()
    }
}
