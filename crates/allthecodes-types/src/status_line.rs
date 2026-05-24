//! Stable JSON payload schema for scriptable status-line integrations.
//!
//! Runtime crates assemble this payload and pass it to user configured
//! `statusLine.command` processes, or forward it over IPC as an opaque JSON
//! value. The schema lives in `cc-types` so engine, IPC, UI, and command crates
//! share one contract without depending on runtime implementation.

use serde::{Deserialize, Serialize};

/// Top-level payload piped to the user's status-line command on stdin.
///
/// All fields except `hook_event_name` and `model` are optional: missing data
/// is represented by absent JSON fields rather than zero values, so scripts can
/// distinguish "unknown" from "known to be zero".
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatusLinePayload {
    /// Static event name so scripts can multiplex if this payload is ever
    /// reused for other hooks. Always `"StatusLine"`.
    pub hook_event_name: String,

    /// Protocol version. Bump on breaking removals or renames; additive fields
    /// keep the same version.
    pub version: u32,

    /// Session identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Active model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelInfo>,

    /// Workspace / cwd information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceStatus>,

    /// Context-window occupancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextWindowStatus>,

    /// Cost accumulator for the current session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostStatus>,

    /// Current output style, e.g. `"default"` / `"explanatory"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_style: Option<String>,

    /// Vim mode information, present only when editor mode is vim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vim: Option<VimStatus>,

    /// Active worktree session metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeStatus>,

    /// Whether an assistant response is currently streaming.
    pub streaming: bool,

    /// Count of visible on-screen messages.
    pub message_count: usize,
}

impl StatusLinePayload {
    /// Current status-line payload schema version.
    pub const VERSION: u32 = 1;

    /// Construct a minimal payload with required bookkeeping fields.
    pub fn new() -> Self {
        Self {
            hook_event_name: "StatusLine".to_string(),
            version: Self::VERSION,
            ..Default::default()
        }
    }
}

/// Model identity exposed to status-line scripts.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

/// Workspace / cwd snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_worktree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_worktree: Option<String>,
}

/// Context-window occupancy.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextWindowStatus {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_fraction: Option<f64>,
}

/// Session-wide cost tracking.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CostStatus {
    pub total_usd: f64,
    pub api_calls: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_duration_secs: Option<u64>,
}

/// Vim editor status.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VimStatus {
    pub mode: String,
}

/// Active allthecodes worktree session metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub original_cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_branch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_serializes_minimum_required_keys() {
        let payload = StatusLinePayload::new();
        let value = serde_json::to_value(&payload).unwrap();

        assert_eq!(value["hookEventName"], "StatusLine");
        assert_eq!(value["version"], 1);
        assert_eq!(value["streaming"], false);
        assert_eq!(value["messageCount"], 0);
        assert!(value.get("sessionId").is_none());
        assert!(value.get("model").is_none());
        assert!(value.get("workspace").is_none());
    }

    #[test]
    fn payload_roundtrips_with_nested_structs_without_shape_changes() {
        let payload = StatusLinePayload {
            hook_event_name: "StatusLine".to_string(),
            version: StatusLinePayload::VERSION,
            session_id: Some("session-1".into()),
            model: Some(ModelInfo {
                id: "claude-sonnet-4-20250514".into(),
                display_name: Some("sonnet-4".into()),
                backend: Some("native".into()),
            }),
            workspace: Some(WorkspaceStatus {
                cwd: "/tmp/project".into(),
                project_dir: Some("/tmp/project".into()),
                git_branch: Some("main".into()),
                is_worktree: Some(false),
                git_worktree: None,
            }),
            context: Some(ContextWindowStatus {
                input_tokens: 1000,
                output_tokens: 250,
                cache_read_tokens: 50,
                cache_creation_tokens: 25,
                max_tokens: Some(200_000),
                used_fraction: Some(0.0066),
            }),
            cost: Some(CostStatus {
                total_usd: 0.0123,
                api_calls: 3,
                session_duration_secs: Some(42),
            }),
            output_style: Some("default".into()),
            vim: Some(VimStatus {
                mode: "NORMAL".into(),
            }),
            worktree: Some(WorktreeStatus {
                name: "feature-x".into(),
                path: "/tmp/worktree".into(),
                branch: Some("feature-x".into()),
                original_cwd: "/tmp/project".into(),
                original_branch: Some("main".into()),
            }),
            streaming: true,
            message_count: 7,
        };

        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["hookEventName"], "StatusLine");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["messageCount"], 7);
        assert_eq!(value["model"]["displayName"], "sonnet-4");
        assert!(value["workspace"].get("gitWorktree").is_none());
        assert_eq!(value["context"]["cacheReadTokens"], 50);
        assert_eq!(value["cost"]["sessionDurationSecs"], 42);

        let parsed: StatusLinePayload = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, payload);
    }
}
