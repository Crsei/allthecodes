//! Structured protocol messages for teammate IPC.
//!
//! Corresponds to TypeScript: mailbox structured message types in
//! `utils/teammateMailbox.ts` and `tools/SendMessageTool/`.
//!
//! Messages are serialized as JSON in the mailbox `text` field.
//! The `type` field discriminates between protocol messages and plain text.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Known protocol message types
// ---------------------------------------------------------------------------

/// All known structured protocol message type strings.
pub const KNOWN_PROTOCOL_TYPES: &[&str] = &[
    "permission_request",
    "permission_response",
    "sandbox_permission_request",
    "sandbox_permission_response",
    "shutdown_request",
    "shutdown_approved",
    "shutdown_rejected",
    "team_permission_update",
    "mode_set_request",
    "plan_approval_request",
    "plan_approval_response",
    "idle_notification",
    "task_assignment",
];

// ---------------------------------------------------------------------------
// Protocol message enum
// ---------------------------------------------------------------------------

/// A structured protocol message that can appear in a mailbox's `text` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    // ── Shutdown ─────────────────────────────────────────────────────
    ShutdownRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        reason: Option<String>,
        timestamp: String,
    },
    ShutdownApproved {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        timestamp: String,
        #[serde(rename = "paneId")]
        pane_id: Option<String>,
        #[serde(rename = "backendType")]
        backend_type: Option<String>,
    },
    ShutdownRejected {
        #[serde(rename = "requestId")]
        request_id: String,
        from: String,
        reason: Option<String>,
        timestamp: String,
    },

    // ── Plan approval ────────────────────────────────────────────────
    PlanApprovalRequest {
        from: String,
        timestamp: String,
        #[serde(rename = "planFilePath")]
        plan_file_path: Option<String>,
        #[serde(rename = "planContent")]
        plan_content: Option<String>,
        #[serde(rename = "requestId")]
        request_id: String,
    },
    PlanApprovalResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        approved: bool,
        feedback: Option<String>,
        timestamp: String,
        #[serde(rename = "permissionMode")]
        permission_mode: Option<String>,
    },

    // ── Permissions ──────────────────────────────────────────────────
    PermissionRequest {
        request_id: String,
        agent_id: String,
        tool_name: String,
        tool_use_id: String,
        description: Option<String>,
        input: Option<Value>,
        #[serde(default)]
        permission_suggestions: Vec<Value>,
    },
    PermissionResponse {
        request_id: String,
        subtype: String, // "success" | "error"
        response: Option<Value>,
    },

    // ── Idle notification ────────────────────────────────────────────
    IdleNotification {
        from: String,
        timestamp: String,
        #[serde(rename = "idleReason")]
        idle_reason: String,
        summary: Option<String>,
        #[serde(rename = "completedTaskId")]
        completed_task_id: Option<String>,
        #[serde(rename = "completedStatus")]
        completed_status: Option<String>,
    },

    // ── Task assignment ──────────────────────────────────────────────
    TaskAssignment {
        #[serde(rename = "taskId")]
        task_id: String,
        subject: Option<String>,
        description: Option<String>,
        #[serde(rename = "assignedBy")]
        assigned_by: String,
    },

    // ── Team permission update ───────────────────────────────────────
    TeamPermissionUpdate {
        #[serde(rename = "permissionUpdate")]
        permission_update: Value,
        #[serde(rename = "directoryPath")]
        directory_path: Option<String>,
        #[serde(rename = "toolName")]
        tool_name: Option<String>,
    },

    // ── Mode set request ─────────────────────────────────────────────
    ModeSetRequest {
        mode: String,
        from: String,
    },

    // ── Sandbox permission ───────────────────────────────────────────
    SandboxPermissionRequest {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "workerId")]
        worker_id: Option<String>,
        #[serde(rename = "hostPattern")]
        host_pattern: Option<Value>,
    },
    SandboxPermissionResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        approved: bool,
    },
}

// ---------------------------------------------------------------------------
// Discrimination
// ---------------------------------------------------------------------------

/// Check if a text string is a structured protocol message.
///
/// Tries to parse as JSON and checks the `type` field against known types.
///
/// Corresponds to TS: `isStructuredProtocolMessage(text)`
pub fn is_structured_protocol_message(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            v.get("type")?
                .as_str()
                .map(|t| KNOWN_PROTOCOL_TYPES.contains(&t))
        })
        .unwrap_or(false)
}

/// Try to parse a text string into a `ProtocolMessage`.
///
/// Returns `None` for plain text messages.
pub fn try_parse_protocol_message(text: &str) -> Option<ProtocolMessage> {
    serde_json::from_str(text).ok()
}

// ---------------------------------------------------------------------------
// Message formatting for conversation injection
// ---------------------------------------------------------------------------

/// Format a teammate message as XML for injection into a conversation.
///
/// ```xml
/// <teammate-message from="researcher" color="blue" timestamp="...">
/// Message content here
/// </teammate-message>
/// ```
pub fn format_teammate_xml(from: &str, color: Option<&str>, timestamp: &str, text: &str) -> String {
    let color_attr = color
        .map(|c| format!(" color=\"{}\"", c))
        .unwrap_or_default();
    format!(
        "<teammate-message from=\"{}\"{}  timestamp=\"{}\">\n{}\n</teammate-message>",
        from, color_attr, timestamp, text
    )
}

