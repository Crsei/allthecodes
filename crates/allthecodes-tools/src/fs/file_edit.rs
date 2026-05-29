use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use similar::TextDiff;

use crate::tool::{
    FileCacheEntry, FileStateCache, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_types::message::{AssistantMessage, ToolResultContent};

use super::safe_write::{safe_write_text, SafeWriteOptions};

/// FileEditTool — Edit a file by replacing exact string matches
///
/// Corresponds to TypeScript: tools/FileEditTool
pub struct FileEditTool;

const FILE_NOT_READ_ERROR: &str = "File has not been read yet. Read it first before writing to it.";
const FILE_UNEXPECTEDLY_MODIFIED_ERROR: &str =
    "File has been unexpectedly modified. Read it again before attempting to write it.";
const MAX_EDIT_FILE_BYTES: usize = 1024 * 1024 * 1024;

struct IndentationAdjustedEdit {
    old_string: String,
    new_string: String,
}

impl FileEditTool {
    pub fn new() -> Self {
        FileEditTool
    }

    fn parse_input(input: &Value) -> (String, String, String, bool) {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        (file_path, old_string, new_string, replace_all)
    }

    fn modified_millis(metadata: &std::fs::Metadata) -> i64 {
        metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0)
    }

    fn state_keys(file_path: &str, path: &Path) -> Vec<String> {
        let mut keys = Vec::new();
        Self::push_unique_key(&mut keys, file_path.to_string());
        Self::push_unique_key(&mut keys, path.to_string_lossy().to_string());
        if let Ok(canonical) = std::fs::canonicalize(path) {
            Self::push_unique_key(&mut keys, canonical.to_string_lossy().to_string());
        }
        keys
    }

    fn push_unique_key(keys: &mut Vec<String>, key: String) {
        if !key.is_empty() && !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }

    fn cached_entry(ctx: &ToolUseContext, file_path: &str, path: &Path) -> Option<FileCacheEntry> {
        Self::state_keys(file_path, path)
            .into_iter()
            .find_map(|key| ctx.read_file_state.get(&key))
    }

    fn validate_cached_read(
        ctx: &ToolUseContext,
        file_path: &str,
        path: &Path,
        content: &str,
    ) -> std::result::Result<(), &'static str> {
        let Some(entry) = Self::cached_entry(ctx, file_path, path) else {
            return Err(FILE_NOT_READ_ERROR);
        };

        let current_hash = FileStateCache::hash_content(content.as_bytes());
        if current_hash != entry.content_hash {
            return Err(FILE_UNEXPECTEDLY_MODIFIED_ERROR);
        }

        Ok(())
    }

    fn validate_file_writable(path: &Path) -> std::result::Result<(), String> {
        let metadata = std::fs::metadata(path)
            .map_err(|err| format!("Failed to stat file before editing: {}", err))?;
        if metadata.permissions().readonly() {
            return Err(format!(
                "File is readonly or locked and cannot be edited: {}",
                path.display()
            ));
        }

        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(|_| ())
            .map_err(|err| {
                if Self::is_lock_or_permission_error(&err) {
                    format!(
                        "File is locked or not writable and cannot be edited: {} ({})",
                        path.display(),
                        err
                    )
                } else {
                    format!("Failed to open file for editing: {}", err)
                }
            })
    }

    fn is_lock_or_permission_error(err: &io::Error) -> bool {
        matches!(
            err.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
        ) || matches!(err.raw_os_error(), Some(5 | 32 | 33))
    }

    fn record_edit_state(ctx: &ToolUseContext, file_path: &str, path: &Path, content: &str) {
        let timestamp = std::fs::metadata(path)
            .map(|metadata| Self::modified_millis(&metadata))
            .unwrap_or(0);
        let entry = FileCacheEntry {
            content_hash: FileStateCache::hash_content(content.as_bytes()),
            last_read_timestamp: timestamp,
        };
        for key in Self::state_keys(file_path, path) {
            ctx.read_file_state.insert(key, entry.clone());
        }
    }

    fn leading_indent(line: &str) -> &str {
        let end = line
            .char_indices()
            .find_map(|(idx, ch)| (!matches!(ch, ' ' | '\t')).then_some(idx))
            .unwrap_or(line.len());
        &line[..end]
    }

    fn trim_leading_indent(line: &str) -> &str {
        &line[Self::leading_indent(line).len()..]
    }

    fn find_indentation_adjusted_edit(
        content: &str,
        old_string: &str,
        new_string: &str,
    ) -> Option<IndentationAdjustedEdit> {
        let old_lines: Vec<&str> = old_string.lines().collect();
        if old_lines.is_empty() {
            return None;
        }
        let old_trimmed: Vec<&str> = old_lines
            .iter()
            .map(|line| Self::trim_leading_indent(line))
            .collect();
        let content_lines: Vec<&str> = content.lines().collect();
        if content_lines.len() < old_lines.len() {
            return None;
        }

        let mut matched_window: Option<&[&str]> = None;
        for window in content_lines.windows(old_lines.len()) {
            let same_without_indent = window
                .iter()
                .map(|line| Self::trim_leading_indent(line))
                .eq(old_trimmed.iter().copied());
            if same_without_indent {
                if matched_window.is_some() {
                    return None;
                }
                matched_window = Some(window);
            }
        }

        let actual_lines = matched_window?;
        let mut indent_map: Vec<(&str, &str)> = Vec::new();
        for (old_line, actual_line) in old_lines.iter().zip(actual_lines.iter()) {
            if Self::trim_leading_indent(old_line).is_empty() {
                continue;
            }
            let old_indent = Self::leading_indent(old_line);
            let actual_indent = Self::leading_indent(actual_line);
            if let Some((_, mapped_actual)) = indent_map
                .iter()
                .find(|(mapped_old, _)| *mapped_old == old_indent)
            {
                if *mapped_actual != actual_indent {
                    return None;
                }
            } else {
                indent_map.push((old_indent, actual_indent));
            }
        }
        indent_map.sort_by_key(|(old_indent, _)| std::cmp::Reverse(old_indent.len()));

        let adjusted_new_lines: Vec<String> = new_string
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    return String::new();
                }
                for (old_indent, actual_indent) in &indent_map {
                    if let Some(stripped) = line.strip_prefix(old_indent) {
                        return format!("{}{}", actual_indent, stripped);
                    }
                }
                line.to_string()
            })
            .collect();

        let actual_old = actual_lines.join("\n");
        let adjusted_new = adjusted_new_lines.join("\n");
        if actual_old == old_string && adjusted_new == new_string {
            return None;
        }

        Some(IndentationAdjustedEdit {
            old_string: actual_old,
            new_string: adjusted_new,
        })
    }

    fn unified_hunk_lines(path: &str, old_content: &str, new_content: &str) -> Vec<String> {
        TextDiff::from_lines(old_content, new_content)
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Find the best fuzzy match for `old_string` within `content` using
    /// a sliding window of lines and `similar::TextDiff` for scoring.
    ///
    /// Returns `Some((matched_text, start_line, end_line, similarity_ratio))`
    /// where lines are 1-indexed, or `None` if content is empty.
    fn find_best_fuzzy_match(content: &str, old_string: &str) -> Option<FuzzyMatch> {
        let content_lines: Vec<&str> = content.lines().collect();
        let needle_lines: Vec<&str> = old_string.lines().collect();

        let needle_count = needle_lines.len();
        if needle_count == 0 || content_lines.is_empty() {
            return None;
        }

        let mut best_ratio: f32 = 0.0;
        let mut best_start: usize = 0;
        let mut best_end: usize = 0;

        // Slide a window of `needle_count` lines across the file content
        let max_start = if content_lines.len() >= needle_count {
            content_lines.len() - needle_count + 1
        } else {
            // If the file has fewer lines than the needle, use one window
            // covering the entire file
            1
        };

        for start in 0..max_start {
            let end = (start + needle_count).min(content_lines.len());
            let window_text = content_lines[start..end].join("\n");
            let needle_text = needle_lines.join("\n");

            let diff = TextDiff::from_chars(&needle_text, &window_text);
            let ratio = diff.ratio();

            if ratio > best_ratio {
                best_ratio = ratio;
                best_start = start;
                best_end = end;
            }
        }

        if best_ratio <= 0.0 {
            return None;
        }

        let matched_text = content_lines[best_start..best_end].join("\n");

        Some(FuzzyMatch {
            text: matched_text,
            start_line: best_start + 1, // 1-indexed
            end_line: best_end,         // 1-indexed inclusive
            similarity: best_ratio,
        })
    }
}

