//! Agent Teams core type definitions.
//!
//! Corresponds to TypeScript: `utils/swarm/teamHelpers.ts` types,
//! `utils/teammate.ts`, `state/AppState.ts` (TeamContext).

use serde::{Deserialize, Serialize};

use allthecodes_tools::tool::PermissionMode;

// ---------------------------------------------------------------------------
// BackendType
// ---------------------------------------------------------------------------

/// Execution backend for a teammate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendType {
    InProcess,
    Tmux,
    #[serde(rename = "iterm2")]
    ITerm2,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::InProcess => write!(f, "in-process"),
            BackendType::Tmux => write!(f, "tmux"),
            BackendType::ITerm2 => write!(f, "iterm2"),
        }
    }
}

impl std::str::FromStr for BackendType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "in-process" | "in_process" | "inprocess" => Ok(BackendType::InProcess),
            "tmux" => Ok(BackendType::Tmux),
            "iterm2" | "iterm" => Ok(BackendType::ITerm2),
            other => Err(format!("unknown Agent Teams backend: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// TeamFile — persisted team configuration
// ---------------------------------------------------------------------------

/// Team configuration persisted on disk.
///
/// File location: `{data_root}/teams/{sanitized_team_name}/config.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamFile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: i64,
    pub lead_agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_session_id: Option<String>,
    #[serde(default)]
    pub hidden_pane_ids: Vec<String>,
    #[serde(default)]
    pub team_allowed_paths: Vec<TeamAllowedPath>,
    pub members: Vec<TeamMember>,
}

/// A shared edit-permission entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamAllowedPath {
    pub path: String,
    pub tool_name: String,
    pub added_by: String,
    pub added_at: i64,
}

/// A team member record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_mode_required: Option<bool>,
    pub joined_at: i64,
    #[serde(default)]
    pub tmux_pane_id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_path: Option<String>,
    #[serde(default)]
    pub subscriptions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<BackendType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_requested_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// TeamContext — AppState extension
// ---------------------------------------------------------------------------
//
// `TeamContext` and `TeammateInfo` are pure data types consumed by
// `types/app_state.rs`. They moved to `cc-types::teams` so cc-types no longer
// needs to reach back into the root crate's `teams::` module. See issue #75
// ("Remaining before the source move" in the workspace-split design doc).
pub use allthecodes_types::teams::{TeamContext, TeammateInfo};

// ---------------------------------------------------------------------------
// TeammateMessage — mailbox message
// ---------------------------------------------------------------------------

/// A message stored in a teammate's mailbox file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateMessage {
    pub from: String,
    pub text: String,
    pub timestamp: String,
    #[serde(default)]
    pub read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ---------------------------------------------------------------------------
// TeammateSpawnConfig
// ---------------------------------------------------------------------------

/// Configuration for spawning a new teammate.
#[derive(Debug, Clone)]
pub struct TeammateSpawnConfig {
    pub name: String,
    pub team_name: String,
    pub color: Option<String>,
    pub plan_mode_required: bool,
    pub prompt: String,
    pub agent_type: Option<String>,
    pub cwd: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_mode: Option<SystemPromptMode>,
    pub worktree_path: Option<String>,
    pub parent_session_id: String,
    pub permissions: Vec<String>,
    pub allow_permission_prompts: bool,
}

/// How a custom system prompt interacts with the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemPromptMode {
    Default,
    Replace,
    Append,
}

// ---------------------------------------------------------------------------
// TeammateSpawnResult
// ---------------------------------------------------------------------------

/// Result of spawning a teammate.
#[derive(Debug)]
pub struct TeammateSpawnResult {
    pub success: bool,
    pub agent_id: String,
    pub error: Option<String>,
    pub abort_handle: Option<tokio::task::AbortHandle>,
    pub task_id: Option<String>,
    pub pane_id: Option<String>,
}

// ---------------------------------------------------------------------------
// InProcessTeammateTaskState
// ---------------------------------------------------------------------------

/// Task status for an in-process teammate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Stopped,
    Completed,
}

/// Identity bundle for a teammate (stored in task_local).
#[derive(Debug, Clone)]
pub struct TeammateIdentity {
    pub agent_id: String,
    pub agent_name: String,
    pub team_name: String,
    pub color: Option<String>,
    pub plan_mode_required: bool,
    pub parent_session_id: String,
}

