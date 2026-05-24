//! Rust-side helper for assistant tool-use messages.
//!
//! Renders tool-use status with a state machine covering queued, in-progress,
//! resolved, error, waiting-for-permission, and classifier-checking states.
//! Integrates with `ToolActivity` for structured in-progress/resolved rendering.

use crate::ui::theme::Theme;
use crate::ui::tool_activity::{ToolActivity, ToolState};
use serde_json::Value;

/// Tool-use lifecycle states used by the renderer.
///
/// Mirrors the TS state calculation in `AssistantToolUseMessage.tsx:95-97`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolUseState {
    /// Tool is waiting in the execution queue.
    Queued,
    /// Tool is currently executing.
    InProgress,
    /// Tool execution completed successfully.
    Resolved,
    /// Tool execution resulted in an error.
    Error,
    /// Waiting for user permission to execute.
    WaitingForPermission,
    /// Content classifier is reviewing the tool use.
    ClassifierChecking,
}

/// Tools that wrap their content transparently (shell/file-editing tools).
///
/// These are considered "transparent wrappers" — in the TS reference
/// (`AssistantToolUseMessage.tsx:99-114`), they render differently when
/// the real content follows immediately.
const TRANSPARENT_WRAPPER_TOOLS: &[&str] = &[
    "Bash",
    "PowerShell",
    "Write",
    "Edit",
    "NotebookEdit",
    "Read",
];

/// Check whether a tool name is a transparent wrapper.
fn is_transparent_wrapper_tool(tool_name: &str) -> bool {
    TRANSPARENT_WRAPPER_TOOLS.contains(&tool_name)
}

/// Render a tool that is queued (waiting its turn).
///
/// TS reference: `AssistantToolUseMessage.tsx:138-140` (queued dot)
fn render_tool_use_queued_message(display_name: &str) -> String {
    format!("  ● {display_name}")
}

/// Render a tool that is currently executing with progress.
///
/// TS reference: `AssistantToolUseMessage.tsx:221-268`
fn render_tool_use_progress_message(
    display_name: &str,
    input_summary: &str,
    has_hook_progress: bool,
) -> String {
    let summary = if input_summary.is_empty() {
        display_name.to_string()
    } else {
        format!("{display_name}({input_summary})")
    };
    if has_hook_progress {
        format!("  ● {summary} [hook running]")
    } else {
        format!("  ● {summary}")
    }
}

/// Render a tool that encountered an error.
fn render_tool_use_error_state(display_name: &str, input_summary: &str) -> String {
    let summary = if input_summary.is_empty() {
        display_name.to_string()
    } else {
        format!("{display_name}({input_summary})")
    };
    format!("  ● {summary} [error]")
}

/// Render a tool that is being checked by the content classifier.
///
/// TS reference: `AssistantToolUseMessage.tsx:173-178`
fn render_classifier_checking(display_name: &str) -> String {
    format!("  ● {display_name} (classifier checking...)")
}

/// Render a tool waiting for user permission.
///
/// TS reference: `AssistantToolUseMessage.tsx:179-182`
fn render_waiting_for_permission(display_name: &str) -> String {
    format!("  ● {display_name} (waiting for permission...)")
}

fn render_shell_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
    has_hook_progress: bool,
) -> Option<String> {
    if !matches!(tool_name, "Bash" | "PowerShell") {
        return None;
    }

    let call = shell_tool_call_summary(tool_name, input)?;
    let title = match state {
        ToolUseState::Queued => "Queued",
        ToolUseState::InProgress => "Running",
        ToolUseState::Resolved => "Ran",
        ToolUseState::Error => "Failed [error]",
        ToolUseState::WaitingForPermission => "Needs permission",
        ToolUseState::ClassifierChecking => "Checking",
    };
    let hook_suffix = if has_hook_progress {
        " [hook running]"
    } else {
        ""
    };
    Some(format!("  ● {title}{hook_suffix}\n   ⎿  {call}"))
}

