use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRolloutIndexEntry {
    pub session_id: String,
    pub rollout_path: PathBuf,
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub event_count: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
}

pub const SESSION_ROLLOUTS_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session_rollouts (
  session_id TEXT NOT NULL,
  rollout_path TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  first_seq INTEGER NOT NULL DEFAULT 0,
  last_seq INTEGER NOT NULL DEFAULT 0,
  event_count INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  parent_session_id TEXT,
  branch_from_seq INTEGER,
  workspace_key TEXT,
  workspace_root TEXT,
  workspace_name TEXT,
  PRIMARY KEY (session_id, rollout_path)
);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_session_id
  ON session_rollouts(session_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_workspace
  ON session_rollouts(workspace_key, updated_at DESC);
"#;
