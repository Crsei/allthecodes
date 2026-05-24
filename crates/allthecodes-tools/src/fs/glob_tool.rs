use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;

/// GlobTool — Find files matching glob patterns
///
/// Corresponds to TypeScript: tools/GlobTool
pub struct GlobTool;

#[derive(Debug, Clone)]
struct GlobMatch {
    path: String,
    modified: Option<SystemTime>,
}

impl GlobTool {
    pub fn new() -> Self {
        GlobTool
    }

    fn parse_input(input: &Value) -> (String, Option<String>) {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        (pattern, path)
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    async fn description(&self, _input: &Value) -> String {
        "Fast file pattern matching tool that works with any codebase size.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in. Defaults to the current working directory."
                }
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

    fn get_path(&self, input: &Value) -> Option<String> {
        input
            .get("path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        if pattern.is_empty() {
            return ValidationResult::Error {
                message: "pattern is required".to_string(),
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
        let (pattern, path) = Self::parse_input(&input);

        if pattern.is_empty() {
            return Ok(ToolResult {
                data: json!({ "error": "pattern is required" }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Determine base directory
        let base_dir = match &path {
            Some(p) => PathBuf::from(p),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };

        if !base_dir.exists() {
            return Ok(ToolResult {
                data: json!({ "error": format!("Directory not found: {}", base_dir.display()) }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Build the full glob pattern
        let full_pattern = if pattern.starts_with('/') || pattern.contains(':') {
            // Absolute pattern
            pattern.clone()
        } else {
            // Relative to base_dir
            let base = base_dir.to_string_lossy();
            // Normalize path separators for glob crate
            let base = base.replace('\\', "/");
            if base.ends_with('/') {
                format!("{}{}", base, pattern)
            } else {
                format!("{}/{}", base, pattern)
            }
        };

        // Use the glob crate to find matching files
        // Run in blocking task since glob is synchronous
        let matches = tokio::task::spawn_blocking(move || {
            let mut results: Vec<GlobMatch> = Vec::new();
            match glob::glob(&full_pattern) {
                Ok(paths) => {
                    for entry in paths {
                        match entry {
                            Ok(path) => {
                                let modified = path
                                    .metadata()
                                    .and_then(|metadata| metadata.modified())
                                    .ok();
                                results.push(GlobMatch {
                                    path: path.to_string_lossy().to_string(),
                                    modified,
                                });
                            }
                            Err(_) => continue,
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Invalid glob pattern: {}", e));
                }
            }
            Ok(results)
        })
        .await?;

        match matches {
            Ok(mut files) => {
                sort_matches_by_modified(&mut files);
                let files: Vec<String> = files.into_iter().map(|entry| entry.path).collect();

                let count = files.len();
                let output = if files.is_empty() {
                    "No matching files found.".to_string()
                } else {
                    files.join("\n")
                };

                // Truncate if too large
                let max_chars = self.max_result_size_chars();
                let output = if output.len() > max_chars {
                    let mut truncated = output;
                    truncated.truncate(max_chars);
                    truncated.push_str("\n... (output truncated)");
                    truncated
                } else {
                    output
                };

                Ok(ToolResult {
                    data: json!({
                        "output": output,
                        "count": count,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            Err(e) => Ok(ToolResult {
                data: json!({ "error": e }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        "- Fast file pattern matching tool that works with any codebase size\n\
- Supports glob patterns like \"**/*.js\" or \"src/**/*.ts\"\n\
- Returns matching file paths sorted by modification time\n\
- Use this tool when you need to find files by name patterns\n\
- When you are doing an open ended search that may require multiple rounds of globbing and grepping, use the Agent tool instead".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Glob".to_string()
    }
}

fn sort_matches_by_modified(files: &mut [GlobMatch]) {
    files.sort_by(|a, b| {
        let modified_order = match (a.modified, b.modified) {
            (Some(a_modified), Some(b_modified)) => b_modified.cmp(&a_modified),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        modified_order.then_with(|| a.path.cmp(&b.path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn test_name() {
        assert_eq!(GlobTool::new().name(), "Glob");
    }

    #[test]
    fn test_parse_input_full() {
        let input = json!({"pattern": "**/*.rs", "path": "/src"});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "**/*.rs");
        assert_eq!(path, Some("/src".to_string()));
    }

    #[test]
    fn test_parse_input_pattern_only() {
        let input = json!({"pattern": "*.txt"});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "*.txt");
        assert!(path.is_none());
    }

    #[test]
    fn test_parse_input_empty() {
        let input = json!({});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "");
        assert!(path.is_none());
    }

    #[test]
    fn test_parse_input_empty_path_treated_as_none() {
        let input = json!({"pattern": "*.rs", "path": ""});
        let (pattern, path) = GlobTool::parse_input(&input);
        assert_eq!(pattern, "*.rs");
        assert!(
            path.is_none(),
            "empty string path should be treated as None"
        );
    }

    #[test]
    fn test_get_path_empty_string_treated_as_none() {
        let tool = GlobTool::new();
        assert_eq!(tool.get_path(&json!({"path": ""})), None);
    }

    #[test]
    fn test_is_read_only_and_concurrent() {
        let tool = GlobTool::new();
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_schema_requires_pattern() {
        let schema = GlobTool::new().input_json_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("pattern")));
    }

    #[test]
    fn test_get_path() {
        let tool = GlobTool::new();
        assert_eq!(
            tool.get_path(&json!({"path": "/src"})),
            Some("/src".to_string())
        );
        assert_eq!(tool.get_path(&json!({})), None);
    }

    #[test]
    fn test_sort_matches_by_modified_descending_with_path_tie_breaker() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut files = vec![
            GlobMatch {
                path: "older.rs".to_string(),
                modified: Some(base),
            },
            GlobMatch {
                path: "z_tie.rs".to_string(),
                modified: Some(base + Duration::from_secs(10)),
            },
            GlobMatch {
                path: "missing_modified.rs".to_string(),
                modified: None,
            },
            GlobMatch {
                path: "a_tie.rs".to_string(),
                modified: Some(base + Duration::from_secs(10)),
            },
        ];

        sort_matches_by_modified(&mut files);
        let paths: Vec<_> = files.into_iter().map(|entry| entry.path).collect();

        assert_eq!(
            paths,
            vec!["a_tie.rs", "z_tie.rs", "older.rs", "missing_modified.rs"]
        );
    }

    #[tokio::test]
    async fn test_glob_finds_files() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        std::fs::write(dir.path().join("c.txt"), "").unwrap();

        let pattern = format!("{}/*.rs", dir.path().to_string_lossy().replace('\\', "/"));
        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            for entry in glob::glob(&pattern).unwrap() {
                results.push(entry.unwrap().to_string_lossy().to_string());
            }
            results
        })
        .await
        .unwrap();

        assert_eq!(matches.len(), 2);
    }

    #[tokio::test]
    async fn test_glob_no_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let pattern = format!("{}/*.xyz", dir.path().to_string_lossy().replace('\\', "/"));
        let matches = tokio::task::spawn_blocking(move || {
            let mut results = Vec::new();
            for entry in glob::glob(&pattern).unwrap() {
                results.push(entry.unwrap());
            }
            results
        })
        .await
        .unwrap();

        assert!(matches.is_empty());
    }
}