// ---------------------------------------------------------------------------
// Helpers for creating protocol messages
// ---------------------------------------------------------------------------

/// Create a shutdown request ID.
pub fn shutdown_request_id(agent_id: &str, timestamp: i64) -> String {
    format!("shutdown-{}-{}", agent_id, timestamp)
}

/// Create a plan approval request ID.
pub fn plan_approval_request_id(agent_name: &str, timestamp: i64) -> String {
    format!("plan_approval-{}-{}", agent_name, timestamp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_structured_protocol_message() {
        assert!(is_structured_protocol_message(
            r#"{"type":"shutdown_request","requestId":"x","from":"lead","timestamp":"t"}"#
        ));
        assert!(is_structured_protocol_message(
            r#"{"type":"idle_notification","from":"r","timestamp":"t","idleReason":"available"}"#
        ));
        assert!(!is_structured_protocol_message("hello plain text"));
        assert!(!is_structured_protocol_message(
            r#"{"type":"unknown_type"}"#
        ));
        assert!(!is_structured_protocol_message(r#"{"no_type":true}"#));
    }

    #[test]
    fn test_parse_shutdown_request() {
        let json = r#"{
            "type": "shutdown_request",
            "requestId": "shutdown-researcher@team-123",
            "from": "team-lead",
            "reason": "Task completed",
            "timestamp": "2026-04-01T12:00:00Z"
        }"#;
        let msg = try_parse_protocol_message(json).unwrap();
        match msg {
            ProtocolMessage::ShutdownRequest {
                request_id,
                from,
                reason,
                ..
            } => {
                assert_eq!(request_id, "shutdown-researcher@team-123");
                assert_eq!(from, "team-lead");
                assert_eq!(reason.unwrap(), "Task completed");
            }
            _ => panic!("Expected ShutdownRequest"),
        }
    }

    #[test]
    fn test_parse_idle_notification() {
        let json = r#"{
            "type": "idle_notification",
            "from": "researcher",
            "timestamp": "2026-04-01T12:30:00Z",
            "idleReason": "available",
            "summary": "Done analyzing endpoints"
        }"#;
        let msg = try_parse_protocol_message(json).unwrap();
        match msg {
            ProtocolMessage::IdleNotification {
                from,
                idle_reason,
                summary,
                ..
            } => {
                assert_eq!(from, "researcher");
                assert_eq!(idle_reason, "available");
                assert_eq!(summary.unwrap(), "Done analyzing endpoints");
            }
            _ => panic!("Expected IdleNotification"),
        }
    }

    #[test]
    fn test_parse_plan_approval_response() {
        let json = r#"{
            "type": "plan_approval_response",
            "requestId": "plan_approval-researcher-123",
            "approved": true,
            "feedback": "Looks good",
            "timestamp": "2026-04-01T13:00:00Z",
            "permissionMode": "default"
        }"#;
        let msg = try_parse_protocol_message(json).unwrap();
        match msg {
            ProtocolMessage::PlanApprovalResponse {
                approved, feedback, ..
            } => {
                assert!(approved);
                assert_eq!(feedback.unwrap(), "Looks good");
            }
            _ => panic!("Expected PlanApprovalResponse"),
        }
    }

    #[test]
    fn test_format_teammate_xml() {
        let xml = format_teammate_xml(
            "researcher",
            Some("blue"),
            "2026-04-01T12:00:00Z",
            "Found the bug",
        );
        assert!(xml.contains("from=\"researcher\""));
        assert!(xml.contains("color=\"blue\""));
        assert!(xml.contains("Found the bug"));
        assert!(xml.starts_with("<teammate-message"));
        assert!(xml.ends_with("</teammate-message>"));
    }

    #[test]
    fn test_format_teammate_xml_no_color() {
        let xml = format_teammate_xml("worker", None, "t", "hello");
        assert!(!xml.contains("color="));
    }

    #[test]
    fn test_shutdown_request_id() {
        let id = shutdown_request_id("researcher@team", 1719000000);
        assert_eq!(id, "shutdown-researcher@team-1719000000");
    }

    #[test]
    fn test_plan_approval_request_id() {
        let id = plan_approval_request_id("researcher", 1719000000);
        assert_eq!(id, "plan_approval-researcher-1719000000");
    }

    #[test]
    fn test_known_protocol_types_completeness() {
        assert!(KNOWN_PROTOCOL_TYPES.contains(&"shutdown_request"));
        assert!(KNOWN_PROTOCOL_TYPES.contains(&"permission_request"));
        assert!(KNOWN_PROTOCOL_TYPES.contains(&"idle_notification"));
        assert!(KNOWN_PROTOCOL_TYPES.contains(&"task_assignment"));
        assert_eq!(KNOWN_PROTOCOL_TYPES.len(), 13);
    }
}
