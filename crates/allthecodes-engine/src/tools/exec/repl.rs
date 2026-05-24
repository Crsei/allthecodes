//! REPL tool -- execute code snippets in various languages.
//!
//! Maps a language name to an interpreter, writes the code to a temporary file,
//! and executes it with a timeout.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

use allthecodes_engine::types::tool::{
    InterruptBehavior, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};
use allthecodes_tools::exec::repl as repl_spec;
use allthecodes_types::message::AssistantMessage;
use allthecodes_utils::bash::resolve_timeout;

/// ReplTool -- execute code snippets in supported languages.
pub struct ReplTool;

impl ReplTool {
    fn parse_input(input: &Value) -> (String, String, u64) {
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let code = input
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);
        (language, code, timeout_ms)
    }
}

#[async_trait]
impl Tool for ReplTool {
    fn name(&self) -> &str {
        repl_spec::NAME
    }

    async fn description(&self, input: &Value) -> String {
        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("code");
        repl_spec::description(Some(language))
    }

    fn input_json_schema(&self) -> Value {
        repl_spec::input_schema()
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false // Executes arbitrary code
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let (language, code, _) = Self::parse_input(input);

        if language.is_empty() {
            return ValidationResult::Error {
                message: "\"language\" must not be empty".to_string(),
                error_code: 1,
            };
        }
        if code.is_empty() {
            return ValidationResult::Error {
                message: "\"code\" must not be empty".to_string(),
                error_code: 1,
            };
        }

        if repl_spec::language_spec(language.as_str()).is_none() {
            return ValidationResult::Error {
                message: format!(
                    "Unsupported language \"{}\". Supported: {}",
                    language,
                    repl_spec::supported_language_names().join(", ")
                ),
                error_code: 1,
            };
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
        let (language, code, timeout_ms) = Self::parse_input(&input);

        let (interpreter, ext) = match repl_spec::language_spec(language.as_str()) {
            Some(pair) => pair,
            None => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Unsupported language: {}", language) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        // Write code to a temporary file
        let temp_dir = std::env::temp_dir();
        let file_name = format!("cc_rust_repl_{}{}", uuid::Uuid::new_v4().simple(), ext);
        let temp_file = temp_dir.join(&file_name);

        if let Err(e) = std::fs::write(&temp_file, &code) {
            return Ok(ToolResult {
                data: json!({ "error": format!("Failed to write temp file: {}", e) }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Ensure cleanup
        let _cleanup = TempFileGuard(temp_file.clone());

        debug!(
            interpreter = interpreter,
            file = %temp_file.display(),
            "REPL: executing code"
        );

        let mut cmd = tokio::process::Command::new(interpreter);
        cmd.arg(temp_file.as_os_str());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let timeout_duration = resolve_timeout(Some(timeout_ms));

        let result = tokio::time::timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                let max_chars = self.max_result_size_chars();
                combined = allthecodes_tools::exec::truncate_output(&combined, max_chars);

                Ok(ToolResult {
                    data: json!({
                        "language": language,
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                        "output": combined,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                data: json!({ "error": format!("Failed to execute {}: {}", interpreter, e) }),
                new_messages: vec![],
                ..Default::default()
            }),
            Err(_) => Ok(ToolResult {
                data: json!({ "error": format!("Execution timed out after {}ms", timeout_duration.as_millis()) }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        repl_spec::prompt()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        repl_spec::NAME.to_string()
    }
}

/// Guard that deletes a temporary file when dropped.
struct TempFileGuard(std::path::PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_tool_name() {
        let tool = ReplTool;
        assert_eq!(tool.name(), "REPL");
    }

    #[test]
    fn test_repl_schema() {
        let tool = ReplTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("language"));
        assert!(props.contains_key("code"));
        assert!(props.contains_key("timeout"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("language")));
        assert!(required.contains(&json!("code")));
    }

    #[test]
    fn test_repl_parse_input_defaults() {
        let input = json!({ "language": "python", "code": "print(1)" });
        let (lang, code, timeout) = ReplTool::parse_input(&input);
        assert_eq!(lang, "python");
        assert_eq!(code, "print(1)");
        assert_eq!(timeout, 120_000);
    }

    #[test]
    fn test_repl_parse_input_custom_timeout() {
        let input = json!({ "language": "node", "code": "console.log(1)", "timeout": 5000 });
        let (lang, code, timeout) = ReplTool::parse_input(&input);
        assert_eq!(lang, "node");
        assert_eq!(code, "console.log(1)");
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn test_repl_not_read_only() {
        let tool = ReplTool;
        assert!(!tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_repl_is_destructive() {
        let tool = ReplTool;
        assert!(tool.is_destructive(&json!({})));
    }

    #[test]
    fn test_language_map_has_expected_entries() {
        let languages = repl_spec::supported_language_names();
        assert!(languages.contains(&"python"));
        assert!(languages.contains(&"python3"));
        assert!(languages.contains(&"node"));
        assert!(languages.contains(&"javascript"));
        assert!(languages.contains(&"ruby"));
    }
}
