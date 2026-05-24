use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPermissionDecisionEvent {
    pub hook_name: String,
    pub hook_event: String,
    pub matcher: String,
    pub decision: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecisionDebugEvent {
    pub tool_name: String,
    pub matcher: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    pub behavior: String,
    pub reason: String,
}
