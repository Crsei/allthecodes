//! ACP tool call mapping and kind classification.
//!
//! Maps internal allthecodes tool names to ACP ToolKind and builds
//! ACP ToolCallUpdate objects from engine tool execution data.

use agent_client_protocol_schema::v2::{ToolCallUpdate, ToolCallStatus, ToolKind};

/// Classify a tool name into an ACP ToolKind.
pub fn classify_tool_kind(tool_name: &str) -> ToolKind {
    let lower = tool_name.to_ascii_lowercase();
    if lower == "read" || lower == "glob" || lower == "grep" || lower == "listfiles" || lower == "filesearch" {
        ToolKind::Read
    } else if lower == "write" || lower == "fileedit" || lower == "filewrite" || lower == "multiedit" || lower == "edit" {
        ToolKind::Edit
    } else if lower == "delete" || lower == "remove" || lower == "filedelete" {
        ToolKind::Delete
    } else if lower == "rename" || lower == "move" || lower == "filerename" {
        ToolKind::Move
    } else if lower == "search" || lower == "grepsearch" || lower == "codesearch" {
        ToolKind::Search
    } else if lower == "bash" || lower == "shell" || lower == "execute" || lower == "process" || lower == "run" || lower == "terminal" {
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
    tool_name: &str,
    input: &serde_json::Value,
    cwd: &std::path::Path,
) -> Option<Vec<String>> {
    let path_fields = ["path", "file_path", "file", "target", "location"];

    let paths: Vec<String> = path_fields.iter()
        .filter_map(|field| input.get(*field))
        .filter_map(|v| v.as_str())
        .map(|p| {
            let path = std::path::Path::new(p);
            if path.is_relative() {
                cwd.join(path).to_string_lossy().to_string()
            } else {
                path.to_string_lossy().to_string()
            }
        })
        .collect();

    if paths.is_empty() { None } else { Some(paths) }
}

/// Build an ACP ToolCallUpdate for an in-progress tool call.
pub fn build_tool_call_update(
    tool_use_id: String,
    tool_name: &str,
    input: &serde_json::Value,
    cwd: &std::path::Path,
) -> ToolCallUpdate {
    let kind = classify_tool_kind(tool_name);
    let _locations = extract_tool_locations(tool_name, input, cwd);

    ToolCallUpdate::new(tool_use_id)
        .kind(kind)
        .status(ToolCallStatus::InProgress)
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
        assert!(locs[0].starts_with("/test/project"));
    }
}
