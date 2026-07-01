use serde::{Deserialize, Serialize};

/// Semantic kind of a tool operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
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
