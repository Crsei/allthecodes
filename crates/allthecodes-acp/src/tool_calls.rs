//! ACP tool call mapping and kind classification.
//!
//! Maps internal allthecodes tool names to ACP ToolKind and builds
//! ACP ToolCallUpdate objects from engine tool execution data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agent_client_protocol_schema::v2::{
    ContentBlock, Diff, TextContent, ToolCallContent, ToolCallContentChunk, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolKind,
};
use allthecodes_tool_display::{OperationKind, OperationStatus, ToolClassifier};

#[derive(Debug, Clone)]
struct ToolCallContext {
    tool_name: String,
    input: serde_json::Value,
}

/// Per-turn cache of tool-use metadata needed to complete later result/progress updates.
#[derive(Debug, Clone)]
pub struct ToolCallContextCache {
    cwd: PathBuf,
    calls: HashMap<String, ToolCallContext>,
}

impl ToolCallContextCache {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            calls: HashMap::new(),
        }
    }

    pub fn start_tool_call(
        &mut self,
        tool_use_id: impl Into<String>,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> ToolCallUpdate {
        let tool_use_id = tool_use_id.into();
        self.calls.insert(
            tool_use_id.clone(),
            ToolCallContext {
                tool_name: tool_name.to_string(),
                input: input.clone(),
            },
        );
        build_tool_call_update(tool_use_id, tool_name, input, &self.cwd)
    }

    pub fn finish_tool_call(
        &mut self,
        tool_use_id: &str,
        output_text: &str,
        is_error: bool,
    ) -> ToolCallUpdate {
        let status = if is_error {
            ToolCallStatus::Failed
        } else {
            ToolCallStatus::Completed
        };

        let Some(context) = self.calls.get(tool_use_id) else {
            return ToolCallUpdate::new(tool_use_id.to_string())
                .status(status)
                .raw_output(serde_json::Value::String(output_text.to_string()))
                .content(text_tool_content(output_text));
        };

        build_tool_call_update_with_status(
            tool_use_id.to_string(),
            &context.tool_name,
            &context.input,
            &self.cwd,
            status,
            Some(output_text),
            is_error,
        )
    }

    pub fn content_chunk(&self, tool_use_id: &str, text: &str) -> Option<ToolCallContentChunk> {
        if text.is_empty() || !self.calls.contains_key(tool_use_id) {
            return None;
        }
        Some(ToolCallContentChunk::new(
            tool_use_id.to_string(),
            ContentBlock::Text(TextContent::new(text.to_string())),
        ))
    }
}

/// Classify a tool name into an ACP ToolKind.
pub fn classify_tool_kind(tool_name: &str) -> ToolKind {
    let lower = tool_name.to_ascii_lowercase();
    if lower == "read"
        || lower == "glob"
        || lower == "grep"
        || lower == "listfiles"
        || lower == "filesearch"
    {
        ToolKind::Read
    } else if lower == "write"
        || lower == "fileedit"
        || lower == "filewrite"
        || lower == "multiedit"
        || lower == "edit"
    {
        ToolKind::Edit
    } else if lower == "delete" || lower == "remove" || lower == "filedelete" {
        ToolKind::Delete
    } else if lower == "rename" || lower == "move" || lower == "filerename" {
        ToolKind::Move
    } else if lower == "search" || lower == "grepsearch" || lower == "codesearch" {
        ToolKind::Search
    } else if lower == "bash"
        || lower == "shell"
        || lower == "execute"
        || lower == "process"
        || lower == "run"
        || lower == "terminal"
    {
        ToolKind::Execute
    } else if lower == "think" || lower == "plan" || lower == "reason" || lower == "brainstorm" {
        ToolKind::Think
    } else if lower == "webfetch" || lower == "websearch" || lower == "fetch" || lower == "http" {
        ToolKind::Fetch
    } else {
        ToolKind::Other
    }
}

