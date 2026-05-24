//! Shared data structures and lookup helpers for user-tool-result messages.

use allthecodes_engine::types::tool::Tool;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Copied from upstream shared message constants.
pub const INTERRUPT_MESSAGE_FOR_TOOL_USE: &str = "[Request interrupted by user for tool use]";
/// Copied from upstream shared message constants.
pub const CANCEL_MESSAGE: &str = "The user doesn't want to take this action right now. STOP what you are doing and wait for the user to tell you how to proceed.";
/// Copied from upstream shared message constants.
pub const REJECT_MESSAGE: &str = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.";
/// Copied from upstream shared message constants.
pub const PLAN_REJECTION_PREFIX: &str = "The agent proposed a plan that was rejected by the user. The user chose to stay in plan mode rather than proceed with implementation.\n\nRejected plan:\n";
/// Copied from upstream shared message constants.
pub const REJECT_MESSAGE_WITH_REASON_PREFIX: &str = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\n";
/// Classifier-denial marker used by the tool-call error formatter.
pub const CLASSIFIER_DENIAL_PREFIX: &str = "Permission for this action has been denied. Reason: ";

/// Tool-call block payload used by the renderer.
#[derive(Debug, Clone)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    pub is_error: bool,
    pub content: String,
    pub tool_use_result: Option<String>,
}

impl ToolResultBlock {
    pub fn new(tool_use_id: impl Into<String>, is_error: bool, content: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            is_error,
            content: content.into(),
            tool_use_result: None,
        }
    }

    #[cfg(test)]
    pub fn with_tool_use_result(mut self, tool_use_result: impl Into<String>) -> Self {
        self.tool_use_result = Some(tool_use_result.into());
        self
    }
}

/// Snapshot of tool usage by `tool_use_id` used to recover tool input/name.
#[derive(Debug, Clone, Default)]
pub struct UserToolResultLookups {
    pub tool_use_by_tool_use_id: HashMap<String, ToolUseRecord>,
}

impl UserToolResultLookups {
    pub fn new() -> Self {
        Self {
            tool_use_by_tool_use_id: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolUseRecord {
    pub tool_name: String,
    pub input: Value,
}

/// Result of `find_tool_from_messages`.
pub struct ToolResolution {
    pub tool: Arc<dyn Tool>,
    pub tool_use_id: String,
    pub input: Value,
}

/// Locate the tool instance and original tool-use input for a message id.
pub fn find_tool_from_messages(
    tool_use_id: &str,
    tools: &[Arc<dyn Tool>],
    lookups: &UserToolResultLookups,
) -> Option<ToolResolution> {
    let record = lookups.tool_use_by_tool_use_id.get(tool_use_id)?;
    let tool = tools
        .iter()
        .find(|tool| tool.name() == record.tool_name)
        .cloned()?;

    Some(ToolResolution {
        tool,
        tool_use_id: tool_use_id.to_string(),
        input: record.input.clone(),
    })
}

/// Prefer explicit tool-facing name and fall back to a generic label.
pub fn tool_display_name(tool: Option<&dyn Tool>, input: Option<&Value>) -> String {
    match tool {
        Some(tool) => tool.user_facing_name(input).trim().to_string(),
        None => "Tool".to_string(),
    }
}

/// Render string content as pretty JSON when parseable, else return raw text.
pub(crate) fn format_tool_output(raw: &str) -> String {
    match serde_json::from_str::<Value>(raw) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string()),
        Err(_) => raw.to_string(),
    }
}

/// Render a stable one-line text representation for testing/snapshots.
pub(crate) fn line_to_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_resolution_preserves_tool_use_id() {
        let resolution = ToolResolution {
            tool: Arc::new(allthecodes_tools::tasks::TodoWriteTool),
            tool_use_id: "toolu_1".to_string(),
            input: serde_json::json!({ "file_path": "src/main.rs" }),
        };

        assert_eq!(resolution.tool_use_id, "toolu_1");
        assert_eq!(resolution.input["file_path"], "src/main.rs");
        assert!(!resolution.tool.name().is_empty());
    }
}
