//! Tool-use summary generation — produces concise summaries of tool usage
//! in a conversation turn.
//!
//! This is a LOCAL computation (no API call). It takes a list of tool usages
//! and formats them into a human-readable one-liner, similar to a git commit
//! message.

/// Information about a single tool invocation.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool name (e.g. "Bash", "FileRead", "Grep").
    pub name: String,
    /// Truncated JSON representation of the tool input.
    pub input_summary: String,
    /// Truncated JSON representation of the tool output.
    pub output_summary: String,
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        // Ensure we don't split a multi-byte character
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Derive a short action phrase from a tool name.
fn action_for_tool(name: &str) -> &str {
    match name {
        "Bash" => "run a command",
        "FileRead" | "Read" => "read a file",
        "FileWrite" | "Write" => "write a file",
        "FileEdit" | "Edit" => "edit a file",
        "Grep" => "search file contents",
        "Glob" => "find files",
        "Agent" | "Skill" => "invoke a sub-agent",
        "AskUser" => "ask the user",
        "WebSearch" => "search the web",
        "WebFetch" => "fetch a URL",
        _ => "use a tool",
    }
}

/// Generate a one-line summary of tool usage (similar to a git commit message).
///
/// Returns `None` if the tools list is empty.
pub fn generate_tool_use_summary(
    tools: &[ToolInfo],
    last_assistant_text: Option<&str>,
) -> Option<String> {
    if tools.is_empty() {
        return None;
    }

    let parts: Vec<String> = tools
        .iter()
        .map(|t| {
            let input = truncate_str(&t.input_summary, 100);
            let output = truncate_str(&t.output_summary, 60);
            format!(
                "{} to {} ({}) → {}",
                t.name,
                action_for_tool(&t.name),
                input,
                output
            )
        })
        .collect();

    let mut summary = format!("Used {}", parts.join(", "));

    if let Some(text) = last_assistant_text {
        let snippet = truncate_str(text.trim(), 80);
        if !snippet.is_empty() {
            summary.push_str(&format!(" — {}", snippet));
        }
    }

    Some(summary)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tools_returns_none() {
        assert!(generate_tool_use_summary(&[], None).is_none());
    }

    #[test]
    fn single_tool_generates_summary() {
        let tools = vec![ToolInfo {
            name: "Bash".to_string(),
            input_summary: "ls -la".to_string(),
            output_summary: "file list".to_string(),
        }];
        let result = generate_tool_use_summary(&tools, None).unwrap();
        assert!(result.contains("Bash"));
        assert!(result.contains("run a command"));
    }

    #[test]
    fn multiple_tools_comma_separated() {
        let tools = vec![
            ToolInfo {
                name: "Grep".to_string(),
                input_summary: "pattern: foo".to_string(),
                output_summary: "3 matches".to_string(),
            },
            ToolInfo {
                name: "Edit".to_string(),
                input_summary: "file: main.rs".to_string(),
                output_summary: "ok".to_string(),
            },
        ];
        let result = generate_tool_use_summary(&tools, None).unwrap();
        assert!(result.contains("Grep"));
        assert!(result.contains("Edit"));
        assert!(result.contains(", "));
    }

    #[test]
    fn includes_assistant_text_snippet() {
        let tools = vec![ToolInfo {
            name: "Read".to_string(),
            input_summary: "file: lib.rs".to_string(),
            output_summary: "contents".to_string(),
        }];
        let result = generate_tool_use_summary(&tools, Some("I found the bug in line 42")).unwrap();
        assert!(result.contains("I found the bug"));
    }

    #[test]
    fn output_summary_included_in_result() {
        let tools = vec![ToolInfo {
            name: "Bash".to_string(),
            input_summary: "echo hello".to_string(),
            output_summary: "hello".to_string(),
        }];
        let result = generate_tool_use_summary(&tools, None).unwrap();
        // The output should appear after the "→" arrow
        assert!(
            result.contains("→"),
            "summary should contain arrow: {}",
            result
        );
        assert!(
            result.contains("hello"),
            "summary should contain output: {}",
            result
        );
    }

    #[test]
    fn output_summary_long_gets_truncated() {
        let tools = vec![ToolInfo {
            name: "Read".to_string(),
            input_summary: "main.rs".to_string(),
            output_summary: "x".repeat(200),
        }];
        let result = generate_tool_use_summary(&tools, None).unwrap();
        assert!(
            result.contains("..."),
            "long output should be truncated: {}",
            result
        );
    }

    #[test]
    fn all_tool_names_have_actions() {
        let tool_names = [
            "Bash",
            "FileRead",
            "Read",
            "FileWrite",
            "Write",
            "FileEdit",
            "Edit",
            "Grep",
            "Glob",
            "Agent",
            "Skill",
            "AskUser",
            "WebSearch",
            "WebFetch",
        ];
        for name in tool_names {
            let action = action_for_tool(name);
            assert_ne!(
                action, "use a tool",
                "{} should have a specific action",
                name
            );
        }
    }

    #[test]
    fn unknown_tool_gets_generic_action() {
        assert_eq!(action_for_tool("SomeUnknownTool"), "use a tool");
    }

    #[test]
    fn truncate_str_short_string_unchanged() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_long_string_truncated() {
        let long = "a".repeat(200);
        let result = truncate_str(&long, 50);
        assert!(result.ends_with("..."));
        // 50 chars + "..." = 53
        assert_eq!(result.len(), 53);
    }

    #[test]
    fn truncate_str_respects_char_boundaries() {
        // "你好世界" — each char is 3 bytes in UTF-8
        let s = "你好世界";
        let result = truncate_str(s, 6); // 6 bytes = exactly 2 chars
        assert_eq!(result, "你好...");
    }
}