/// Build a set of tool call locations from tool input.
pub fn extract_tool_locations(
    _tool_name: &str,
    input: &serde_json::Value,
    cwd: &Path,
) -> Option<Vec<ToolCallLocation>> {
    let path_fields = [
        "path",
        "file_path",
        "filePath",
        "file",
        "target",
        "location",
        "notebook_path",
    ];

    let paths: Vec<ToolCallLocation> = path_fields
        .iter()
        .filter_map(|field| input.get(*field))
        .filter_map(|v| v.as_str())
        .filter(|p| !p.trim().is_empty())
        .filter(|p| url::Url::parse(p).map_or(true, |url| url.scheme() == "file"))
        .map(|p| {
            let path = if let Ok(url) = url::Url::parse(p) {
                url.to_file_path().unwrap_or_else(|_| PathBuf::from(p))
            } else {
                PathBuf::from(p)
            };
            if path.is_relative() {
                cwd.join(path)
            } else {
                path
            }
        })
        .map(ToolCallLocation::new)
        .collect();

    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// Build an ACP ToolCallUpdate for an in-progress tool call.
pub fn build_tool_call_update(
    tool_use_id: String,
    tool_name: &str,
    input: &serde_json::Value,
    cwd: &Path,
) -> ToolCallUpdate {
    build_tool_call_update_with_status(
        tool_use_id,
        tool_name,
        input,
        cwd,
        ToolCallStatus::InProgress,
        None,
        false,
    )
}

fn build_tool_call_update_with_status(
    tool_use_id: String,
    tool_name: &str,
    input: &serde_json::Value,
    cwd: &Path,
    status: ToolCallStatus,
    output_text: Option<&str>,
    is_error: bool,
) -> ToolCallUpdate {
    let operation_status = match status {
        ToolCallStatus::Completed => OperationStatus::Resolved,
        ToolCallStatus::Failed => OperationStatus::Error,
        _ => OperationStatus::InProgress,
    };
    let operation = if let Some(output_text) = output_text {
        ToolClassifier::classify_with_result(
            tool_name,
            input,
            operation_status,
            Some(output_text),
            is_error,
        )
    } else {
        ToolClassifier::classify(tool_name, input, operation_status)
    };

    let mut update = ToolCallUpdate::new(tool_use_id)
        .title(operation.label)
        .kind(operation_kind_to_tool_kind(operation.kind, tool_name))
        .status(status)
        .raw_input(input.clone());

    if let Some(locations) = extract_tool_locations(tool_name, input, cwd) {
        update = update.locations(locations);
    }

    if let Some(output_text) = output_text {
        update = update
            .raw_output(serde_json::Value::String(output_text.to_string()))
            .content(tool_result_content(input, output_text));
    }

    update
}

fn operation_kind_to_tool_kind(kind: OperationKind, tool_name: &str) -> ToolKind {
    match kind {
        OperationKind::Read => {
            let fallback = classify_tool_kind(tool_name);
            if fallback == ToolKind::Fetch {
                ToolKind::Fetch
            } else {
                ToolKind::Read
            }
        }
        OperationKind::Search => ToolKind::Search,
        OperationKind::Create | OperationKind::Modify => ToolKind::Edit,
        OperationKind::Delete => ToolKind::Delete,
        OperationKind::Execute => ToolKind::Execute,
        OperationKind::Plan | OperationKind::Status => ToolKind::Think,
        OperationKind::Network => ToolKind::Fetch,
        OperationKind::Permission
        | OperationKind::Delegate
        | OperationKind::System
        | OperationKind::Unknown => classify_tool_kind(tool_name),
    }
}

fn tool_result_content(input: &serde_json::Value, output_text: &str) -> Vec<ToolCallContent> {
    let mut content = text_tool_content(output_text);
    if let Some(diff) = build_diff_content(input) {
        content.push(ToolCallContent::Diff(diff));
    }
    content
}

fn text_tool_content(text: &str) -> Vec<ToolCallContent> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
            text.to_string(),
        )))]
    }
}

fn build_diff_content(input: &serde_json::Value) -> Option<Diff> {
    let path = string_field(input, &["file_path", "path", "filePath"])?;
    let new_text = string_field(input, &["new_string", "new_text", "newText", "content"])?;
    let old_text = string_field(input, &["old_string", "old_text", "oldText"]);
    Some(Diff::new(PathBuf::from(path), new_text).old_text(old_text))
}

fn string_field(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| input.get(*key))
        .filter_map(|value| value.as_str())
        .map(str::to_string)
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tool_classification() {
        assert_eq!(classify_tool_kind("Read"), ToolKind::Read);
        assert_eq!(classify_tool_kind("Glob"), ToolKind::Read);
        assert_eq!(classify_tool_kind("Grep"), ToolKind::Read);
    }

    #[test]
    fn edit_tool_classification() {
        assert_eq!(classify_tool_kind("Write"), ToolKind::Edit);
        assert_eq!(classify_tool_kind("FileEdit"), ToolKind::Edit);
    }

    #[test]
    fn execute_tool_classification() {
        assert_eq!(classify_tool_kind("Bash"), ToolKind::Execute);
    }

    #[test]
    fn think_tool_classification() {
        assert_eq!(classify_tool_kind("Think"), ToolKind::Think);
    }

    #[test]
    fn web_tool_classification() {
        assert_eq!(classify_tool_kind("WebFetch"), ToolKind::Fetch);
    }

    #[test]
    fn unknown_tool_classification() {
        assert_eq!(classify_tool_kind("SomeUnknownTool"), ToolKind::Other);
    }

    #[test]
    fn read_tool_location_is_absolute() {
        let cwd = std::path::Path::new("/test/project");
        let input = serde_json::json!({"path": "src/main.rs"});
        let locations = extract_tool_locations("Read", &input, cwd);
        assert!(locations.is_some());
        let locs = locations.unwrap();
        assert!(locs[0].path.starts_with("/test/project"));
    }
}
