use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{
    FileStateReceipt, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

use super::safe_write::{
    safe_write_text, validate_write_request, SafeWriteOptions, DEFAULT_MAX_WRITE_BYTES,
};

/// FileWriteTool — Write content to a file
///
/// Corresponds to TypeScript: tools/FileWriteTool
pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        FileWriteTool
    }

    fn parse_input(input: &Value) -> (String, String) {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (file_path, content)
    }
}

impl Default for FileWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    async fn description(&self, _input: &Value) -> String {
        "Writes a file to the local filesystem.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        input
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
        json!({
            "file_path": file_path,
            "content_bytes": content.len(),
            "content_lines": content.lines().count(),
            "operation": "write_file"
        })
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        crate::interaction::observable_input::backfill_file_path(input);
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if file_path.is_empty() {
            return ValidationResult::Error {
                message: "file_path is required".to_string(),
                error_code: 1,
            };
        }
        if input.get("content").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::Error {
                message: "content is required".to_string(),
                error_code: 1,
            };
        }
        let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if let Err(e) = validate_write_request(
            Path::new(file_path),
            content.as_bytes(),
            DEFAULT_MAX_WRITE_BYTES,
        ) {
            return ValidationResult::Error {
                message: e.to_string(),
                error_code: 1,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let (file_path, content) = Self::parse_input(&input);

        if file_path.is_empty() {
            return Ok(ToolResult {
                data: json!({ "error": "file_path is required" }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let file_path_for_write = file_path.clone();
        let content_for_write = content.clone();
        let safe_options = SafeWriteOptions {
            session_id: Some(ctx.session_id.clone()),
            ..Default::default()
        };

        let report = match tokio::task::spawn_blocking(move || {
            safe_write_text(file_path_for_write, &content_for_write, &safe_options)
        })
        .await
        {
            Ok(Ok(report)) => report,
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to write file safely: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Safe write task failed: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        let line_count = report.line_count;
        let byte_count = report.bytes_written;
        let file_state_receipt = FileStateReceipt::from_content(
            &ctx.cwd,
            &file_path,
            &report.target_path,
            content.as_bytes(),
        );

        // Fire FileChanged hook
        {
            let app_state = (ctx.get_app_state)();
            let configs =
                allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "FileChanged");
            if !configs.is_empty() {
                let payload = json!({
                    "file_path": &file_path,
                    "operation": "write",
                    "byte_count": byte_count,
                    "line_count": line_count,
                    "safe_write": {
                        "atomic": true,
                        "backup_path": report.backup_path.as_ref().map(|p| p.display().to_string()),
                    },
                });
                let _ = crate::hooks::run_event_hooks("FileChanged", &payload, &configs).await;
            }
        }

        Ok(ToolResult {
            data: json!({
                "output": format!(
                    "Successfully wrote {} bytes ({} lines) to {}",
                    byte_count, line_count, file_path
                ),
                "path": file_path.clone(),
                "safe_write": {
                    "atomic": true,
                    "created": report.created,
                    "backup_path": report.backup_path.as_ref().map(|p| p.display().to_string()),
                    "bytes": report.bytes_written,
                    "line_count": report.line_count,
                    "permissions_preserved": report.permissions_preserved,
                    "symlink_resolved": report.symlink_resolved,
                    "requested_path": report.requested_path.display().to_string(),
                    "target_path": report.target_path.display().to_string(),
                },
            }),
            new_messages: vec![super::edited_text_file_message(file_path)],
            file_state_receipts: vec![file_state_receipt],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Writes a file to the local filesystem.\n\n\
Usage:\n\
- This tool will overwrite the existing file if there is one at the provided path.\n\
- If this is an existing file, you MUST use the Read tool first to read the file's contents. This tool will fail if you did not read the file first.\n\
- Prefer the Edit tool for modifying existing files \u{2014} it only sends the diff. Only use this tool to create new files or for complete rewrites.\n\
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User.\n\
- Only use emojis if the user explicitly requests it. Avoid writing emojis to files unless asked.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Write".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::tool::ToolAppState as AppState;
    use crate::tool::{FileStateCache, ToolUseOptions};
    use allthecodes_types::message::ContentBlock;
    use serde_json::json;
    use uuid::Uuid;

    fn test_context() -> ToolUseContext {
        let app_state = AppState::default();
        let (_tx, rx) = tokio::sync::watch::channel(false);

        ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || app_state.clone()),
            set_app_state: Arc::new(|_| {}),
            session_id: "file-write-test-session".to_string(),
            langfuse_session_id: "file-write-test-session".to_string(),
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
            taint_context: Default::default(),
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
    fn test_name() {
        assert_eq!(FileWriteTool::new().name(), "Write");
    }

    #[test]
    fn test_schema_has_required_fields() {
        let schema = FileWriteTool::new().input_json_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("file_path").is_some());
        assert!(props.get("content").is_some());
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("file_path")));
        assert!(required.contains(&json!("content")));
    }

    #[test]
    fn test_parse_input_full() {
        let input = json!({"file_path": "/tmp/test.txt", "content": "hello"});
        let (path, content) = FileWriteTool::parse_input(&input);
        assert_eq!(path, "/tmp/test.txt");
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_parse_input_missing() {
        let input = json!({});
        let (path, content) = FileWriteTool::parse_input(&input);
        assert_eq!(path, "");
        assert_eq!(content, "");
    }

    #[test]
    fn test_is_destructive() {
        let tool = FileWriteTool::new();
        assert!(tool.is_destructive(&json!({})));
        assert!(!tool.is_read_only(&json!({})));
        assert!(!tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_get_path() {
        let tool = FileWriteTool::new();
        assert_eq!(
            tool.get_path(&json!({"file_path": "/a/b.rs"})),
            Some("/a/b.rs".to_string())
        );
        assert_eq!(tool.get_path(&json!({})), None);
    }

    #[tokio::test]
    async fn test_write_and_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("output.txt");
        let content = "line1\nline2\nline3";

        // Test the actual write via tokio::fs (same as what call() uses)
        tokio::fs::write(&file_path, content).await.unwrap();
        let read_back = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(read_back, content);
        assert_eq!(content.lines().count(), 3);
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let dir = tempfile::TempDir::new().unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("test.txt");

        // Simulate what call() does
        if let Some(parent) = nested.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&nested, "hello").await.unwrap();
        assert!(nested.exists());
    }

    #[tokio::test]
    async fn test_call_returns_backward_compatible_result_with_safe_write_diagnostics() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("output.txt");
        let input = json!({
            "file_path": file_path.display().to_string(),
            "content": "line1\nline2",
        });

        let result = FileWriteTool::new()
            .call(input, &test_context(), &parent_message(), None)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "line1\nline2"
        );
        let expected_path = file_path.display().to_string();
        assert_eq!(
            result.data.get("path").and_then(|v| v.as_str()),
            Some(expected_path.as_str())
        );
        assert!(result
            .data
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap()
            .contains("Successfully wrote"));

        let safe_write = result
            .data
            .get("safe_write")
            .expect("safe_write diagnostics");
        assert_eq!(
            safe_write.get("atomic").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            safe_write.get("created").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(safe_write.get("bytes").and_then(|v| v.as_u64()), Some(11));
        assert!(matches!(
            result.new_messages.as_slice(),
            [allthecodes_types::message::Message::Attachment(attachment)]
                if matches!(
                    &attachment.attachment,
                    allthecodes_types::message::Attachment::EditedTextFile { path }
                        if path == &expected_path
                )
        ));
    }

    #[tokio::test]
    async fn test_validate_input_rejects_binary_and_oversized_content() {
        let tool = FileWriteTool::new();
        let ctx = test_context();

        let binary = tool
            .validate_input(
                &json!({"file_path": "/tmp/binary.txt", "content": "abc\0def"}),
                &ctx,
            )
            .await;
        assert!(matches!(binary, ValidationResult::Error { .. }));

        let oversized = tool
            .validate_input(
                &json!({
                    "file_path": "/tmp/large.txt",
                    "content": "x".repeat(DEFAULT_MAX_WRITE_BYTES + 1),
                }),
                &ctx,
            )
            .await;
        assert!(matches!(oversized, ValidationResult::Error { .. }));
    }
}