/// Result of a fuzzy match search.
struct FuzzyMatch {
    text: String,
    start_line: usize,
    end_line: usize,
    similarity: f32,
}

impl Default for FileEditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    async fn description(&self, _input: &Value) -> String {
        "Performs exact string replacements in files.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_string (default false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string"]
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
        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        json!({
            "file_path": file_path,
            "old_bytes": old_string.len(),
            "old_lines": old_string.lines().count(),
            "new_bytes": new_string.len(),
            "new_lines": new_string.lines().count(),
            "replace_all": replace_all,
            "operation": "edit_file"
        })
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        crate::observable_input::backfill_file_path(input);
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
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
        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if old_string.is_empty() {
            return ValidationResult::Error {
                message: "old_string is required and must not be empty".to_string(),
                error_code: 1,
            };
        }
        let new_string = input.get("new_string").and_then(|v| v.as_str());
        if new_string.is_none() {
            return ValidationResult::Error {
                message: "new_string is required".to_string(),
                error_code: 1,
            };
        }
        if old_string == new_string.unwrap_or("") {
            return ValidationResult::Error {
                message: "old_string and new_string must be different".to_string(),
                error_code: 1,
            };
        }
        let path = Path::new(file_path);
        if path.exists() {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => {
                    if let Err(message) = Self::validate_cached_read(ctx, file_path, path, &content)
                    {
                        return ValidationResult::Error {
                            message: message.to_string(),
                            error_code: 7,
                        };
                    }
                    if let Err(message) = Self::validate_file_writable(path) {
                        return ValidationResult::Error {
                            message,
                            error_code: 8,
                        };
                    }
                }
                Err(e) => {
                    return ValidationResult::Error {
                        message: format!("Failed to read file: {}", e),
                        error_code: 1,
                    };
                }
            }
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
        let (file_path, old_string, new_string, replace_all) = Self::parse_input(&input);

        if file_path.is_empty() || old_string.is_empty() {
            return Ok(ToolResult {
                data: json!({ "error": "file_path and old_string are required" }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let path = Path::new(&file_path);

        if !path.exists() {
            return Ok(ToolResult {
                data: json!({ "error": format!("File not found: {}", file_path) }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Read current content
        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read file: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        if let Err(message) = Self::validate_cached_read(ctx, &file_path, path, &content) {
            return Ok(ToolResult {
                data: json!({ "error": message }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        if let Err(message) = Self::validate_file_writable(path) {
            return Ok(ToolResult {
                data: json!({ "error": message }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Count occurrences of old_string. If the only mismatch is leading
        // indentation, repair the replacement against the file's actual indent.
        let mut actual_old_string = old_string.clone();
        let mut actual_new_string = new_string.clone();
        let mut auto_indent_adjusted = false;
        let mut occurrence_count = content.matches(&actual_old_string).count();

        if occurrence_count == 0 {
            if let Some(adjusted) =
                Self::find_indentation_adjusted_edit(&content, &old_string, &new_string)
            {
                actual_old_string = adjusted.old_string;
                actual_new_string = adjusted.new_string;
                auto_indent_adjusted = true;
                occurrence_count = content.matches(&actual_old_string).count();
            }
        }

        if occurrence_count == 0 {
            // Attempt fuzzy matching to provide a helpful suggestion
            if let Some(fuzzy) = Self::find_best_fuzzy_match(&content, &old_string) {
                if fuzzy.similarity > 0.6 {
                    let pct = (fuzzy.similarity * 100.0).round() as u32;
                    return Ok(ToolResult {
                        data: json!({
                            "error": format!(
                                "old_string not found in {}. Did you mean:\n\n{}\n\n(lines {}-{}, {}% similar)",
                                file_path, fuzzy.text, fuzzy.start_line, fuzzy.end_line, pct
                            )
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }
            }

            return Ok(ToolResult {
                data: json!({
                    "error": format!(
                        "old_string not found in {}. Make sure the string matches exactly, including whitespace and indentation.",
                        file_path
                    )
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        if occurrence_count > 1 && !replace_all {
            return Ok(ToolResult {
                data: json!({
                    "error": format!(
                        "old_string appears {} times in {}. Either provide a larger string with more surrounding context to make it unique, or set replace_all to true.",
                        occurrence_count, file_path
                    )
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // Perform replacement
        let new_content = if replace_all {
            content.replace(&actual_old_string, &actual_new_string)
        } else {
            // Replace only the first occurrence
            content.replacen(&actual_old_string, &actual_new_string, 1)
        };

        let safe_options = SafeWriteOptions {
            max_bytes: MAX_EDIT_FILE_BYTES,
            session_id: Some(ctx.session_id.clone()),
            ..Default::default()
        };
        let file_path_for_write = file_path.clone();
        let new_content_for_write = new_content.clone();
        let write_report = match tokio::task::spawn_blocking(move || {
            safe_write_text(file_path_for_write, &new_content_for_write, &safe_options)
        })
        .await
        {
            Ok(Ok(report)) => report,
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to write edit safely: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Safe edit task failed: {}", e) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        // Write back
        {
            let replacements = if replace_all { occurrence_count } else { 1 };
            Self::record_edit_state(ctx, &file_path, path, &new_content);
            let output = format!(
                "Successfully replaced {} occurrence(s) in {}",
                replacements, file_path
            );
            let hunk_lines = Self::unified_hunk_lines(&file_path, &content, &new_content);
            let display_preview = json!({
                "kind": "file_edit",
                "tool": "Edit",
                "path": &file_path,
                "output": &output,
                "replacements": replacements,
                "auto_indent_adjusted": auto_indent_adjusted,
                "hunk_lines": &hunk_lines,
                "edit_history": {
                    "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                    "atomic": true,
                    "permissions_preserved": write_report.permissions_preserved,
                },
            })
            .to_string();

            // Fire FileChanged hook
            {
                let app_state = (ctx.get_app_state)();
                let configs =
                    allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "FileChanged");
                if !configs.is_empty() {
                    let payload = json!({
                        "file_path": &file_path,
                        "operation": "edit",
                        "replacements": replacements,
                        "auto_indent_adjusted": auto_indent_adjusted,
                        "edit_history": {
                            "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                        },
                    });
                    let _ = crate::hooks::run_event_hooks("FileChanged", &payload, &configs).await;
                }
            }

            Ok(ToolResult {
                data: json!({
                    "output": &output,
                    "path": &file_path,
                    "replacements": replacements,
                    "auto_indent_adjusted": auto_indent_adjusted,
                    "edit_history": {
                        "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                        "atomic": true,
                        "permissions_preserved": write_report.permissions_preserved,
                    },
                }),
                model_content: Some(ToolResultContent::Text(output)),
                display_preview: Some(display_preview),
                new_messages: vec![super::edited_text_file_message(file_path)],
            })
        }
    }

    async fn prompt(&self) -> String {
        "Performs exact string replacements in files.\n\n\
Usage:\n\
- You must use your `Read` tool at least once in the conversation before editing. This tool will error if you attempt an edit without reading the file. \n\
- When editing text from Read tool output, ensure you preserve the exact indentation (tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: line number + tab. Everything after that is the actual file content to match. Never include any part of the line number prefix in the old_string or new_string.\n\
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless explicitly required.\n\
- Only use emojis if the user explicitly requests it. Avoid adding emojis to files unless asked.\n\
- The edit will FAIL if `old_string` is not unique in the file. Either provide a larger string with more surrounding context to make it unique or use `replace_all` to change every instance of `old_string`.\n\
- Use `replace_all` for replacing and renaming strings across the file. This parameter is useful if you want to rename a variable for instance.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Edit".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::file_read::FileReadTool;
    use crate::tool::ToolAppState as AppState;
    use crate::tool::{FileStateCache, ToolUseOptions};
    use allthecodes_types::message::ContentBlock;
    use std::ffi::OsString;
    use std::sync::Arc;
    use uuid::Uuid;

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, path: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, path);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn test_context() -> ToolUseContext {
        let app_state = AppState::default();
        let (_tx, rx) = tokio::sync::watch::channel(false);

        ToolUseContext {
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
            session_id: "file-edit-test-session".to_string(),
            langfuse_session_id: "file-edit-test-session".to_string(),
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

    fn cache_file_state(ctx: &ToolUseContext, path: &Path, content: &str) {
        let timestamp = std::fs::metadata(path)
            .map(|metadata| FileEditTool::modified_millis(&metadata))
            .unwrap_or(0);
        ctx.read_file_state.insert(
            path.to_string_lossy().to_string(),
            FileCacheEntry {
                content_hash: FileStateCache::hash_content(content.as_bytes()),
                last_read_timestamp: timestamp,
            },
        );
    }

    #[test]
    fn test_fuzzy_match_whitespace_diff() {
        // The file has 4-space indentation, but old_string uses 2-space
        let content = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let old_string = "  let x = 1;\n  let y = 2;";

        let result = FileEditTool::find_best_fuzzy_match(content, old_string);
        assert!(result.is_some(), "Should find a fuzzy match");

        let m = result.unwrap();
        assert!(
            m.similarity > 0.6,
            "Similarity should be > 0.6 for whitespace-only difference, got {}",
            m.similarity
        );
        assert_eq!(m.start_line, 2);
        assert_eq!(m.end_line, 3);
        assert!(m.text.contains("let x = 1;"));
        assert!(m.text.contains("let y = 2;"));
    }

    #[test]
    fn test_fuzzy_match_no_close_match() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let old_string = "struct Foo {\n    bar: i32,\n    baz: String,\n}";

        let result = FileEditTool::find_best_fuzzy_match(content, old_string);
        // Either None or similarity <= 0.6
        match result {
            None => {} // acceptable
            Some(m) => {
                assert!(
                    m.similarity <= 0.6,
                    "Similarity should be <= 0.6 for unrelated content, got {}",
                    m.similarity
                );
            }
        }
    }

    #[test]
    fn test_fuzzy_match_high_similarity() {
        // One character difference: "count" vs "counr" (typo)
        let content = "let count = 0;\ncount += 1;\nprintln!(\"{}\", count);\n";
        let old_string = "let counr = 0;\ncounr += 1;\nprintln!(\"{}\", counr);";

        let result = FileEditTool::find_best_fuzzy_match(content, old_string);
        assert!(result.is_some(), "Should find a high-similarity match");

        let m = result.unwrap();
        assert!(
            m.similarity > 0.8,
            "Similarity should be > 0.8 for single-character typos, got {}",
            m.similarity
        );
        assert_eq!(m.start_line, 1);
        assert_eq!(m.end_line, 3);
        assert!(m.text.contains("let count = 0;"));
    }

    #[test]
    fn indentation_adjustment_requires_unique_trimmed_match() {
        let content = "\
if ready {
    println!(\"one\");
}

if ready {
    println!(\"one\");
}
";
        let old_string = "  if ready {\n    println!(\"one\");\n  }";
        let new_string = "  if ready {\n    println!(\"two\");\n  }";

        assert!(
            FileEditTool::find_indentation_adjusted_edit(content, old_string, new_string).is_none()
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn edit_rejects_file_that_has_not_been_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "alpha\n").await.unwrap();

        let ctx = test_context();
        let result = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "alpha",
                    "new_string": "beta",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["error"], FILE_NOT_READ_ERROR);
        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "alpha\n"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn edit_rejects_file_modified_after_cached_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "alpha\n").await.unwrap();

        let ctx = test_context();
        cache_file_state(&ctx, &file_path, "alpha\n");
        tokio::fs::write(&file_path, "external\n").await.unwrap();

        let result = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "alpha",
                    "new_string": "beta",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["error"], FILE_UNEXPECTEDLY_MODIFIED_ERROR);
        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "external\n"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn edit_auto_adjusts_unique_indentation_mismatch() {
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", dir.path());
        let file_path = dir.path().join("sample.rs");
        let original = "\
fn main() {
    if ready {
        println!(\"old\");
    }
}
";
        tokio::fs::write(&file_path, original).await.unwrap();

        let ctx = test_context();
        cache_file_state(&ctx, &file_path, original);

        let result = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "if ready {\n  println!(\"old\");\n}",
                    "new_string": "if ready {\n  println!(\"new\");\n}",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["replacements"], 1);
        assert_eq!(result.data["auto_indent_adjusted"], true);
        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "\
fn main() {
    if ready {
        println!(\"new\");
    }
}
"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn edit_rejects_readonly_file_before_writing() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "alpha\n").await.unwrap();

        let ctx = test_context();
        cache_file_state(&ctx, &file_path, "alpha\n");

        let mut permissions = std::fs::metadata(&file_path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file_path, permissions).unwrap();

        let result = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "alpha",
                    "new_string": "beta",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&file_path).unwrap().permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(&file_path, permissions).unwrap();
        }
        #[cfg(not(unix))]
        {
            let mut permissions = std::fs::metadata(&file_path).unwrap().permissions();
            permissions.set_readonly(false);
            std::fs::set_permissions(&file_path, permissions).unwrap();
        }

        assert!(result.data["error"]
            .as_str()
            .unwrap()
            .contains("readonly or locked"));
        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "alpha\n"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn full_read_registers_state_and_edit_refreshes_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", dir.path());
        let file_path = dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "alpha\nbeta\n").await.unwrap();

        let ctx = test_context();
        let read_result = FileReadTool::new()
            .call(
                json!({ "file_path": file_path.to_string_lossy() }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(read_result.data["truncated"], false);

        let first_edit = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "alpha",
                    "new_string": "gamma",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(first_edit.data["replacements"], 1);
        assert_eq!(first_edit.data["edit_history"]["atomic"], true);
        let expected_path = file_path.to_string_lossy().to_string();
        assert!(matches!(
            first_edit.new_messages.as_slice(),
            [allthecodes_types::message::Message::Attachment(attachment)]
                if matches!(
                    &attachment.attachment,
                    allthecodes_types::message::Attachment::EditedTextFile { path }
                        if path == &expected_path
                )
        ));
        match first_edit.model_content {
            Some(ToolResultContent::Text(ref text)) => {
                assert!(text.contains("Successfully replaced 1 occurrence(s)"));
            }
            other => panic!("expected concise text model content, got {:?}", other),
        }
        let display_preview = first_edit
            .display_preview
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .expect("edit should expose a UI display preview");
        assert_eq!(display_preview["kind"], "file_edit");
        assert_eq!(display_preview["replacements"], 1);
        assert!(display_preview["hunk_lines"]
            .as_array()
            .expect("preview has hunk lines")
            .iter()
            .any(|line| line.as_str().is_some_and(|line| line.contains("gamma"))));
        let backup_path = first_edit.data["edit_history"]["backup_path"]
            .as_str()
            .expect("edit should create a recovery backup");
        assert_eq!(
            std::fs::read_to_string(backup_path).unwrap(),
            "alpha\nbeta\n"
        );

        let second_edit = FileEditTool::new()
            .call(
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "old_string": "beta",
                    "new_string": "delta",
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(second_edit.data["replacements"], 1);
        assert_eq!(
            tokio::fs::read_to_string(&file_path).await.unwrap(),
            "gamma\ndelta\n"
        );
    }
}
