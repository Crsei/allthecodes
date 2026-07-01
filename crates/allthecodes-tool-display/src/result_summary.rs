use serde_json::Value;

use crate::OperationResultSummary;

/// Strategy for unwrapping and summarising JSON values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonUnwrapStrategy {
    /// Prefer top-level scalar or short text fields.
    Short,
    /// Include structured details (array counts, nested keys).
    Detailed,
}

/// Attempt to extract meaningful text from a JSON value by checking common
/// wrapping keys (`content`, `output`, `text`, `error`, `message`, `data`).
pub fn json_unwrap_text(value: &Value) -> Option<String> {
    // Direct string → return it trimmed
    if let Some(s) = value.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    // Object → check known keys
    if let Some(obj) = value.as_object() {
        for key in &["content", "output", "text", "error", "message", "data"] {
            if let Some(val) = obj.get(*key) {
                if let Some(s) = val.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
                // Recursively unwrap
                if let Some(unwrapped) = json_unwrap_text(val) {
                    return Some(unwrapped);
                }
            }
        }
        // Fallback: pick the first string value
        for val in obj.values() {
            if let Some(s) = val.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    // Array → count
    if let Some(arr) = value.as_array() {
        if !arr.is_empty() {
            return Some(format!("[{} items]", arr.len()));
        }
    }

    None
}

/// Summarise a tool result into an `OperationResultSummary`.
///
/// This handles different tool types differently:
/// - Shell commands: look at exit code, stdout/stderr.
/// - File reads: count lines.
/// - Search results: count files and matches.
/// - Edits: count lines added/removed.
/// - Generic: unwrap JSON and produce a text summary.
pub fn summarize_result(
    tool_name: &str,
    result_content: &str,
    is_error: bool,
) -> OperationResultSummary {
    match tool_name {
        "Bash" | "bash" | "PowerShell" | "powershell" => {
            summarize_shell_result(result_content, is_error)
        }
        "Read" | "read_file" | "read" => summarize_read_result(result_content),
        "Grep" | "grep" => summarize_grep_result(result_content),
        "Glob" | "glob" => summarize_glob_result(result_content),
        "Edit" | "FileEdit" | "edit_file" | "file_edit" | "MultiEdit" | "NotebookEdit"
        | "Write" | "FileWrite" | "file_write" | "write_file" => {
            summarize_edit_result(result_content)
        }
        _ => summarize_generic_result(result_content, is_error),
    }
}

fn summarize_shell_result(content: &str, is_error: bool) -> OperationResultSummary {
    if let Some(summary) = summarize_structured_shell_result(content, is_error) {
        return summary;
    }

    let lines: Vec<&str> = content.lines().collect();
    let output_lines = lines.len();
    let truncated = output_lines > 20;

    let text = if is_error {
        format!("tool error: {}", truncate_text(content, 120))
    } else if output_lines == 0 {
        "done".to_string()
    } else if output_lines <= 5 {
        content.trim().to_string()
    } else {
        truncate_text(content, 120)
    };

    OperationResultSummary {
        text,
        output_lines,
        exit_code: None,
        has_stderr: false,
        truncated,
        file_lines: None,
        search_file_count: None,
        search_match_count: None,
        lines_added: None,
        lines_removed: None,
    }
}

fn summarize_structured_shell_result(
    content: &str,
    is_error: bool,
) -> Option<OperationResultSummary> {
    let value = serde_json::from_str::<Value>(content).ok()?;
    let obj = value.as_object()?;

    let exit_code = obj
        .get("exit_code")
        .or_else(|| obj.get("exitCode"))
        .or_else(|| obj.get("code"))
        .and_then(|value| value.as_i64())
        .and_then(|code| i32::try_from(code).ok());
    let stdout = obj
        .get("stdout")
        .or_else(|| obj.get("output"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let stderr = obj
        .get("stderr")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let error = obj
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    let has_stderr = !stderr.trim().is_empty();
    let output_lines =
        stdout.lines().count() + stderr.lines().count() + usize::from(!error.is_empty());
    let primary = if has_stderr {
        stderr
    } else if !stdout.trim().is_empty() {
        stdout
    } else {
        error
    };
    let text = match exit_code {
        Some(code) if code != 0 => format!("exit {code}: {}", truncate_text(primary, 120)),
        Some(_) if has_stderr => format!("stderr: {}", truncate_text(stderr, 120)),
        Some(_) if !primary.trim().is_empty() => truncate_text(primary, 120),
        Some(_) => "done".to_string(),
        None if is_error && !primary.trim().is_empty() => {
            format!("tool error: {}", truncate_text(primary, 120))
        }
        None if !primary.trim().is_empty() => truncate_text(primary, 120),
        None => "done".to_string(),
    };

    Some(OperationResultSummary {
        text,
        output_lines,
        exit_code,
        has_stderr,
        truncated: primary.len() > 120,
        file_lines: None,
        search_file_count: None,
        search_match_count: None,
        lines_added: None,
        lines_removed: None,
    })
}

fn summarize_read_result(content: &str) -> OperationResultSummary {
    let line_count = content.lines().filter(|_l| true).count();
    let truncated = line_count > 100;
    let text = if truncated {
        format!("{} lines (truncated)", line_count)
    } else if line_count <= 3 {
        content.trim().to_string()
    } else {
        let first_line = content.lines().next().unwrap_or("");
        if first_line
            .chars()
            .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace() || c == '/')
        {
            if line_count <= 10 {
                content.trim().to_string()
            } else {
                format!("{} lines", line_count)
            }
        } else {
            format!("{} lines", line_count)
        }
    };

    OperationResultSummary {
        text,
        output_lines: line_count,
        exit_code: None,
        has_stderr: false,
        truncated,
        file_lines: Some(line_count),
        search_file_count: None,
        search_match_count: None,
        lines_added: None,
        lines_removed: None,
    }
}

fn summarize_grep_result(content: &str) -> OperationResultSummary {
    let line_count = content.lines().filter(|l| !l.is_empty()).count();
    let text = match line_count {
        0 => "no matches".to_string(),
        _ => format!("{} matches", line_count),
    };

    OperationResultSummary {
        text,
        output_lines: line_count,
        exit_code: None,
        has_stderr: false,
        truncated: false,
        file_lines: None,
        search_file_count: None,
        search_match_count: Some(line_count),
        lines_added: None,
        lines_removed: None,
    }
}

fn summarize_glob_result(content: &str) -> OperationResultSummary {
    let paths: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    let text = match paths.len() {
        0 => "no files found".to_string(),
        1 => format!("1 file: {}", paths[0]),
        n => format!("{} files", n),
    };

    OperationResultSummary {
        text,
        output_lines: paths.len(),
        exit_code: None,
        has_stderr: false,
        truncated: false,
        file_lines: None,
        search_file_count: Some(paths.len()),
        search_match_count: None,
        lines_added: None,
        lines_removed: None,
    }
}

fn summarize_edit_result(content: &str) -> OperationResultSummary {
    let line_count = content.lines().count();
    let text = if line_count <= 10 {
        content.trim().to_string()
    } else {
        format!("{} lines changed", line_count)
    };

    OperationResultSummary {
        text,
        output_lines: line_count,
        exit_code: None,
        has_stderr: false,
        truncated: false,
        file_lines: None,
        search_file_count: None,
        search_match_count: None,
        lines_added: Some(count_added_lines(content)),
        lines_removed: Some(count_removed_lines(content)),
    }
}

fn summarize_generic_result(content: &str, is_error: bool) -> OperationResultSummary {
    let text = if content.trim().is_empty() {
        "done".to_string()
    } else if is_error {
        format!("error: {}", truncate_text(content, 120))
    } else {
        truncate_text(content, 120)
    };

    OperationResultSummary {
        text,
        output_lines: content.lines().count(),
        exit_code: None,
        has_stderr: false,
        truncated: content.len() > 120,
        file_lines: None,
        search_file_count: None,
        search_match_count: None,
        lines_added: None,
        lines_removed: None,
    }
}

fn truncate_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = String::with_capacity(max + 3);
        for c in s.chars() {
            if truncated.len() + c.len_utf8() > max {
                break;
            }
            truncated.push(c);
        }
        truncated.push('…');
        truncated
    }
}

fn count_added_lines(content: &str) -> usize {
    content.lines().filter(|l| l.starts_with('+')).count()
}

fn count_removed_lines(content: &str) -> usize {
    content.lines().filter(|l| l.starts_with('-')).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_unwrap_text_direct_string() {
        assert_eq!(
            json_unwrap_text(&json!("hello world")),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn test_json_unwrap_text_content_key() {
        let v = json!({"content": "result value", "extra": "ignored"});
        assert_eq!(json_unwrap_text(&v), Some("result value".to_string()));
    }

    #[test]
    fn test_json_unwrap_text_nested() {
        let v = json!({"data": {"content": "nested content"}});
        assert_eq!(json_unwrap_text(&v), Some("nested content".to_string()));
    }

    #[test]
    fn test_json_unwrap_text_array() {
        let v = json!([1, 2, 3]);
        assert_eq!(json_unwrap_text(&v), Some("[3 items]".to_string()));
    }

    #[test]
    fn test_json_unwrap_text_empty() {
        assert_eq!(json_unwrap_text(&json!(null)), None);
        assert_eq!(json_unwrap_text(&json!("")), None);
    }

    #[test]
    fn test_summarize_shell_result() {
        let summary = summarize_result("Bash", "hello\nworld\n", false);
        assert_eq!(summary.text, "hello\nworld");
        assert_eq!(summary.exit_code, None);
        assert!(!summary.truncated);
    }

    #[test]
    fn test_summarize_shell_error() {
        let summary = summarize_result("Bash", "permission denied", true);
        assert!(summary.text.contains("permission denied"));
        assert!(!summary.has_stderr);
        assert_eq!(summary.exit_code, None);
    }

    #[test]
    fn test_summarize_shell_structured_exit_and_stderr() {
        let summary = summarize_result(
            "Bash",
            r#"{"exit_code":2,"stdout":"partial output\n","stderr":"permission denied\n"}"#,
            true,
        );
        assert_eq!(summary.exit_code, Some(2));
        assert!(summary.has_stderr);
        assert!(summary.text.contains("exit 2"));
        assert!(summary.text.contains("permission denied"));
    }

    #[test]
    fn test_summarize_read_result() {
        let summary = summarize_result("Read", "line1\nline2\nline3\n", false);
        assert_eq!(summary.text, "line1\nline2\nline3");
        assert_eq!(summary.file_lines, Some(3));
    }

    #[test]
    fn test_summarize_grep_result() {
        let summary = summarize_result("Grep", "match1\nmatch2\n", false);
        assert_eq!(summary.search_match_count, Some(2));
    }

    #[test]
    fn test_summarize_glob_result() {
        let summary = summarize_result("Glob", "src/main.rs\nsrc/lib.rs\n", false);
        assert_eq!(summary.text, "2 files");
        assert_eq!(summary.search_file_count, Some(2));
    }

    #[test]
    fn test_summarize_edit_result() {
        let summary = summarize_result("Edit", "+added\n-removed\n", false);
        assert_eq!(summary.lines_added, Some(1));
        assert_eq!(summary.lines_removed, Some(1));
    }

    #[test]
    fn test_summarize_generic_empty() {
        let summary = summarize_result("SomeTool", "", false);
        assert_eq!(summary.text, "done");
    }
}
