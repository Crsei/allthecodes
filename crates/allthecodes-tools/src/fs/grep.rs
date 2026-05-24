use anyhow::{Context, Result};
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::Command;

use crate::tool::*;
use allthecodes_types::message::AssistantMessage;

pub struct GrepTool;

#[derive(Deserialize)]
struct GrepInput {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    #[serde(rename = "type")]
    file_type: Option<String>,
    output_mode: Option<String>,
    #[serde(rename = "-C")]
    context: Option<usize>,
    #[serde(rename = "-A")]
    after_context: Option<usize>,
    #[serde(rename = "-B")]
    before_context: Option<usize>,
    #[serde(rename = "-i")]
    case_insensitive: Option<bool>,
    #[serde(rename = "-n")]
    line_numbers: Option<bool>,
    head_limit: Option<usize>,
    multiline: Option<bool>,
    offset: Option<usize>,
}

/// Attempt to run the search via the external `rg` (ripgrep) binary.
///
/// Returns `Some(output)` on success, `None` if `rg` is not found or
/// the invocation fails for any reason.
async fn try_ripgrep(params: &GrepInput, search_path: &str) -> Option<String> {
    // Check if rg is available
    if Command::new("rg").arg("--version").output().is_err() {
        return None;
    }

    let output_mode = params
        .output_mode
        .as_deref()
        .unwrap_or("files_with_matches");

    let mut cmd = Command::new("rg");
    cmd.arg("--no-heading");

    // Pattern
    cmd.arg("-e").arg(&params.pattern);

    // Glob filter
    if let Some(ref g) = params.glob {
        cmd.arg("--glob").arg(g);
    }

    // File type
    if let Some(ref ft) = params.file_type {
        cmd.arg("--type").arg(ft);
    }

    // Case insensitive
    if params.case_insensitive.unwrap_or(false) {
        cmd.arg("-i");
    }

    // Multiline
    if params.multiline.unwrap_or(false) {
        cmd.arg("-U").arg("--multiline-dotall");
    }

    // Output mode
    match output_mode {
        "files_with_matches" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {
            // "content" mode
            if params.line_numbers.unwrap_or(true) {
                cmd.arg("-n");
            }
        }
    }

    // Context flags (only meaningful for content mode)
    if output_mode == "content" || output_mode != "files_with_matches" && output_mode != "count" {
        if let Some(c) = params.context {
            cmd.arg("-C").arg(c.to_string());
        } else {
            if let Some(a) = params.after_context {
                cmd.arg("-A").arg(a.to_string());
            }
            if let Some(b) = params.before_context {
                cmd.arg("-B").arg(b.to_string());
            }
        }
    }

    // max-count: use head_limit as a rough upper bound on matches per file
    // (rg --max-count is per-file, so this is an approximation; we do precise
    // truncation after collecting output)
    // We intentionally do NOT pass --max-count here because it is per-file
    // and we need global limits applied in post-processing.

    // Search path
    cmd.arg(search_path);

    let output = cmd.output().ok()?;

    // rg returns exit code 1 when no matches are found — that is not an error
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Some(stdout)
}