fn render_file_edit_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
    has_hook_progress: bool,
) -> Option<String> {
    if !matches!(
        tool_name,
        "Edit" | "Write" | "FileEdit" | "FileWrite" | "NotebookEdit" | "MultiEdit"
    ) {
        return None;
    }

    let value = serde_json::from_str::<Value>(input.trim()).ok()?;
    let object = value.as_object()?;
    let path = ["file_path", "path", "notebook_path"]
        .iter()
        .find_map(|key| object.get(*key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let path = format_tool_arg(path);
    let title = match state {
        ToolUseState::Queued => "Queued edit",
        ToolUseState::InProgress => "Editing",
        ToolUseState::Resolved => "Edited",
        ToolUseState::Error => "Edit failed [error]",
        ToolUseState::WaitingForPermission => "Edit needs permission",
        ToolUseState::ClassifierChecking => "Checking edit",
    };
    let hook_suffix = if has_hook_progress {
        " [hook running]"
    } else {
        ""
    };
    Some(format!("  ● {title}{hook_suffix}\n   ⎿  Edit(path={path})"))
}

fn render_todo_write_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
) -> Option<String> {
    if !matches!(tool_name, "TodoWrite" | "todo_write") {
        return None;
    }

    let value = serde_json::from_str::<Value>(input.trim()).ok()?;
    let todos = value.get("todos").and_then(Value::as_array)?;
    let title = match state {
        ToolUseState::Queued => "Queued todo update",
        ToolUseState::InProgress => "Updating todos",
        ToolUseState::Resolved => "Updated todos",
        ToolUseState::Error => "Todo update failed [error]",
        ToolUseState::WaitingForPermission => "Todo update needs permission",
        ToolUseState::ClassifierChecking => "Checking todo update",
    };

    let mut lines = vec![format!("  ● {title}")];
    if todos.is_empty() {
        lines.push("   ⎿  (empty todo list)".to_string());
    } else {
        for todo in todos.iter().take(12) {
            let status = todo
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending");
            let content = todo
                .get("activeForm")
                .and_then(Value::as_str)
                .filter(|value| status == "in_progress" && !value.trim().is_empty())
                .or_else(|| todo.get("content").and_then(Value::as_str))
                .unwrap_or("(untitled todo)");
            lines.push(format!(
                "   ⎿  {} {}",
                todo_status_marker(status),
                format_tool_arg(content.trim())
            ));
        }
        if todos.len() > 12 {
            lines.push(format!("   ⎿  ... {} more", todos.len() - 12));
        }
    }
    Some(lines.join("\n"))
}

fn todo_status_marker(status: &str) -> &'static str {
    match status {
        "completed" => "[x]",
        "in_progress" => "[*]",
        _ => "[ ]",
    }
}

fn shell_tool_call_summary(tool_name: &str, input: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(input.trim()).ok()?;
    let object = value.as_object()?;
    let mut parts = Vec::new();

    if let Some(description) = object
        .get("description")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format_tool_arg(description));
    }

    if let Some(command) = object
        .get("command")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format_tool_arg(command));
    }

    (!parts.is_empty()).then(|| format!("{tool_name}({})", parts.join(", ")))
}

fn format_tool_arg(value: &str) -> String {
    const MAX_CHARS: usize = 96;
    let single_line = value.replace('\n', "\\n");
    if single_line.chars().count() <= MAX_CHARS {
        single_line
    } else {
        format!(
            "{}...",
            single_line
                .chars()
                .take(MAX_CHARS.saturating_sub(3))
                .collect::<String>()
        )
    }
}

