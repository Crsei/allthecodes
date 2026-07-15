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
    OperationSideChannel, OperationStatus, OperationSubtype, ToolOperation, ToolOperationDisplay,
};
pub use classifier::ToolClassifier;
pub use result_summary::{json_unwrap_text, summarize_result, JsonUnwrapStrategy};
pub use shell_heuristic::{shell_command_operation, ShellClassification};

/// Return the same normalized permission classification used by every UI
/// transport. A supplied classification wins; otherwise the shared classifier
/// derives one from the request fields.
pub fn normalize_permission_operation(
    tool_name: &str,
    input: &serde_json::Value,
    message: &str,
    supplied: Option<&ToolOperation>,
) -> ToolOperation {
    supplied.cloned().unwrap_or_else(|| {
        ToolClassifier::classify_permission(
            tool_name,
            input,
            Some(message),
            OperationStatus::InProgress,
        )
    })
}

/// Produce a transport-safe permission operation without raw input/output or
/// side-channel references.
pub fn permission_operation_display(
    tool_name: &str,
    input: &serde_json::Value,
    message: &str,
    supplied: Option<&ToolOperation>,
) -> ToolOperationDisplay {
    ToolOperationDisplay::from(&normalize_permission_operation(
        tool_name, input, message, supplied,
    ))
}

#[cfg(test)]
mod permission_projection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_operation_uses_shared_permission_classifier() {
        let input = json!({"command": "cargo test"});
        let operation = normalize_permission_operation("Bash", &input, "run tests", None);
        assert_eq!(operation.kind, OperationKind::Permission);
        assert_eq!(operation.status, OperationStatus::InProgress);
    }

    #[test]
    fn safe_projection_drops_raw_fields_from_supplied_operation() {
        let input = json!({"command": "secret"});
        let operation = normalize_permission_operation("Bash", &input, "run", None);
        let display = permission_operation_display("Bash", &input, "run", Some(&operation));
        let json = serde_json::to_value(display).unwrap();
        assert!(json.get("raw_input").is_none());
        assert!(json.get("raw_output").is_none());
        assert!(json.get("side_channels").is_none());
    }
}
