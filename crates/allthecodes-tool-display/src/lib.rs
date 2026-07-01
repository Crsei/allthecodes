//! # allthecodes-tool-display
//!
//! Shared operation classifier and display model for tool calls.
//!
//! This crate provides the semantic operation model described in the
//! command-operation-display plan. It replaces the ad-hoc grouping and
//! tool-name mapping scattered across the TUI with a centralized classifier
//! that both the TUI and Web/IPC layers can share.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use allthecodes_tool_display::*;
//!
//! let op = ToolClassifier::classify(
//!     "Bash",
//!     &serde_json::json!({"command": "rm -rf /tmp/x"}),
//!     OperationStatus::InProgress,
//! );
//! assert_eq!(op.kind, OperationKind::Delete);
//! assert_eq!(op.risk, OperationRisk::Destructive);
//! ```

mod classifier;
mod result_summary;
mod shell_heuristic;

pub use allthecodes_types::tool_operation::{
    OperationConfidence, OperationKind, OperationResultSummary, OperationRisk,
    OperationSideChannel, OperationStatus, OperationSubtype, ToolOperation,
};
pub use classifier::ToolClassifier;
pub use result_summary::{json_unwrap_text, summarize_result, JsonUnwrapStrategy};
pub use shell_heuristic::{shell_command_operation, ShellClassification};