/// Render the main tool-use message for a given state.
///
/// `tool_name`: the name of the tool (e.g. "Bash", "Read", "Edit")
/// `input`: raw JSON input string for the tool call
/// `state`: current lifecycle state of the tool use
/// `has_hook_progress`: whether a hook is currently running for this tool
///
/// Returns a string representation, possibly with continuation lines.
pub fn render_assistant_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
    has_hook_progress: bool,
    _theme: &Theme,
) -> String {
    if let Some(rendered) = render_todo_write_tool_use_message(tool_name, input, state) {
        return rendered;
    }

    if let Some(rendered) =
        render_shell_tool_use_message(tool_name, input, state, has_hook_progress)
    {
        return rendered;
    }

    if let Some(rendered) =
        render_file_edit_tool_use_message(tool_name, input, state, has_hook_progress)
    {
        return rendered;
    }

    // Build summary from the tool input.
    let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Running);
    let display_name = activity
        .user_facing_name
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(tool_name);
    let input_summary = activity.display_call();
    // The display_call includes the tool name, so strip it for summary-only.
    let summary_only = input_summary
        .strip_prefix(&format!(
            "{}(",
            activity.user_facing_name.as_deref().unwrap_or(tool_name)
        ))
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or("");

    match state {
        ToolUseState::Queued => render_tool_use_queued_message(display_name),
        ToolUseState::InProgress => {
            render_tool_use_progress_message(display_name, summary_only, has_hook_progress)
        }
        ToolUseState::Resolved => {
            if is_transparent_wrapper_tool(tool_name) {
                // Keep an explicit completion marker unless the main render path
                // replaces this helper with a richer tool-result row.
                format!("  ● {display_name}")
            } else {
                let summary = if summary_only.is_empty() {
                    display_name.to_string()
                } else {
                    format!("{display_name}({summary_only})")
                };
                format!("  ● {summary}")
            }
        }
        ToolUseState::Error => render_tool_use_error_state(display_name, summary_only),
        ToolUseState::WaitingForPermission => render_waiting_for_permission(display_name),
        ToolUseState::ClassifierChecking => render_classifier_checking(display_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_state_renders_dot_and_name() {
        let result = render_assistant_tool_use_message(
            "Read",
            "",
            ToolUseState::Queued,
            false,
            &Theme::default(),
        );
        assert_eq!(result, "  ● Read");
    }

    #[test]
    fn in_progress_shows_tool_and_input() {
        let result = render_assistant_tool_use_message(
            "Read",
            r#"{"file_path":"src/main.rs"}"#,
            ToolUseState::InProgress,
            false,
            &Theme::default(),
        );
        assert!(result.contains("●"));
        assert!(result.contains("Read"));
        assert!(result.contains("src/main.rs"));
    }

    #[test]
    fn in_progress_with_hook_shows_hook_indicator() {
        let result = render_assistant_tool_use_message(
            "Bash",
            r#"{"command":"cargo build"}"#,
            ToolUseState::InProgress,
            true,
            &Theme::default(),
        );
        assert!(result.contains("[hook running]"));
    }

    #[test]
    fn bash_renders_ran_block_with_description_before_command() {
        let result = render_assistant_tool_use_message(
            "Bash",
            r#"{"command":"cargo test","description":"Run Rust tests"}"#,
            ToolUseState::Resolved,
            false,
            &Theme::default(),
        );
        assert_eq!(result, "  ● Ran\n   ⎿  Bash(Run Rust tests, cargo test)");
    }

    #[test]
    fn write_renders_as_edit_call_block() {
        let result = render_assistant_tool_use_message(
            "Write",
            r#"{"file_path":"src/lib.rs","content":"fn main() {}"}"#,
            ToolUseState::Resolved,
            false,
            &Theme::default(),
        );

        assert_eq!(result, "  ● Edited\n   ⎿  Edit(path=src/lib.rs)");
    }

    #[test]
    fn todo_write_renders_checklist_content() {
        let result = render_assistant_tool_use_message(
            "TodoWrite",
            r#"{"todos":[{"content":"Inspect UI","status":"completed"},{"content":"Patch rendering","status":"in_progress","activeForm":"Patching rendering"},{"content":"Run tests","status":"pending"}]}"#,
            ToolUseState::Resolved,
            false,
            &Theme::default(),
        );

        assert!(result.contains("Updated todos"));
        assert!(result.contains("[x] Inspect UI"));
        assert!(result.contains("[*] Patching rendering"));
        assert!(result.contains("[ ] Run tests"));
    }

    #[test]
    fn resolved_transparent_wrapper_keeps_completion_marker() {
        let result = render_assistant_tool_use_message(
            "Bash",
            r#"{"command":"ls"}"#,
            ToolUseState::Resolved,
            false,
            &Theme::default(),
        );
        assert_eq!(result, "  ● Ran\n   ⎿  Bash(ls)");
    }

    #[test]
    fn resolved_non_transparent_shows_summary() {
        let result = render_assistant_tool_use_message(
            "Grep",
            r#"{"pattern":"fn main"}"#,
            ToolUseState::Resolved,
            false,
            &Theme::default(),
        );
        assert!(result.contains("●"));
    }

    #[test]
    fn error_state_shows_error_indicator() {
        let result = render_assistant_tool_use_message(
            "Bash",
            r#"{"command":"invalid"}"#,
            ToolUseState::Error,
            false,
            &Theme::default(),
        );
        assert!(result.contains("[error]"));
    }

    #[test]
    fn waiting_for_permission_shows_prompt() {
        let result = render_assistant_tool_use_message(
            "Bash",
            "",
            ToolUseState::WaitingForPermission,
            false,
            &Theme::default(),
        );
        assert!(result.contains("waiting for permission"));
    }

    #[test]
    fn classifier_checking_shows_classifier() {
        let result = render_assistant_tool_use_message(
            "Bash",
            "",
            ToolUseState::ClassifierChecking,
            false,
            &Theme::default(),
        );
        assert!(result.contains("classifier checking"));
    }
}
