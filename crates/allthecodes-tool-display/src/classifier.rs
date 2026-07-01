use serde_json::Value;

use crate::result_summary::summarize_result;
use crate::{
    OperationConfidence, OperationKind, OperationRisk, OperationStatus, OperationSubtype,
    ToolOperation,
};

/// The main classifier that maps a raw tool name, input, and optional result
/// into a `ToolOperation`.
#[derive(Debug, Clone, Default)]
pub struct ToolClassifier;

impl ToolClassifier {
    /// Classify a tool invocation into a semantic `ToolOperation`.
    ///
    /// * `tool_name` — the raw tool name (e.g. `"Bash"`, `"Read"`, `"FileEdit"`).
    /// * `input` — the tool input JSON value.
    /// * `status` — the current execution status.
    pub fn classify(tool_name: &str, input: &Value, status: OperationStatus) -> ToolOperation {
        let raw_input = input.clone();

        // Determine the operation kind and metadata.
        let (kind, subtype, risk, confidence, label, target, command_summary) =
            Self::classify_inner(tool_name, input);

        ToolOperation {
            kind,
            subtype,
            status,
            risk,
            confidence,
            label,
            target,
            command_summary,
            result_summary: None,
            raw_tool_name: tool_name.to_string(),
            raw_input,
            raw_output: None,
            side_channels: Vec::new(),
        }
    }

    /// Classify a permission request while preserving the requested operation's
    /// target, risk, and command summary.
    pub fn classify_permission(
        tool_name: &str,
        input: &Value,
        message: Option<&str>,
        status: OperationStatus,
    ) -> ToolOperation {
        let mut op = Self::classify(tool_name, input, status);
        let requested_label = message
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| op.label.clone());