/// Apply offset (skip first N lines) and head_limit (keep at most N lines)
/// to a newline-separated output string from ripgrep.
fn apply_offset_and_limit(output: &str, offset: usize, head_limit: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let after_offset: Vec<&str> = lines.into_iter().skip(offset).collect();
    let limited = if head_limit > 0 && after_offset.len() > head_limit {
        &after_offset[..head_limit]
    } else {
        &after_offset[..]
    };
    limited.join("\n")
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search file contents with regex patterns.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "File or directory to search" },
                "glob": { "type": "string", "description": "Glob pattern to filter files" },
                "type": { "type": "string", "description": "File type (js, py, rs, etc.)" },
                "output_mode": { "type": "string", "enum": ["content", "files_with_matches", "count"] },
                "-C": { "type": "number", "description": "Context lines before and after" },
                "-A": { "type": "number", "description": "Lines after each match" },
                "-B": { "type": "number", "description": "Lines before each match" },
                "-i": { "type": "boolean", "description": "Case insensitive" },
                "-n": { "type": "boolean", "description": "Show line numbers" },
                "head_limit": { "type": "number", "description": "Limit output entries" },
                "multiline": { "type": "boolean", "description": "Enable multiline mode where . matches newlines and patterns can span lines" },
                "offset": { "type": "number", "description": "Skip first N entries before applying head_limit" }
            },
            "required": ["pattern"]
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
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: GrepInput = serde_json::from_value(input)?;
        let search_path = params
            .path
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let head_limit = params.head_limit.unwrap_or(250);
        let offset = params.offset.unwrap_or(0);

        // --- Try external ripgrep first ---
        if let Some(rg_output) = try_ripgrep(&params, &search_path).await {
            let output = apply_offset_and_limit(&rg_output, offset, head_limit);
            let output = if output.is_empty() {
                "No matches found.".to_string()
            } else {
                output
            };
            return Ok(ToolResult {
                data: json!(output),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // --- Fallback: internal regex + ignore walker ---
        let output_mode = params
            .output_mode
            .as_deref()
            .unwrap_or("files_with_matches");
        let case_insensitive = params.case_insensitive.unwrap_or(false);
        let _context_lines = params.context.or(params.after_context).unwrap_or(0);

        let pattern_str = if case_insensitive {
            format!("(?i){}", params.pattern)
        } else {
            params.pattern.clone()
        };
        let re = Regex::new(&pattern_str).context("Invalid regex pattern")?;

        let mut walker = WalkBuilder::new(&search_path);
        walker.hidden(false).git_ignore(true);

        if let Some(ref glob_pat) = params.glob {
            let mut types_builder = ignore::types::TypesBuilder::new();
            let glob_ext = glob_pat.trim_start_matches("*.");
            // Ignore errors from add() — if the pattern is invalid, we'll fall through
            // to the manual glob matching below.
            let _ = types_builder.add("custom", &format!("*.{}", glob_ext));
            types_builder.select("custom");
            if let Ok(types) = types_builder.build() {
                walker.types(types);
            }
        }

        let mut results: Vec<String> = Vec::new();
        let mut _file_count = 0;
        let mut _match_count = 0;

        for entry in walker.build().flatten() {
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();

            if let Some(ref glob_pat) = params.glob {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !glob::Pattern::new(glob_pat).map_or(true, |p| p.matches(name)) {
                    continue;
                }
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue, // skip binary/unreadable
            };

            let lines: Vec<&str> = content.lines().collect();
            let mut file_matches = Vec::new();

            for (i, line) in lines.iter().enumerate() {
                if re.is_match(line) {
                    file_matches.push((i + 1, *line));
                    _match_count += 1;
                }
            }

            if !file_matches.is_empty() {
                _file_count += 1;
                let path_str = path.display().to_string();

                match output_mode {
                    "files_with_matches" => {
                        results.push(path_str);
                    }
                    "count" => {
                        results.push(format!("{}:{}", path_str, file_matches.len()));
                    }
                    _ => {
                        for (line_num, line_content) in &file_matches {
                            let show_num = params.line_numbers.unwrap_or(true);
                            if show_num {
                                results.push(format!("{}:{}:{}", path_str, line_num, line_content));
                            } else {
                                results.push(format!("{}:{}", path_str, line_content));
                            }
                        }
                    }
                }
            }
        }

        // Apply offset then head_limit
        if offset > 0 {
            results = results.into_iter().skip(offset).collect();
        }
        if head_limit > 0 && results.len() > head_limit {
            results.truncate(head_limit);
        }

        let output = if results.is_empty() {
            "No matches found.".to_string()
        } else {
            results.join("\n")
        };

        Ok(ToolResult {
            data: json!(output),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "A powerful search tool built on ripgrep\n\n\
  Usage:\n\
  - ALWAYS use Grep for search tasks. NEVER invoke `grep` or `rg` as a Bash command. The Grep tool has been optimized for correct permissions and access.\n\
  - Supports full regex syntax (e.g., \"log.*Error\", \"function\\\\s+\\\\w+\")\n\
  - Filter files with glob parameter (e.g., \"*.js\", \"**/*.tsx\") or type parameter (e.g., \"js\", \"py\", \"rust\")\n\
  - Output modes: \"content\" shows matching lines, \"files_with_matches\" shows only file paths (default), \"count\" shows match counts\n\
  - Use Agent tool for open-ended searches requiring multiple rounds\n\
  - Pattern syntax: Uses ripgrep (not grep) - literal braces need escaping (use `interface\\\\{\\\\}` to find `interface{}` in Go code)\n\
  - Multiline matching: By default patterns match within single lines only. For cross-line patterns like `struct \\\\{[\\\\s\\\\S]*?field`, use `multiline: true`\n".to_string()
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        json!(format!("grep {}", pattern))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grep_input_multiline_field() {
        let json_input = json!({
            "pattern": "foo.*bar",
            "multiline": true,
            "offset": 5
        });
        let input: GrepInput = serde_json::from_value(json_input).unwrap();
        assert_eq!(input.multiline, Some(true));
        assert_eq!(input.offset, Some(5));
    }

    #[test]
    fn test_grep_input_multiline_default() {
        let json_input = json!({
            "pattern": "hello"
        });
        let input: GrepInput = serde_json::from_value(json_input).unwrap();
        assert_eq!(input.multiline, None);
        assert_eq!(input.offset, None);
    }

    #[test]
    fn test_grep_schema_has_multiline() {
        let tool = GrepTool;
        let schema = tool.input_json_schema();
        let props = schema.get("properties").unwrap();
        assert!(
            props.get("multiline").is_some(),
            "schema must include 'multiline' property"
        );
        assert!(
            props.get("offset").is_some(),
            "schema must include 'offset' property"
        );

        let ml = props.get("multiline").unwrap();
        assert_eq!(ml.get("type").unwrap(), "boolean");

        let off = props.get("offset").unwrap();
        assert_eq!(off.get("type").unwrap(), "number");
    }

    #[test]
    fn test_empty_path_treated_as_cwd() {
        let json_input = json!({
            "pattern": "hello",
            "path": ""
        });
        let input: GrepInput = serde_json::from_value(json_input).unwrap();
        let search_path = input
            .path
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ".".to_string());
        assert_eq!(search_path, ".", "empty path should fall back to cwd");
    }

    #[test]
    fn test_apply_offset_and_limit() {
        let output = "line1\nline2\nline3\nline4\nline5";
        assert_eq!(
            apply_offset_and_limit(output, 0, 250),
            "line1\nline2\nline3\nline4\nline5"
        );
        assert_eq!(
            apply_offset_and_limit(output, 2, 250),
            "line3\nline4\nline5"
        );
        assert_eq!(apply_offset_and_limit(output, 0, 3), "line1\nline2\nline3");
        assert_eq!(apply_offset_and_limit(output, 1, 2), "line2\nline3");
        assert_eq!(apply_offset_and_limit(output, 10, 250), "");
    }
}
