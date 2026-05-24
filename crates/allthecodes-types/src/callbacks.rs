use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::permission_events::{HookPermissionDecisionEvent, PermissionDecisionDebugEvent};

/// Structured payload for an interactive permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionRequestPayload {
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub message: String,
    pub options: Vec<String>,
}

impl PermissionRequestPayload {
    /// Legacy display string used by older IPC clients that only render a
    /// command/description field.
    pub fn legacy_command(&self) -> String {
        if self.message.trim().is_empty() {
            self.tool_name.clone()
        } else {
            format!("{}: {}", self.tool_name, self.message)
        }
    }
}

/// Structured response from an interactive permission request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResponsePayload {
    /// One of "allow", "deny", "always_allow".
    pub decision: String,
    /// Optional user instructions supplied while approving or rejecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

impl PermissionResponsePayload {
    pub fn new(decision: impl Into<String>, feedback: Option<String>) -> Self {
        let feedback = feedback
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Self {
            decision: decision.into(),
            feedback,
        }
    }

    pub fn decision(decision: impl Into<String>) -> Self {
        Self::new(decision, None)
    }

    pub fn deny() -> Self {
        Self::decision("deny")
    }

    pub fn normalized_decision(&self) -> String {
        self.decision.trim().to_ascii_lowercase()
    }
}

/// Structured payload for AskUserQuestion prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskUserRequestPayload {
    pub question: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default = "default_allow_free_text")]
    pub allow_free_text: bool,
}

fn default_allow_free_text() -> bool {
    true
}

/// Async callback for interactive permission requests.
pub type PermissionCallback = Arc<
    dyn Fn(
            PermissionRequestPayload,
        ) -> Pin<Box<dyn Future<Output = PermissionResponsePayload> + Send>>
        + Send
        + Sync,
>;

/// Async callback for interactive "ask the user" tool requests.
pub type AskUserCallback = Arc<
    dyn Fn(AskUserRequestPayload) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync,
>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionEventPayload {
    HookDecision { event: HookPermissionDecisionEvent },
    DecisionDebug { event: PermissionDecisionDebugEvent },
}

pub type PermissionEventCallback = Arc<dyn Fn(PermissionEventPayload) + Send + Sync>;

/// Tool progress callback payload.
#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub tool_use_id: String,
    pub data: Value,
}

/// Host capability needed to install interactive callbacks.
pub trait CallbackHost {
    fn set_permission_callback(&self, cb: PermissionCallback);
    fn set_ask_user_callback(&self, cb: AskUserCallback);
    fn set_permission_event_callback(&self, cb: PermissionEventCallback);
    fn set_tool_progress_callback(&self, cb: Arc<dyn Fn(ToolProgress) + Send + Sync>);
}