        op.kind = OperationKind::Permission;
        op.label = format!("Permission: {}", requested_label);
        if op.command_summary.is_none() {
            op.command_summary = Some(requested_label);
        }
        op
    }

    /// Classify and attach a result summary.
    pub fn classify_with_result(
        tool_name: &str,
        input: &Value,
        status: OperationStatus,
        result_content: Option<&str>,
        is_error: bool,
    ) -> ToolOperation {
        let mut op = Self::classify(tool_name, input, status);
        op.result_summary = result_content.and_then(|content| {
            if content.is_empty() {
                None
            } else {
                Some(summarize_result(tool_name, content, is_error))
            }
        });
        op.raw_output = result_content.map(|content| Value::String(content.to_string()));
        op
    }

    /// Core classification logic.
    fn classify_inner(
        tool_name: &str,
        input: &Value,
    ) -> (
        OperationKind,
        Option<OperationSubtype>,
        OperationRisk,
        OperationConfidence,
        String,
        Option<String>,
        Option<String>,
    ) {
        match tool_name {
            // == Read operations ==
            "Read" | "read_file" | "read" => {
                let target = input
                    .get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = target
                    .as_deref()
                    .map_or("Read file".to_string(), |p| format!("Read {}", p));
                (
                    OperationKind::Read,
                    None,
                    OperationRisk::Safe,
                    OperationConfidence::High,
                    label,
                    target,
                    None,
                )
            }

            // == Search operations ==
            "Grep" | "grep" | "Glob" | "glob" | "WebSearch" | "web_search" => {
                let pattern = input
                    .get("pattern")
                    .or_else(|| input.get("query"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let path = input
                    .get("path")
                    .or_else(|| input.get("dir"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let target = pattern.clone().or(path.clone());
                let label = pattern.as_deref().map_or_else(
                    || format!("Search"),
                    |p| format!("Search \"{}\"", truncate_str(p, 64)),
                );
                (
                    OperationKind::Search,
                    None,
                    OperationRisk::Safe,
                    OperationConfidence::High,
                    label,
                    target,
                    None,
                )
            }

            // == Web operations ==
            "WebFetch" | "web_fetch" => {
                let url = input
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = url.as_deref().map_or("Fetch URL".to_string(), |u| {
                    format!("Fetch {}", truncate_str(u, 64))
                });
                (
                    OperationKind::Read,
                    None,
                    OperationRisk::Safe,
                    OperationConfidence::High,
                    label,
                    url,
                    None,
                )
            }

            // == File operations ==
            "Edit" | "FileEdit" | "edit_file" | "file_edit" | "MultiEdit" | "NotebookEdit" => {
                let file_path = input
                    .get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = file_path
                    .as_deref()
                    .map_or("Edit file".to_string(), |p| format!("Edit {}", p));
                (
                    OperationKind::Modify,
                    None,
                    OperationRisk::Medium,
                    OperationConfidence::High,
                    label,
                    file_path,
                    None,
                )
            }

            "Write" | "FileWrite" | "file_write" | "write_file" => {
                let file_path = input
                    .get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = file_path
                    .as_deref()
                    .map_or("Write file".to_string(), |p| format!("Write {}", p));
                (
                    OperationKind::Create,
                    None,
                    OperationRisk::Medium,
                    OperationConfidence::High,
                    label,
                    file_path,
                    None,
                )
            }

            // == Shell operations ==
            "Bash" | "bash" | "PowerShell" | "powershell" => {
                let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let shell_result = super::shell_heuristic::shell_command_operation(command);
                let label = if command.chars().count() > 80 {
                    truncate_str(command, 80)
                } else if command.is_empty() {
                    "Run command".to_string()
                } else {
                    command.to_string()
                };
                let cmd_summary = if command.chars().count() > 120 {
                    Some(truncate_str(command, 120))
                } else {
                    Some(command.to_string())
                };
                (
                    shell_result.kind,
                    shell_result.subtype,
                    shell_result.risk,
                    shell_result.confidence,
                    label,
                    shell_result.target,
                    cmd_summary,
                )
            }

            // == Permission ==
            "Permission" | "permission" => {
                let message = input
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = message
                    .as_deref()
                    .unwrap_or("Permission request")
                    .to_string();
                (
                    OperationKind::Permission,
                    None,
                    OperationRisk::Medium,
                    OperationConfidence::High,
                    label,
                    None,
                    message,
                )
            }

            // == Delegate (Agent / Task / SubAgent) ==
            "Agent" | "Task" | "SubAgent" | "subagent" => {
                let description = input
                    .get("description")
                    .or_else(|| input.get("prompt"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = description
                    .as_deref()
                    .map_or("Delegate task".to_string(), |d| truncate_str(d, 64));
                (
                    OperationKind::Delegate,
                    None,
                    OperationRisk::Medium,
                    OperationConfidence::High,
                    label,
                    None,
                    description,
                )
            }

            // == MCP tools ==
            server if server.contains("__") || server.contains(":") => {
                let parts: Vec<&str> = server.split("__").collect();
                let mcp_tool = parts.last().unwrap_or(&server);
                let label = format!("MCP: {}", mcp_tool);
                (
                    OperationKind::System,
                    Some(OperationSubtype::Mcp),
                    OperationRisk::Medium,
                    OperationConfidence::Medium,
                    label,
                    None,
                    None,
                )
            }

            // == Known tool names (catch common tools not handled above) ==
            "LS" | "List" | "ls" | "list" => {
                let target = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let label = target
                    .as_deref()
                    .map_or("List directory".to_string(), |p| format!("List {}", p));
                (
                    OperationKind::Read,
                    None,
                    OperationRisk::Safe,
                    OperationConfidence::High,
                    label,
                    target,
                    None,
                )
            }

            "TodoWrite" | "todo_write" => {
                let todos = input
                    .get("todos")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let label = if todos > 0 {
                    format!("Update TODO list ({} items)", todos)
                } else {
                    "Update TODO list".to_string()
                };
                (
                    OperationKind::Status,
                    Some(OperationSubtype::Todo),
                    OperationRisk::Safe,
                    OperationConfidence::High,
                    label,
                    None,
                    None,
                )
            }

            // == Unknown tool ==
            _ => {
                let label = tool_name.to_string();
                (
                    OperationKind::Unknown,
                    None,
                    OperationRisk::Low,
                    OperationConfidence::Low,
                    label,
                    None,
                    None,
                )
            }
        }
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_classify_read() {
        let op = ToolClassifier::classify(
            "Read",
            &json!({"file_path": "src/main.rs"}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Read);
        assert_eq!(op.risk, OperationRisk::Safe);
        assert_eq!(op.target.as_deref(), Some("src/main.rs"));
        assert_eq!(op.raw_tool_name, "Read");
    }

    #[test]
    fn test_classify_grep() {
        let op = ToolClassifier::classify(
            "Grep",
            &json!({"pattern": "fn main", "path": "src/"}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Search);
        assert!(op.label.contains("fn main"));
    }

    #[test]
    fn test_classify_edit() {
        let op = ToolClassifier::classify(
            "FileEdit",
            &json!({"file_path": "src/lib.rs"}),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Modify);
        assert_eq!(op.risk, OperationRisk::Medium);
    }

    #[test]
    fn test_classify_write() {
        let op = ToolClassifier::classify(
            "FileWrite",
            &json!({"file_path": "src/new.rs"}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Create);
    }

    #[test]
    fn test_classify_bash() {
        let op = ToolClassifier::classify(
            "Bash",
            &json!({"command": "cargo test"}),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Execute);
        assert_eq!(op.subtype, Some(OperationSubtype::Test));
    }

    #[test]
    fn test_classify_rm() {
        let op = ToolClassifier::classify(
            "Bash",
            &json!({"command": "rm -rf /tmp"}),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Delete);
        assert_eq!(op.risk, OperationRisk::Destructive);
    }

    #[test]
    fn test_classify_todo_write() {
        let op = ToolClassifier::classify(
            "TodoWrite",
            &json!({"todos": [{"content": "fix bug"}]}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Status);
        assert_eq!(op.subtype, Some(OperationSubtype::Todo));
    }

    #[test]
    fn test_classify_agent() {
        let op = ToolClassifier::classify(
            "Agent",
            &json!({"description": "Find the bug in auth.rs"}),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Delegate);
    }

    #[test]
    fn test_classify_unknown() {
        let op = ToolClassifier::classify("SomeUnknownTool", &json!({}), OperationStatus::Resolved);
        assert_eq!(op.kind, OperationKind::Unknown);
    }

    #[test]
    fn test_classify_mcp_tool() {
        let op = ToolClassifier::classify(
            "mcp__github__create_pr",
            &json!({}),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::System);
        assert_eq!(op.subtype, Some(OperationSubtype::Mcp));
    }

    #[test]
    fn test_web_fetch() {
        let op = ToolClassifier::classify(
            "WebFetch",
            &json!({"url": "https://example.com"}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Read);
    }

    #[test]
    fn test_classify_with_result() {
        let op = ToolClassifier::classify_with_result(
            "Bash",
            &json!({"command": "echo hello"}),
            OperationStatus::Resolved,
            Some("hello\n"),
            false,
        );
        assert!(op.result_summary.is_some());
        assert_eq!(op.result_summary.as_ref().unwrap().text, "hello");
        assert_eq!(op.raw_output, Some(serde_json::json!("hello\n")));
    }

    #[test]
    fn test_classify_permission_wraps_requested_operation() {
        let op = ToolClassifier::classify_permission(
            "Bash",
            &json!({"command": "rm -rf target"}),
            Some("Delete target"),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Permission);
        assert_eq!(op.risk, OperationRisk::Destructive);
        assert_eq!(op.target.as_deref(), Some("target"));
        assert_eq!(op.label, "Permission: Delete target");
        assert_eq!(op.raw_tool_name, "Bash");
    }

    #[test]
    fn test_classify_long_unicode_command_does_not_panic() {
        let command = format!("echo {}", "路径".repeat(80));
        let op = ToolClassifier::classify(
            "Bash",
            &json!({ "command": command }),
            OperationStatus::InProgress,
        );
        assert_eq!(op.kind, OperationKind::Execute);
        assert!(op.label.ends_with('…'));
    }

    #[test]
    fn test_classify_shell_simple() {
        let op = ToolClassifier::classify(
            "Bash",
            &json!({"command": "ls -la"}),
            OperationStatus::Resolved,
        );
        assert_eq!(op.kind, OperationKind::Execute);
        assert_eq!(op.subtype, Some(OperationSubtype::Shell));
    }

    #[test]
    fn test_classify_multi_type() {
        let op = ToolClassifier::classify(
            "Edit",
            &json!({"file_path": "a.rs"}),
            OperationStatus::Error,
        );
        assert_eq!(op.kind, OperationKind::Modify);
        assert_eq!(op.status, OperationStatus::Error);
    }
}
