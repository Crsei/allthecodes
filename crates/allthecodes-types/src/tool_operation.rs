use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Semantic kind of a tool operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Read,
    Search,
    Create,
    Modify,
    Delete,
    Execute,
    Permission,
    Plan,
    Status,
    Delegate,
    Network,
    System,
    Unknown,
}

impl OperationKind {
    /// A short, user-facing label for display in the TUI.
    pub fn label(self) -> &'static str {
        match self {
            OperationKind::Read => "Read",
            OperationKind::Search => "Search",
            OperationKind::Create => "Create",
            OperationKind::Modify => "Modify",
            OperationKind::Delete => "Delete",
            OperationKind::Execute => "Run",
            OperationKind::Permission => "Permission",
            OperationKind::Plan => "Plan",
            OperationKind::Status => "Status",
            OperationKind::Delegate => "Delegate",
            OperationKind::Network => "Network",
            OperationKind::System => "System",
            OperationKind::Unknown => "Unknown",
        }
    }
}

/// Finer-grained subtype of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationSubtype {
    Build,
    Test,
    Format,
    Install,
    Shell,
    Mcp,
    Todo,
    Task,
}

impl OperationSubtype {
    pub fn label(self) -> &'static str {
        match self {
            OperationSubtype::Build => "Build",
            OperationSubtype::Test => "Test",
            OperationSubtype::Format => "Format",
            OperationSubtype::Install => "Install",
            OperationSubtype::Shell => "Shell",
            OperationSubtype::Mcp => "MCP",
            OperationSubtype::Todo => "TODO",
            OperationSubtype::Task => "Task",
        }
    }
}

/// Risk level of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationRisk {
    /// Safe read-only operations.
    Safe,
    /// Low-risk operations.
    Low,
    /// Medium-risk operations.
    Medium,
    /// High-risk operations such as network access or installs.
    High,
    /// Destructive operations such as deletes.
    Destructive,
}

impl OperationRisk {
    pub fn label(self) -> &'static str {
        match self {
            OperationRisk::Safe => "safe",
            OperationRisk::Low => "low",
            OperationRisk::Medium => "medium",
            OperationRisk::High => "high",
            OperationRisk::Destructive => "destructive",
        }
    }
}

/// Confidence in the operation classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationConfidence {
    /// The classifier is certain based on tool name or explicit signal.
    High,
    /// The classifier used heuristics.
    Medium,
    /// The classifier could not reliably determine the operation.
    Low,
}

/// Execution status of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// The tool call has been issued but not yet completed.
    InProgress,
    /// The tool call completed successfully.
    Resolved,
    /// The tool call completed with an error.
    Error,
    /// The tool call was cancelled.
    Cancelled,
}

impl OperationStatus {
    pub fn label(self) -> &'static str {
        match self {
            OperationStatus::InProgress => "in progress",
            OperationStatus::Resolved => "done",
            OperationStatus::Error => "error",
            OperationStatus::Cancelled => "cancelled",
        }
    }
}

/// A summarized view of a tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationResultSummary {
    /// One-line summary text.
    pub text: String,
    /// How many lines of output were produced.
    pub output_lines: usize,
    /// Non-zero exit code when the backend exposes one.
    pub exit_code: Option<i32>,
    /// Whether stderr was observed as a distinct channel.
    pub has_stderr: bool,
    /// Whether the output was truncated.
    pub truncated: bool,
    /// For file reads: number of lines read.
    pub file_lines: Option<usize>,
    /// For search: number of files matched.
    pub search_file_count: Option<usize>,
    /// For search: number of matches.
    pub search_match_count: Option<usize>,
    /// For edits: lines added.
    pub lines_added: Option<usize>,
    /// For edits: lines removed.
    pub lines_removed: Option<usize>,
}

/// A side-channel reference such as an image, preview URL, or diff source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationSideChannel {
    pub channel_type: String,
    pub reference: String,
    pub description: Option<String>,
}

/// A fully classified tool operation for display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOperation {
    /// The semantic kind of operation.
    pub kind: OperationKind,
    /// Optional subtype.
    pub subtype: Option<OperationSubtype>,
    /// Current execution status.
    pub status: OperationStatus,
    /// Estimated risk level.
    pub risk: OperationRisk,
    /// How confident the classifier is.
    pub confidence: OperationConfidence,
    /// User-facing label.
    pub label: String,
    /// Primary target of the operation.
    pub target: Option<String>,
    /// Short summary of the command or input.
    pub command_summary: Option<String>,
    /// Summarized result, when available.
    pub result_summary: Option<OperationResultSummary>,
    /// Original raw tool name.
    pub raw_tool_name: String,
    /// Original raw input.
    pub raw_input: serde_json::Value,
    /// Original raw output, when available.
    pub raw_output: Option<serde_json::Value>,
    /// Side-channel references.
    pub side_channels: Vec<OperationSideChannel>,
}

/// Display-safe projection of a classified tool operation.
///
/// This intentionally omits raw input/output, result bodies, and side-channel
/// references. Transports that already expose their own bounded input field can
/// add this projection without accidentally widening the data surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ToolOperationDisplay {
    pub kind: OperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<OperationSubtype>,
    pub status: OperationStatus,
    pub risk: OperationRisk,
    pub confidence: OperationConfidence,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_summary: Option<String>,
    pub raw_tool_name: String,
}

impl From<&ToolOperation> for ToolOperationDisplay {
    fn from(operation: &ToolOperation) -> Self {
        Self {
            kind: operation.kind,
            subtype: operation.subtype,
            status: operation.status,
            risk: operation.risk,
            confidence: operation.confidence,
            label: operation.label.clone(),
            target: operation.target.clone(),
            command_summary: operation.command_summary.clone(),
            raw_tool_name: operation.raw_tool_name.clone(),
        }
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_projection_omits_raw_and_side_channel_data() {
        let operation = ToolOperation {
            kind: OperationKind::Execute,
            subtype: Some(OperationSubtype::Shell),
            status: OperationStatus::InProgress,
            risk: OperationRisk::High,
            confidence: OperationConfidence::High,
            label: "Run command".to_string(),
            target: Some("workspace".to_string()),
            command_summary: Some("redacted command".to_string()),
            result_summary: None,
            raw_tool_name: "Bash".to_string(),
            raw_input: json!({"secret": "do-not-project"}),
            raw_output: Some(json!({"secret": "do-not-project"})),
            side_channels: vec![OperationSideChannel {
                channel_type: "path".to_string(),
                reference: "/private/live-channel".to_string(),
                description: None,
            }],
        };

        let value = serde_json::to_value(ToolOperationDisplay::from(&operation)).unwrap();
        assert!(value.get("raw_input").is_none());
        assert!(value.get("raw_output").is_none());
        assert!(value.get("side_channels").is_none());
        assert_eq!(value["raw_tool_name"], "Bash");
    }
}
