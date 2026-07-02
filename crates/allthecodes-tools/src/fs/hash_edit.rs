use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::TextDiff;

use crate::tool::{
    FileCacheEntry, FileStateCache, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_types::message::{AssistantMessage, ToolResultContent};

use super::hashline::{self, TextLine};
use super::safe_write::{safe_write_text, SafeWriteOptions};

pub struct HashEditTool;

const FILE_NOT_READ_ERROR: &str =
    "File has not been read yet. Read it fully before using HashEdit.";
const FILE_UNEXPECTEDLY_MODIFIED_ERROR: &str =
    "File has been unexpectedly modified. Read it again before using HashEdit.";
const HASHLINE_DISABLED_ERROR: &str = "HashEdit requires hashline_mode to be enabled.";
const MAX_HASH_EDIT_FILE_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
struct HashEditInput {
    file_path: String,
    base_file_hash: String,
    operations: Vec<HashEditOperation>,
}

#[derive(Debug, Clone, Deserialize)]
struct LineAnchor {
    line: usize,
    hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HashEditOperation {
    ReplaceRange {
        start: LineAnchor,
        end: LineAnchor,
        new_text: String,
    },
    DeleteRange {
        start: LineAnchor,
        end: LineAnchor,
    },
    InsertBefore {
        anchor: LineAnchor,
        new_text: String,
    },
    InsertAfter {
        anchor: LineAnchor,
        new_text: String,
    },
}

#[derive(Debug, Clone)]
enum ResolvedOperation {
    Replace {
        start: usize,
        end: usize,
        replacement: Vec<TextLine>,
    },
    Delete {
        start: usize,
        end: usize,
    },
    Insert {
        index: usize,
        after_line: Option<usize>,
        replacement: Vec<TextLine>,
    },
}

impl HashEditTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_input(input: &Value) -> std::result::Result<HashEditInput, String> {
        serde_json::from_value(input.clone())
            .map_err(|err| format!("Invalid HashEdit input: {err}"))
    }

    fn hashline_enabled(ctx: &ToolUseContext) -> bool {
        (ctx.get_app_state)()
            .settings
            .hashline_mode
            .unwrap_or(false)
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
            .map_err(|err| format!("Failed to stat file before editing: {err}"))?;
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
                    format!("Failed to open file for editing: {err}")
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

    fn resolve_anchor(
        anchor: &LineAnchor,
        lines: &[TextLine],
    ) -> std::result::Result<usize, String> {
        if anchor.line == 0 || anchor.line > lines.len() {
            return Err(format!(
                "Line anchor {} is outside the current file range 1-{}",
                anchor.line,
                lines.len()
            ));
        }

        let index = anchor.line - 1;
        let actual_hash = hashline::line_hash(&lines[index].body);
        if actual_hash != anchor.hash {
            return Err(format!(
                "Line anchor {} hash mismatch: expected {}, found {}. Read the file again.",
                anchor.line, anchor.hash, actual_hash
            ));
        }

        Ok(index)
    }

    fn resolve_operations(
        operations: &[HashEditOperation],
        lines: &[TextLine],
    ) -> std::result::Result<Vec<ResolvedOperation>, String> {
        let eol = hashline::dominant_line_ending(lines);
        let mut resolved = Vec::new();
        let mut occupied_lines = HashSet::new();
        let mut insertion_points = HashSet::new();

        for operation in operations {
            match operation {
                HashEditOperation::ReplaceRange {
                    start,
                    end,
                    new_text,
                } => {
                    let start_index = Self::resolve_anchor(start, lines)?;
                    let end_index = Self::resolve_anchor(end, lines)?;
                    if start_index > end_index {
                        return Err(
                            "replace_range start must be before or equal to end".to_string()
                        );
                    }
                    Self::reserve_range(&mut occupied_lines, start_index, end_index)?;
                    let replacement =
                        Self::replacement_lines(new_text, eol, end_index + 1 < lines.len());
                    resolved.push(ResolvedOperation::Replace {
                        start: start_index,
                        end: end_index,
                        replacement,
                    });
                }
                HashEditOperation::DeleteRange { start, end } => {
                    let start_index = Self::resolve_anchor(start, lines)?;
                    let end_index = Self::resolve_anchor(end, lines)?;
                    if start_index > end_index {
                        return Err("delete_range start must be before or equal to end".to_string());
                    }
                    Self::reserve_range(&mut occupied_lines, start_index, end_index)?;
                    resolved.push(ResolvedOperation::Delete {
                        start: start_index,
                        end: end_index,
                    });
                }
                HashEditOperation::InsertBefore { anchor, new_text } => {
                    let anchor_index = Self::resolve_anchor(anchor, lines)?;
                    Self::reserve_insertion(&mut insertion_points, anchor_index)?;
                    resolved.push(ResolvedOperation::Insert {
                        index: anchor_index,
                        after_line: None,
                        replacement: Self::insertion_lines(new_text, eol),
                    });
                }
                HashEditOperation::InsertAfter { anchor, new_text } => {
                    let anchor_index = Self::resolve_anchor(anchor, lines)?;
                    let insertion_index = anchor_index + 1;
                    Self::reserve_insertion(&mut insertion_points, insertion_index)?;
                    resolved.push(ResolvedOperation::Insert {
                        index: insertion_index,
                        after_line: Some(anchor_index),
                        replacement: Self::insertion_lines(new_text, eol),
                    });
                }
            }
        }

        Ok(resolved)
    }

    fn reserve_range(
        occupied_lines: &mut HashSet<usize>,
        start: usize,
        end: usize,
    ) -> std::result::Result<(), String> {
        for line in start..=end {
            if !occupied_lines.insert(line) {
                return Err("HashEdit operations must not overlap modified line ranges".to_string());
            }
        }
        Ok(())
    }

    fn reserve_insertion(
        insertion_points: &mut HashSet<usize>,
        index: usize,
    ) -> std::result::Result<(), String> {
        if !insertion_points.insert(index) {
            return Err(
                "HashEdit operations must not insert multiple times at the same anchor".to_string(),
            );
        }
        Ok(())
    }

    fn replacement_lines(text: &str, eol: &str, has_following_lines: bool) -> Vec<TextLine> {
        if text.is_empty() {
            return Vec::new();
        }
        let mut normalized = Self::normalize_line_endings(text, eol);
        if has_following_lines && !normalized.ends_with(eol) {
            normalized.push_str(eol);
        }
        hashline::parse_lines(&normalized)
    }

    fn insertion_lines(text: &str, eol: &str) -> Vec<TextLine> {
        if text.is_empty() {
            return Vec::new();
        }
        let mut normalized = Self::normalize_line_endings(text, eol);
        if !normalized.ends_with(eol) {
            normalized.push_str(eol);
        }
        hashline::parse_lines(&normalized)
    }

    fn normalize_line_endings(text: &str, eol: &str) -> String {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if eol == "\n" {
            normalized
        } else {
            normalized.replace('\n', eol)
        }
    }

    fn apply_operations(mut lines: Vec<TextLine>, operations: Vec<ResolvedOperation>) -> String {
        let eol = hashline::dominant_line_ending(&lines).to_string();
        let mut operations = operations;
        operations.sort_by(|left, right| right.sort_index().cmp(&left.sort_index()));

        for operation in operations {
            match operation {
                ResolvedOperation::Replace {
                    start,
                    end,
                    replacement,
                } => {
                    lines.splice(start..=end, replacement);
                }
                ResolvedOperation::Delete { start, end } => {
                    lines.drain(start..=end);
                }
                ResolvedOperation::Insert {
                    index,
                    after_line,
                    replacement,
                } => {
                    if let Some(after_line) = after_line {
                        if let Some(line) = lines.get_mut(after_line) {
                            if line.ending.is_empty() {
                                line.ending = eol.clone();
                            }
                        }
                    }
                    lines.splice(index..index, replacement);
                }
            }
        }

        lines.into_iter().map(|line| line.full_text()).collect()
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

    async fn run_hash_edit(
        &self,
        parsed: HashEditInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult> {
        if !Self::hashline_enabled(ctx) {
            return Ok(Self::error_result(HASHLINE_DISABLED_ERROR));
        }

        let path = Path::new(&parsed.file_path);
        if !path.exists() {
            return Ok(Self::error_result(format!(
                "File not found: {}",
                parsed.file_path
            )));
        }

        let content = match tokio::fs::read_to_string(&parsed.file_path).await {
            Ok(content) => content,
            Err(err) => {
                return Ok(Self::error_result(format!("Failed to read file: {err}")));
            }
        };

        if let Err(message) = Self::validate_cached_read(ctx, &parsed.file_path, path, &content) {
            return Ok(Self::error_result(message));
        }
        if hashline::file_hash(&content) != parsed.base_file_hash {
            return Ok(Self::error_result(
                "base_file_hash does not match the current file. Read the file again.",
            ));
        }
        if let Err(message) = Self::validate_file_writable(path) {
            return Ok(Self::error_result(message));
        }

        let lines = hashline::parse_lines(&content);
        let resolved = match Self::resolve_operations(&parsed.operations, &lines) {
            Ok(resolved) => resolved,
            Err(message) => return Ok(Self::error_result(message)),
        };
        let operation_count = resolved.len();
        let new_content = Self::apply_operations(lines, resolved);

        if new_content == content {
            return Ok(Self::error_result("HashEdit produced no changes."));
        }

        let safe_options = SafeWriteOptions {
            max_bytes: MAX_HASH_EDIT_FILE_BYTES,
            session_id: Some(ctx.session_id.clone()),
            ..Default::default()
        };
        let file_path_for_write = parsed.file_path.clone();
        let new_content_for_write = new_content.clone();
        let write_report = match tokio::task::spawn_blocking(move || {
            safe_write_text(file_path_for_write, &new_content_for_write, &safe_options)
        })
        .await
        {
            Ok(Ok(report)) => report,
            Ok(Err(err)) => {
                return Ok(Self::error_result(format!(
                    "Failed to write edit safely: {err}"
                )));
            }
            Err(err) => {
                return Ok(Self::error_result(format!("Safe edit task failed: {err}")));
            }
        };

        Self::record_edit_state(ctx, &parsed.file_path, path, &new_content);
        let output = format!(
            "Successfully applied {} hash edit operation(s) in {}",
            operation_count, parsed.file_path
        );
        let hunk_lines = Self::unified_hunk_lines(&parsed.file_path, &content, &new_content);
        let display_preview = json!({
            "kind": "file_edit",
            "tool": "HashEdit",
            "path": &parsed.file_path,
            "output": &output,
            "operations": operation_count,
            "hunk_lines": &hunk_lines,
            "edit_history": {
                "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                "atomic": true,
                "permissions_preserved": write_report.permissions_preserved,
            },
        })
        .to_string();

        {
            let app_state = (ctx.get_app_state)();
            let configs =
                allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "FileChanged");
            if !configs.is_empty() {
                let payload = json!({
                    "file_path": &parsed.file_path,
                    "operation": "hash_edit",
                    "operations": operation_count,
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
                "path": &parsed.file_path,
                "operations": operation_count,
                "edit_history": {
                    "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                    "atomic": true,
                    "permissions_preserved": write_report.permissions_preserved,
                },
            }),
            model_content: Some(ToolResultContent::Text(output)),
            display_preview: Some(display_preview),
            new_messages: vec![super::edited_text_file_message(parsed.file_path)],
            ..Default::default()
        })
    }

    fn error_result(message: impl Into<String>) -> ToolResult {
        ToolResult {
            data: json!({ "error": message.into() }),
            new_messages: vec![],
            ..Default::default()
        }
    }
}

impl ResolvedOperation {
    fn sort_index(&self) -> usize {
        match self {
            ResolvedOperation::Replace { start, .. } => *start,
            ResolvedOperation::Delete { start, .. } => *start,
            ResolvedOperation::Insert { index, .. } => *index,
        }
    }
}

impl Default for HashEditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HashEditTool {
    fn name(&self) -> &str {
        "HashEdit"
    }

    async fn description(&self, _input: &Value) -> String {
        "Edits files using line-number plus line-hash anchors from hashline Read output."
            .to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "base_file_hash": {
                    "type": "string",
                    "description": "The base_file_hash value from the most recent full Read output"
                },
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["replace_range", "delete_range", "insert_before", "insert_after"]
                            },
                            "start": { "$ref": "#/$defs/anchor" },
                            "end": { "$ref": "#/$defs/anchor" },
                            "anchor": { "$ref": "#/$defs/anchor" },
                            "new_text": { "type": "string" }
                        },
                        "required": ["type"]
                    }
                }
            },
            "required": ["file_path", "base_file_hash", "operations"],
            "$defs": {
                "anchor": {
                    "type": "object",
                    "properties": {
                        "line": { "type": "integer", "minimum": 1 },
                        "hash": { "type": "string" }
                    },
                    "required": ["line", "hash"]
                }
            }
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
        let operations = input
            .get("operations")
            .and_then(|value| value.as_array())
            .map(|value| value.len())
            .unwrap_or(0);
        json!({
            "file_path": file_path,
            "operations": operations,
            "operation": "hash_edit_file"
        })
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        crate::interaction::observable_input::backfill_file_path(input);
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        if !Self::hashline_enabled(ctx) {
            return ValidationResult::Error {
                message: HASHLINE_DISABLED_ERROR.to_string(),
                error_code: 1,
            };
        }

        let parsed = match Self::parse_input(input) {
            Ok(parsed) => parsed,
            Err(message) => {
                return ValidationResult::Error {
                    message,
                    error_code: 1,
                };
            }
        };
        if parsed.file_path.is_empty() {
            return ValidationResult::Error {
                message: "file_path is required".to_string(),
                error_code: 1,
            };
        }
        if parsed.base_file_hash.is_empty() {
            return ValidationResult::Error {
                message: "base_file_hash is required".to_string(),
                error_code: 1,
            };
        }
        if parsed.operations.is_empty() {
            return ValidationResult::Error {
                message: "operations must contain at least one operation".to_string(),
                error_code: 1,
            };
        }

        let path = Path::new(&parsed.file_path);
        if path.exists() {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => {
                    if let Err(message) =
                        Self::validate_cached_read(ctx, &parsed.file_path, path, &content)
                    {
                        return ValidationResult::Error {
                            message: message.to_string(),
                            error_code: 7,
                        };
                    }
                    if hashline::file_hash(&content) != parsed.base_file_hash {
                        return ValidationResult::Error {
                            message:
                                "base_file_hash does not match the current file. Read the file again."
                                    .to_string(),
                            error_code: 7,
                        };
                    }
                    if let Err(message) = Self::validate_file_writable(path) {
                        return ValidationResult::Error {
                            message,
                            error_code: 8,
                        };
                    }
                    let lines = hashline::parse_lines(&content);
                    if let Err(message) = Self::resolve_operations(&parsed.operations, &lines) {
                        return ValidationResult::Error {
                            message,
                            error_code: 1,
                        };
                    }
                }
                Err(err) => {
                    return ValidationResult::Error {
                        message: format!("Failed to read file: {err}"),
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
        let parsed = match Self::parse_input(&input) {
            Ok(parsed) => parsed,
            Err(message) => return Ok(Self::error_result(message)),
        };
        self.run_hash_edit(parsed, ctx).await
    }

    async fn prompt(&self) -> String {
        "Edits text files by anchoring changes to Read output line hashes.\n\n\
Usage:\n\
- Use this tool only when hashline edit mode is enabled and the latest full Read output includes base_file_hash and line#hash prefixes.\n\
- You must provide base_file_hash from the Read output.\n\
- Anchors use the Read line number and the short hash after '#'. Ranges are inclusive.\n\
- This tool edits whole lines. For replace_range and delete_range, start and end must both match current file hashes.\n\
- If any hash does not match, read the file again before editing.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "HashEdit".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolAppState as AppState;
    use crate::tool::{ToolUseOptions, ValidationResult};
    use allthecodes_types::message::ContentBlock;
    use std::sync::Arc;
    use uuid::Uuid;

    fn test_context(hashline_mode: bool) -> ToolUseContext {
        let mut app_state = AppState::default();
        app_state.settings.hashline_mode = Some(hashline_mode);
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
            session_id: "hash-edit-test-session".to_string(),
            langfuse_session_id: "hash-edit-test-session".to_string(),
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
        ctx.read_file_state.insert(
            path.to_string_lossy().to_string(),
            FileCacheEntry {
                content_hash: FileStateCache::hash_content(content.as_bytes()),
                last_read_timestamp: 0,
            },
        );
    }

    #[tokio::test]
    async fn replaces_range_when_hashes_match() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sample.txt");
        let original = "alpha\nbeta\ngamma\n";
        std::fs::write(&path, original).unwrap();
        let ctx = test_context(true);
        cache_file_state(&ctx, &path, original);

        let input = json!({
            "file_path": path.to_string_lossy(),
            "base_file_hash": hashline::file_hash(original),
            "operations": [{
                "type": "replace_range",
                "start": { "line": 2, "hash": hashline::line_hash("beta") },
                "end": { "line": 2, "hash": hashline::line_hash("beta") },
                "new_text": "BETA\n"
            }]
        });

        let result = HashEditTool::new()
            .call(input, &ctx, &parent_message(), None)
            .await
            .unwrap();
        assert!(result.data.get("error").is_none());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "alpha\nBETA\ngamma\n"
        );
    }

    #[tokio::test]
    async fn rejects_stale_line_hash() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sample.txt");
        let original = "alpha\nbeta\n";
        std::fs::write(&path, original).unwrap();
        let ctx = test_context(true);
        cache_file_state(&ctx, &path, original);

        let input = json!({
            "file_path": path.to_string_lossy(),
            "base_file_hash": hashline::file_hash(original),
            "operations": [{
                "type": "delete_range",
                "start": { "line": 2, "hash": "badbadbadbad" },
                "end": { "line": 2, "hash": "badbadbadbad" }
            }]
        });

        let validation = HashEditTool::new().validate_input(&input, &ctx).await;
        assert!(matches!(validation, ValidationResult::Error { .. }));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[tokio::test]
    async fn rejects_when_hashline_mode_is_disabled() {
        let ctx = test_context(false);
        let input = json!({
            "file_path": "/tmp/nope",
            "base_file_hash": "abc",
            "operations": []
        });
        let validation = HashEditTool::new().validate_input(&input, &ctx).await;
        assert!(matches!(validation, ValidationResult::Error { .. }));
    }

    #[tokio::test]
    async fn preserves_crlf_for_insertions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sample.txt");
        let original = "alpha\r\nbeta\r\n";
        std::fs::write(&path, original).unwrap();
        let ctx = test_context(true);
        cache_file_state(&ctx, &path, original);

        let input = json!({
            "file_path": path.to_string_lossy(),
            "base_file_hash": hashline::file_hash(original),
            "operations": [{
                "type": "insert_after",
                "anchor": { "line": 1, "hash": hashline::line_hash("alpha") },
                "new_text": "inserted"
            }]
        });

        let result = HashEditTool::new()
            .call(input, &ctx, &parent_message(), None)
            .await
            .unwrap();
        assert!(result.data.get("error").is_none());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "alpha\r\ninserted\r\nbeta\r\n"
        );
    }
}