/// Full state of an in-process teammate task.
#[derive(Debug)]
pub struct InProcessTeammateTaskState {
    pub id: String,
    pub status: TaskStatus,
    pub identity: TeammateIdentity,
    pub prompt: String,
    pub model: Option<String>,
    pub abort_handle: Option<tokio::task::AbortHandle>,
    pub cancellation_token: Option<tokio_util::sync::CancellationToken>,
    pub awaiting_plan_approval: bool,
    pub permission_mode: PermissionMode,
    pub error: Option<String>,
    pub pending_user_messages: Vec<String>,
    pub is_idle: bool,
    pub shutdown_requested: bool,
    pub last_reported_tool_count: usize,
    pub last_reported_token_count: usize,
}

// ---------------------------------------------------------------------------
// Idle reason
// ---------------------------------------------------------------------------

/// Why a teammate became idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdleReason {
    Available,
    Interrupted,
    Failed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_display() {
        assert_eq!(BackendType::InProcess.to_string(), "in-process");
        assert_eq!(BackendType::Tmux.to_string(), "tmux");
        assert_eq!(BackendType::ITerm2.to_string(), "iterm2");
    }

    #[test]
    fn test_backend_type_serde_roundtrip() {
        let json = serde_json::to_string(&BackendType::InProcess).unwrap();
        assert_eq!(json, "\"in-process\"");
        let parsed: BackendType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, BackendType::InProcess);
    }

    #[test]
    fn test_backend_type_from_str() {
        assert_eq!(
            "in-process".parse::<BackendType>().unwrap(),
            BackendType::InProcess
        );
        assert_eq!("tmux".parse::<BackendType>().unwrap(), BackendType::Tmux);
        assert_eq!(
            "iterm2".parse::<BackendType>().unwrap(),
            BackendType::ITerm2
        );
        assert!("external".parse::<BackendType>().is_err());
    }

    #[test]
    fn test_team_file_serde() {
        let tf = TeamFile {
            name: "test-team".into(),
            description: Some("A test team".into()),
            created_at: 1700000000,
            lead_agent_id: "team-lead@test-team".into(),
            lead_session_id: None,
            hidden_pane_ids: vec![],
            team_allowed_paths: vec![],
            members: vec![TeamMember {
                agent_id: "team-lead@test-team".into(),
                name: "team-lead".into(),
                agent_type: None,
                model: None,
                prompt: None,
                color: None,
                plan_mode_required: None,
                joined_at: 1700000000,
                tmux_pane_id: String::new(),
                cwd: "/home/user/project".into(),
                worktree_path: None,
                session_id: None,
                task_id: None,
                task_path: None,
                subscriptions: vec![],
                backend_type: Some(BackendType::InProcess),
                is_active: Some(true),
                mode: None,
                close_state: None,
                close_requested_at: None,
            }],
        };
        let json = serde_json::to_string_pretty(&tf).unwrap();
        let parsed: TeamFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test-team");
        assert_eq!(parsed.members.len(), 1);
        assert_eq!(parsed.members[0].backend_type, Some(BackendType::InProcess));
    }

    #[test]
    fn test_teammate_message_serde() {
        let msg = TeammateMessage {
            from: "researcher".into(),
            text: "Found the bug".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            read: false,
            color: Some("blue".into()),
            summary: Some("Bug found".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: TeammateMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, "researcher");
        assert!(!parsed.read);
    }

    #[test]
    fn test_task_status() {
        assert_ne!(TaskStatus::Running, TaskStatus::Stopped);
        assert_ne!(TaskStatus::Running, TaskStatus::Completed);
    }

    // `test_team_context_default` moved to `cc-types::teams` together with
    // `TeamContext` / `TeammateInfo`.

    #[test]
    fn test_idle_reason_serde() {
        let json = serde_json::to_string(&IdleReason::Available).unwrap();
        assert_eq!(json, "\"available\"");
        let parsed: IdleReason = serde_json::from_str("\"interrupted\"").unwrap();
        assert_eq!(parsed, IdleReason::Interrupted);
    }

    #[test]
    fn test_system_prompt_mode_serde() {
        let json = serde_json::to_string(&SystemPromptMode::Append).unwrap();
        assert_eq!(json, "\"append\"");
    }
}
