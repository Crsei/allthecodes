use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use similar::TextDiff;

use crate::tool::{
    FileCacheEntry, FileStateCache, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_types::message::{AssistantMessage, ToolResultContent};

use super::safe_write::{safe_write_text, SafeWriteOptions};

const MAX_NOTEBOOK_BYTES: usize = 1024 * 1024 * 1024;
const FILE_NOT_READ_ERROR: &str = "File has not been read yet. Read it first before writing to it.";
const FILE_UNEXPECTEDLY_MODIFIED_ERROR: &str =
    "File has been unexpectedly modified. Read it again before attempting to write it.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Replace,
    Insert,
    Delete,
}

impl EditMode {
    fn parse(raw: Option<&str>) -> std::result::Result<Self, &'static str> {
        match raw.unwrap_or("replace") {
            "replace" => Ok(Self::Replace),
            "insert" => Ok(Self::Insert),
            "delete" => Ok(Self::Delete),
            _ => Err("Edit mode must be replace, insert, or delete."),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Insert => "insert",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellType {
    Code,
    Markdown,
}

impl CellType {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "code" => Some(Self::Code),
            "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone)]
struct NotebookEditInput {
    notebook_path: String,
    cell_id: Option<String>,
    new_source: String,
    cell_type: Option<CellType>,
    edit_mode: EditMode,
}

#[derive(Debug, Clone)]
struct NotebookEditOutcome {
    notebook: Value,
    edit_mode: EditMode,
    cell_id: Option<String>,
    cell_type: CellType,
    language: String,
}

pub struct NotebookEditTool;

impl Default for NotebookEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl NotebookEditTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_input(input: &Value) -> std::result::Result<NotebookEditInput, String> {
        let notebook_path = input
            .get("notebook_path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if notebook_path.is_empty() {
            return Err("notebook_path is required".to_string());
        }

        let new_source = input
            .get("new_source")
            .and_then(Value::as_str)
            .ok_or_else(|| "new_source is required".to_string())?
            .to_string();
        let edit_mode = EditMode::parse(input.get("edit_mode").and_then(Value::as_str))
            .map_err(str::to_string)?;
        let cell_type = match input.get("cell_type").and_then(Value::as_str) {
            Some(raw) => Some(
                CellType::parse(raw)
                    .ok_or_else(|| "cell_type must be code or markdown.".to_string())?,
            ),
            None => None,
        };

        if edit_mode == EditMode::Insert && cell_type.is_none() {
            return Err("Cell type is required when using edit_mode=insert.".to_string());
        }

        Ok(NotebookEditInput {
            notebook_path,
            cell_id: input
                .get("cell_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            new_source,
            cell_type,
            edit_mode,
        })
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

    fn validate_file_writable(path: &Path) -> std::result::Result<(), String> {
        let metadata = std::fs::metadata(path)
            .map_err(|err| format!("Failed to stat notebook before editing: {}", err))?;
        if metadata.permissions().readonly() {
            return Err(format!(
                "Notebook is readonly or locked and cannot be edited: {}",
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
                        "Notebook is locked or not writable and cannot be edited: {} ({})",
                        path.display(),
                        err
                    )
                } else {
                    format!("Failed to open notebook for editing: {}", err)
                }
            })
    }

    fn is_lock_or_permission_error(err: &io::Error) -> bool {
        matches!(
            err.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
        ) || matches!(err.raw_os_error(), Some(5 | 32 | 33))
    }

    fn parse_notebook(content: &str) -> std::result::Result<Value, String> {
        let notebook: Value =
            serde_json::from_str(content).map_err(|_| "Notebook is not valid JSON.".to_string())?;
        if !notebook
            .get("cells")
            .and_then(Value::as_array)
            .is_some_and(|_| notebook.is_object())
        {
            return Err("Notebook JSON must contain a cells array.".to_string());
        }
        Ok(notebook)
    }

    fn language(notebook: &Value) -> String {
        notebook
            .get("metadata")
            .and_then(|v| v.get("language_info"))
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("python")
            .to_string()
    }

    fn nbformat_supports_cell_ids(notebook: &Value) -> bool {
        let major = notebook
            .get("nbformat")
            .and_then(Value::as_u64)
            .unwrap_or(4);
        let minor = notebook
            .get("nbformat_minor")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        major > 4 || (major == 4 && minor >= 5)
    }

    fn cells(notebook: &Value) -> std::result::Result<&Vec<Value>, String> {
        notebook
            .get("cells")
            .and_then(Value::as_array)
            .ok_or_else(|| "Notebook JSON must contain a cells array.".to_string())
    }

    fn cells_mut(notebook: &mut Value) -> std::result::Result<&mut Vec<Value>, String> {
        notebook
            .get_mut("cells")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "Notebook JSON must contain a cells array.".to_string())
    }

    fn parse_cell_index(cell_id: &str) -> Option<usize> {
        cell_id.strip_prefix("cell-")?.parse::<usize>().ok()
    }

    fn find_cell_index(cells: &[Value], cell_id: &str) -> Option<usize> {
        cells
            .iter()
            .position(|cell| cell.get("id").and_then(Value::as_str) == Some(cell_id))
            .or_else(|| Self::parse_cell_index(cell_id).filter(|idx| *idx < cells.len()))
    }

    fn validate_cell_reference(notebook: &Value, input: &NotebookEditInput) -> ValidationResult {
        let Ok(cells) = Self::cells(notebook) else {
            return ValidationResult::Error {
                message: "Notebook JSON must contain a cells array.".to_string(),
                error_code: 6,
            };
        };

        match (&input.cell_id, input.edit_mode) {
            (None, EditMode::Insert) => ValidationResult::Ok,
            (None, _) => ValidationResult::Error {
                message: "Cell ID must be specified when not inserting a new cell.".to_string(),
                error_code: 7,
            },
            (Some(cell_id), _) if Self::find_cell_index(cells, cell_id).is_some() => {
                ValidationResult::Ok
            }
            (Some(cell_id), _) if Self::parse_cell_index(cell_id).is_some() => {
                let parsed = Self::parse_cell_index(cell_id).unwrap_or(0);
                ValidationResult::Error {
                    message: format!("Cell with index {} does not exist in notebook.", parsed),
                    error_code: 7,
                }
            }
            (Some(cell_id), _) => ValidationResult::Error {
                message: format!("Cell with ID \"{}\" not found in notebook.", cell_id),
                error_code: 8,
            },
        }
    }

    fn apply_edit(
        mut notebook: Value,
        input: &NotebookEditInput,
    ) -> std::result::Result<NotebookEditOutcome, String> {
        let language = Self::language(&notebook);
        let supports_ids = Self::nbformat_supports_cell_ids(&notebook);
        let original_len = Self::cells(&notebook)?.len();

        let mut cell_index = match &input.cell_id {
            Some(cell_id) => Self::find_cell_index(Self::cells(&notebook)?, cell_id)
                .ok_or_else(|| format!("Cell with ID \"{}\" not found in notebook.", cell_id))?,
            None => 0,
        };

        let mut edit_mode = input.edit_mode;
        if edit_mode == EditMode::Insert && input.cell_id.is_some() {
            cell_index += 1;
        }
        if edit_mode == EditMode::Replace && cell_index == original_len {
            edit_mode = EditMode::Insert;
        }

        let mut resulting_cell_id = input.cell_id.clone();
        let resulting_cell_type = match edit_mode {
            EditMode::Delete => {
                let cells = Self::cells(&notebook)?;
                Self::cell_type(cells.get(cell_index))?
            }
            EditMode::Insert => input.cell_type.unwrap_or(CellType::Code),
            EditMode::Replace => {
                let cells = Self::cells(&notebook)?;
                input
                    .cell_type
                    .unwrap_or(Self::cell_type(cells.get(cell_index))?)
            }
        };

        if edit_mode == EditMode::Insert {
            resulting_cell_id = supports_ids.then(Self::new_cell_id);
        }

        {
            let cells = Self::cells_mut(&mut notebook)?;
            match edit_mode {
                EditMode::Delete => {
                    if cell_index >= cells.len() {
                        return Err(format!(
                            "Cell index {} does not exist in notebook.",
                            cell_index
                        ));
                    }
                    cells.remove(cell_index);
                }
                EditMode::Insert => {
                    if cell_index > cells.len() {
                        return Err(format!(
                            "Cell index {} does not exist in notebook.",
                            cell_index
                        ));
                    }
                    cells.insert(
                        cell_index,
                        Self::new_cell(
                            resulting_cell_type,
                            resulting_cell_id.as_deref(),
                            &input.new_source,
                        ),
                    );
                }
                EditMode::Replace => {
                    let cell = cells.get_mut(cell_index).ok_or_else(|| {
                        format!("Cell index {} does not exist in notebook.", cell_index)
                    })?;
                    Self::replace_cell_source(cell, &input.new_source, resulting_cell_type)?;
                }
            }
        }

        Ok(NotebookEditOutcome {
            notebook,
            edit_mode,
            cell_id: resulting_cell_id,
            cell_type: resulting_cell_type,
            language,
        })
    }

    fn cell_type(cell: Option<&Value>) -> std::result::Result<CellType, String> {
        let raw = cell
            .and_then(|cell| cell.get("cell_type"))
            .and_then(Value::as_str)
            .ok_or_else(|| "Notebook cell is missing a cell_type.".to_string())?;
        CellType::parse(raw).ok_or_else(|| format!("Unsupported notebook cell type: {}", raw))
    }

    fn new_cell(cell_type: CellType, cell_id: Option<&str>, new_source: &str) -> Value {
        let mut cell = Map::new();
        cell.insert(
            "cell_type".to_string(),
            Value::String(cell_type.as_str().to_string()),
        );
        if let Some(cell_id) = cell_id {
            cell.insert("id".to_string(), Value::String(cell_id.to_string()));
        }
        cell.insert("metadata".to_string(), Value::Object(Map::new()));
        cell.insert("source".to_string(), Value::String(new_source.to_string()));
        if cell_type == CellType::Code {
            cell.insert("execution_count".to_string(), Value::Null);
            cell.insert("outputs".to_string(), Value::Array(Vec::new()));
        }
        Value::Object(cell)
    }

    fn replace_cell_source(
        cell: &mut Value,
        new_source: &str,
        cell_type: CellType,
    ) -> std::result::Result<(), String> {
        let object = cell
            .as_object_mut()
            .ok_or_else(|| "Notebook cell must be an object.".to_string())?;
        object.insert("source".to_string(), Value::String(new_source.to_string()));
        object.insert(
            "cell_type".to_string(),
            Value::String(cell_type.as_str().to_string()),
        );
        if cell_type == CellType::Code {
            object.insert("execution_count".to_string(), Value::Null);
            object.insert("outputs".to_string(), Value::Array(Vec::new()));
        } else {
            object.remove("execution_count");
            object.remove("outputs");
        }
        Ok(())
    }

    fn new_cell_id() -> String {
        uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(12)
            .collect()
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
}

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "NotebookEdit"
    }

    async fn description(&self, _input: &Value) -> String {
        "Edits cells in a Jupyter notebook (.ipynb) file.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "The absolute path to the Jupyter notebook file to edit"
                },
                "cell_id": {
                    "type": "string",
                    "description": "The ID of the cell to edit. For insert, the new cell is inserted after this cell, or at the beginning if omitted."
                },
                "new_source": {
                    "type": "string",
                    "description": "The new source for the cell"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown"],
                    "description": "The cell type. Required when edit_mode is insert."
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "default": "replace",
                    "description": "The type of edit to make"
                }
            },
            "required": ["notebook_path", "new_source"]
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
            .get("notebook_path")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        let notebook_path = input
            .get("notebook_path")
            .and_then(Value::as_str)
            .unwrap_or("");
        let edit_mode = input
            .get("edit_mode")
            .and_then(Value::as_str)
            .unwrap_or("replace");
        let new_source = input
            .get("new_source")
            .and_then(Value::as_str)
            .unwrap_or("");
        Value::String(format!("{notebook_path} {edit_mode}: {new_source}"))
    }

    fn backfill_observable_input(&self, input: &mut Map<String, Value>) {
        if !input.contains_key("file_path") {
            if let Some(path) = input.get("notebook_path").cloned() {
                input.insert("file_path".to_string(), path);
            }
        }
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        let parsed = match Self::parse_input(input) {
            Ok(parsed) => parsed,
            Err(message) => {
                return ValidationResult::Error {
                    message,
                    error_code: 1,
                };
            }
        };

        let path = Path::new(&parsed.notebook_path);
        if path.extension().and_then(|ext| ext.to_str()) != Some("ipynb") {
            return ValidationResult::Error {
                message: "File must be a Jupyter notebook (.ipynb file). For editing other file types, use the Edit tool.".to_string(),
                error_code: 2,
            };
        }

        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return ValidationResult::Error {
                    message: "Notebook file does not exist.".to_string(),
                    error_code: 3,
                };
            }
            Err(err) => {
                return ValidationResult::Error {
                    message: format!("Failed to read notebook: {}", err),
                    error_code: 3,
                };
            }
        };

        if let Err(message) = Self::validate_cached_read(ctx, &parsed.notebook_path, path, &content)
        {
            return ValidationResult::Error {
                message: message.to_string(),
                error_code: 9,
            };
        }
        if let Err(message) = Self::validate_file_writable(path) {
            return ValidationResult::Error {
                message,
                error_code: 10,
            };
        }

        let notebook = match Self::parse_notebook(&content) {
            Ok(notebook) => notebook,
            Err(message) => {
                return ValidationResult::Error {
                    message,
                    error_code: 6,
                };
            }
        };
        Self::validate_cell_reference(&notebook, &parsed)
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
            Err(message) => {
                return Ok(ToolResult {
                    data: json!({ "error": message }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };
        let path = Path::new(&parsed.notebook_path);
        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(err) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to read notebook: {}", err) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        if let Err(message) = Self::validate_cached_read(ctx, &parsed.notebook_path, path, &content)
        {
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

        let notebook = match Self::parse_notebook(&content) {
            Ok(notebook) => notebook,
            Err(message) => {
                return Ok(ToolResult {
                    data: json!({ "error": message }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };
        if let ValidationResult::Error { message, .. } =
            Self::validate_cell_reference(&notebook, &parsed)
        {
            return Ok(ToolResult {
                data: json!({ "error": message }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let outcome = match Self::apply_edit(notebook, &parsed) {
            Ok(outcome) => outcome,
            Err(message) => {
                return Ok(ToolResult {
                    data: json!({ "error": message }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };
        let updated_content = serde_json::to_string_pretty(&outcome.notebook)?;
        let safe_options = SafeWriteOptions {
            max_bytes: MAX_NOTEBOOK_BYTES,
            session_id: Some(ctx.session_id.clone()),
            #[cfg(test)]
            recovery_root: std::env::var_os("ALLTHECODES_TEST_RECOVERY_ROOT")
                .map(std::path::PathBuf::from),
            ..Default::default()
        };
        let notebook_path_for_write = parsed.notebook_path.clone();
        let updated_content_for_write = updated_content.clone();
        let write_report = match tokio::task::spawn_blocking(move || {
            safe_write_text(
                notebook_path_for_write,
                &updated_content_for_write,
                &safe_options,
            )
        })
        .await
        {
            Ok(Ok(report)) => report,
            Ok(Err(err)) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to write notebook safely: {}", err) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
            Err(err) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Safe notebook write task failed: {}", err) }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        Self::record_edit_state(ctx, &parsed.notebook_path, path, &updated_content);
        {
            let app_state = (ctx.get_app_state)();
            let configs =
                allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "FileChanged");
            if !configs.is_empty() {
                let payload = json!({
                    "file_path": &parsed.notebook_path,
                    "operation": "notebook_edit",
                    "edit_mode": outcome.edit_mode.as_str(),
                    "cell_id": outcome.cell_id,
                    "cell_type": outcome.cell_type.as_str(),
                    "safe_write": {
                        "atomic": true,
                        "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                    },
                });
                let _ = crate::hooks::run_event_hooks("FileChanged", &payload, &configs).await;
            }
        }

        let output = match outcome.edit_mode {
            EditMode::Replace => format!(
                "Updated notebook cell {} in {}",
                outcome.cell_id.as_deref().unwrap_or("cell"),
                parsed.notebook_path
            ),
            EditMode::Insert => format!(
                "Inserted notebook cell {} in {}",
                outcome.cell_id.as_deref().unwrap_or("cell"),
                parsed.notebook_path
            ),
            EditMode::Delete => format!(
                "Deleted notebook cell {} in {}",
                parsed.cell_id.as_deref().unwrap_or("cell"),
                parsed.notebook_path
            ),
        };
        let hunk_lines =
            Self::unified_hunk_lines(&parsed.notebook_path, &content, &updated_content);
        let display_preview = json!({
            "kind": "notebook_edit",
            "tool": "NotebookEdit",
            "path": &parsed.notebook_path,
            "output": &output,
            "edit_mode": outcome.edit_mode.as_str(),
            "cell_id": outcome.cell_id,
            "cell_type": outcome.cell_type.as_str(),
            "hunk_lines": hunk_lines,
            "edit_history": {
                "backup_path": write_report.backup_path.as_ref().map(|p| p.display().to_string()),
                "atomic": true,
                "permissions_preserved": write_report.permissions_preserved,
            },
        })
        .to_string();

        Ok(ToolResult {
            data: json!({
                "new_source": parsed.new_source,
                "cell_id": outcome.cell_id,
                "cell_type": outcome.cell_type.as_str(),
                "language": outcome.language,
                "edit_mode": outcome.edit_mode.as_str(),
                "error": "",
                "notebook_path": parsed.notebook_path,
                "original_file": content,
                "updated_file": updated_content,
            }),
            model_content: Some(ToolResultContent::Text(output)),
            display_preview: Some(display_preview),
            new_messages: vec![super::edited_text_file_message(
                path.to_string_lossy().to_string(),
            )],
        })
    }

    async fn prompt(&self) -> String {
        "Edits cells in Jupyter notebook (.ipynb) files.\n\n\
Usage:\n\
- Use this for .ipynb files instead of editing raw notebook JSON with Edit.\n\
- You must use Read on the notebook before editing; this tool rejects stale edits.\n\
- Use edit_mode=replace to replace a cell, insert to insert after a cell, and delete to remove a cell.\n\
- cell_id may be the notebook cell id or a cell-N zero-based index from notebook read output.\n\
- Use cell_type=code or cell_type=markdown when inserting, or when changing the target cell type.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Edit Notebook".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::file_read::FileReadTool;
    use crate::tool::ToolAppState as AppState;
    use crate::tool::ToolUseOptions;
    use allthecodes_types::message::ContentBlock;
    use std::ffi::OsString;
    use std::sync::Arc;
    use tempfile::tempdir;
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
            session_id: "notebook-edit-test-session".to_string(),
            langfuse_session_id: "notebook-edit-test-session".to_string(),
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

    fn notebook_json() -> String {
        json!({
            "cells": [
                {
                    "cell_type": "markdown",
                    "id": "intro",
                    "metadata": {"keep": true},
                    "source": ["# Intro\n"]
                },
                {
                    "cell_type": "code",
                    "id": "code-1",
                    "metadata": {},
                    "source": "print('old')\n",
                    "execution_count": 7,
                    "outputs": [{"output_type": "stream", "text": "old\n"}]
                }
            ],
            "metadata": {"language_info": {"name": "python"}},
            "nbformat": 4,
            "nbformat_minor": 5
        })
        .to_string()
    }

    fn cache_file_state(ctx: &ToolUseContext, path: &Path, content: &str) {
        let timestamp = std::fs::metadata(path)
            .map(|metadata| NotebookEditTool::modified_millis(&metadata))
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
    fn apply_edit_replaces_code_cell_and_clears_outputs() {
        let input = NotebookEditInput {
            notebook_path: "demo.ipynb".to_string(),
            cell_id: Some("code-1".to_string()),
            new_source: "print('new')\n".to_string(),
            cell_type: None,
            edit_mode: EditMode::Replace,
        };
        let notebook = NotebookEditTool::parse_notebook(&notebook_json()).unwrap();
        let outcome = NotebookEditTool::apply_edit(notebook, &input).unwrap();
        let cell = &outcome.notebook["cells"][1];
        assert_eq!(cell["source"], "print('new')\n");
        assert!(cell["execution_count"].is_null());
        assert_eq!(cell["outputs"].as_array().unwrap().len(), 0);
        assert_eq!(cell["metadata"], json!({}));
    }

    #[test]
    fn apply_edit_supports_cell_index_and_type_switch() {
        let input = NotebookEditInput {
            notebook_path: "demo.ipynb".to_string(),
            cell_id: Some("cell-0".to_string()),
            new_source: "print('markdown became code')\n".to_string(),
            cell_type: Some(CellType::Code),
            edit_mode: EditMode::Replace,
        };
        let notebook = NotebookEditTool::parse_notebook(&notebook_json()).unwrap();
        let outcome = NotebookEditTool::apply_edit(notebook, &input).unwrap();
        let cell = &outcome.notebook["cells"][0];
        assert_eq!(cell["cell_type"], "code");
        assert_eq!(cell["source"], "print('markdown became code')\n");
        assert!(cell["execution_count"].is_null());
        assert_eq!(cell["outputs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn apply_edit_inserts_and_deletes_cells() {
        let insert = NotebookEditInput {
            notebook_path: "demo.ipynb".to_string(),
            cell_id: Some("intro".to_string()),
            new_source: "inserted\n".to_string(),
            cell_type: Some(CellType::Markdown),
            edit_mode: EditMode::Insert,
        };
        let notebook = NotebookEditTool::parse_notebook(&notebook_json()).unwrap();
        let outcome = NotebookEditTool::apply_edit(notebook, &insert).unwrap();
        assert_eq!(outcome.notebook["cells"].as_array().unwrap().len(), 3);
        assert_eq!(outcome.notebook["cells"][1]["source"], "inserted\n");
        assert!(outcome.notebook["cells"][1]["id"].is_string());

        let delete = NotebookEditInput {
            notebook_path: "demo.ipynb".to_string(),
            cell_id: Some("cell-1".to_string()),
            new_source: "".to_string(),
            cell_type: None,
            edit_mode: EditMode::Delete,
        };
        let outcome = NotebookEditTool::apply_edit(outcome.notebook, &delete).unwrap();
        assert_eq!(outcome.notebook["cells"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn validates_read_before_edit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("demo.ipynb");
        std::fs::write(&path, notebook_json()).unwrap();
        let ctx = test_context();
        let input = json!({
            "notebook_path": path.to_string_lossy(),
            "cell_id": "code-1",
            "new_source": "print('new')\n"
        });
        let result = NotebookEditTool::new().validate_input(&input, &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { message, .. } if message == FILE_NOT_READ_ERROR
        ));
    }

    #[tokio::test]
    async fn read_then_notebook_edit_refreshes_cache() {
        let dir = tempdir().unwrap();
        let _history = EnvGuard::set_path("ALLTHECODES_TEST_RECOVERY_ROOT", dir.path());
        let path = dir.path().join("demo.ipynb");
        std::fs::write(&path, notebook_json()).unwrap();
        let ctx = test_context();
        let read_input = json!({ "file_path": path.to_string_lossy() });
        FileReadTool::new()
            .call(read_input, &ctx, &parent_message(), None)
            .await
            .unwrap();
        let input = json!({
            "notebook_path": path.to_string_lossy(),
            "cell_id": "code-1",
            "new_source": "print('new')\n"
        });
        let result = NotebookEditTool::new()
            .call(input, &ctx, &parent_message(), None)
            .await
            .unwrap();
        assert_eq!(result.data["error"], "");
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("print('new')"));
        assert!(ctx.read_file_state.get(&path.to_string_lossy()).is_some());
    }

    #[tokio::test]
    async fn stale_read_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("demo.ipynb");
        let original = notebook_json();
        std::fs::write(&path, &original).unwrap();
        let ctx = test_context();
        cache_file_state(&ctx, &path, &original);
        std::fs::write(&path, original.replace("old", "external")).unwrap();
        let input = json!({
            "notebook_path": path.to_string_lossy(),
            "cell_id": "code-1",
            "new_source": "print('new')\n"
        });
        let result = NotebookEditTool::new().validate_input(&input, &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { message, .. } if message == FILE_UNEXPECTEDLY_MODIFIED_ERROR
        ));
    }
}
