#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorktreeSessionsQuery {
    #[serde(default, alias = "repo_id", skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    #[serde(default, alias = "goal_id", skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorktreeSessionBySessionParams {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorktreeSessionsResponse {
    pub ok: bool,
    pub sessions: Vec<WorktreeSessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CurrentWorktreeSessionResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<WorktreeSessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorktreeSessionSummary {
    pub schema_version: u32,
    pub session_id: String,
    pub repo_id: String,
    pub worktree_path: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_goal_id: Option<String>,
    pub created_by: WorktreeSessionCreator,
    pub git_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_cwd: Option<String>,
    pub status: WorktreeSessionStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<String>,
    pub source: WorktreeSessionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorktreeSessionCreator {
    Human,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorktreeSessionStatus {
    Active,
    Kept,
    Removed,
    CleanupFailed,
    Orphaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorktreeSessionSource {
    EnterWorktree,
    AgentIsolation,
    WorktreeCreateHook,
    Imported,
}
