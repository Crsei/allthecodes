use std::collections::HashMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_types::callbacks::SecurityDecisionDisplay;
use allthecodes_types::tool_operation::ToolOperationDisplay;

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ChatRequest {
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AbortRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ChatPermissionResponseRequest {
    pub session_id: String,
    pub decision: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,

    /// Opaque single-use binding emitted with the pending permission event.
    /// Legacy normal-permission responses may omit it; exact approvals require
    /// a matching value before an allow decision can be consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_binding: Option<String>,
}

/// Typed SSE projection emitted while a chat request is awaiting permission.
///
/// The legacy fields remain required and unchanged. The additive operation and
/// security fields are deliberately display-safe, while `response_binding` is
/// an opaque one-use nonce rather than a digest of request input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ChatPermissionRequestEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub session_id: String,
    pub tool_use_id: String,
    pub tool: String,
    pub command: String,
    pub input: Value,
    pub options: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<ToolOperationDisplay>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityDecisionDisplay>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_binding: Option<String>,
}

// ---------------------------------------------------------------------------
// State / Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct UsageResponse {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub api_call_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CommandInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct StateResponse {
    pub model: String,
    pub session_id: String,
    pub workspace_key: String,
    pub default_chat_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
    pub tools: Vec<String>,
    pub permission_mode: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_enabled: Option<bool>,

    pub fast_mode: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,

    pub usage: UsageResponse,
    pub commands: Vec<CommandInfo>,
    pub settings_map: HashMap<String, Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub settings_sources: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings_diagnostics: Vec<String>,
    pub effective_system_prompt: String,
    pub version: String,
    pub capabilities: HashMap<String, bool>,
}

// ---------------------------------------------------------------------------
// System Prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SystemPromptResponse {
    pub prompt: String,
}

// ---------------------------------------------------------------------------
// Coding Agent Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CodingAgentStatus {
    pub id: String,
    pub label: String,
    pub available: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::tool_operation::{
        OperationConfidence, OperationKind, OperationRisk, OperationStatus,
    };
    use serde_json::json;

    #[test]
    fn legacy_permission_event_shape_remains_decodable() {
        let event: ChatPermissionRequestEvent = serde_json::from_value(json!({
            "type": "permission_request",
            "session_id": "session-1",
            "tool_use_id": "tool-1",
            "tool": "Bash",
            "command": "Bash: cargo test",
            "input": {"command": "cargo test"},
            "options": ["Allow", "Deny"]
        }))
        .unwrap();

        assert_eq!(event.event_type, "permission_request");
        assert!(event.operation.is_none());
        assert!(event.security.is_none());
        assert!(event.response_binding.is_none());
    }

    #[test]
    fn exact_permission_event_round_trips_display_safe_fields() {
        let event = ChatPermissionRequestEvent {
            event_type: "permission_request".to_string(),
            session_id: "session-1".to_string(),
            tool_use_id: "tool-1".to_string(),
            tool: "Bash".to_string(),
            command: "Bash: cargo test".to_string(),
            input: json!({"command": "cargo test"}),
            options: vec!["Allow once".to_string(), "Deny".to_string()],
            operation: Some(ToolOperationDisplay {
                kind: OperationKind::Permission,
                subtype: None,
                status: OperationStatus::InProgress,
                risk: OperationRisk::High,
                confidence: OperationConfidence::High,
                label: "Permission".to_string(),
                target: None,
                command_summary: Some("cargo test".to_string()),
                raw_tool_name: "Bash".to_string(),
            }),
            security: Some(SecurityDecisionDisplay {
                sink: "ShellExec".to_string(),
                decision: "ask".to_string(),
                rule_ids: vec!["workflow-injection".to_string()],
                source_labels: vec!["web".to_string()],
                source_digests: vec!["sha256:abc".to_string()],
                exact_approval: true,
            }),
            response_binding: Some("opaque-binding".to_string()),
        };

        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["security"]["exact_approval"], true);
        assert_eq!(value["operation"]["kind"], "permission");
        assert!(value["operation"].get("raw_input").is_none());
        let decoded: ChatPermissionRequestEvent = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn permission_response_binding_is_optional() {
        let response: ChatPermissionResponseRequest = serde_json::from_value(json!({
            "session_id": "session-1",
            "decision": "deny"
        }))
        .unwrap();
        assert!(response.response_binding.is_none());
    }
}
