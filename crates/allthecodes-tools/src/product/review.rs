//! Artifact review product tool.

use allthecodes_types::message::AssistantMessage;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};

pub(super) struct ReviewArtifactTool;

#[async_trait]
impl Tool for ReviewArtifactTool {
    fn name(&self) -> &str {
        "ReviewArtifact"
    }

    async fn description(&self, input: &Value) -> String {
        input
            .get("title")
            .and_then(Value::as_str)
            .map(|title| format!("Review artifact: {title}"))
            .unwrap_or_else(|| "Review an artifact with structured annotations.".to_string())
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact": { "type": "string" },
                "title": { "type": "string" },
                "annotations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "line": { "type": "integer", "minimum": 1 },
                            "message": { "type": "string" },
                            "severity": {
                                "type": "string",
                                "enum": ["info", "warning", "error", "suggestion"]
                            }
                        },
                        "required": ["message"]
                    }
                },
                "summary": { "type": "string" }
            },
            "required": ["artifact", "annotations"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("artifact")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
            || !input.get("annotations").is_some_and(Value::is_array)
        {
            return ValidationResult::Error {
                message: "'artifact' and array 'annotations' are required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let annotations = input
            .get("annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(ToolResult {
            data: json!({
                "artifact": input.get("artifact").cloned().unwrap_or(Value::Null),
                "title": input.get("title").cloned(),
                "annotations": annotations,
                "annotationCount": annotations.len(),
                "summary": input.get("summary").cloned(),
            }),
            display_preview: Some(format!(
                "Review complete: {} annotation(s)",
                annotations.len()
            )),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Present a structured review of a code snippet, document, or generated artifact with inline annotations and an optional summary.".to_string()
    }
}
