//! Shared data types for agent tree, agent info, and team member IPC messages.
//!
//! Moved from `src/ipc/agent_types.rs` to cc-types in Phase 6 so crates that
//! need to reference these types (engine for sdk_to_agent_event, tool context
//! for `bg_agent_tx`) don't have to depend on the future cc-ipc crate.
//!
//! All types are `Serialize + Deserialize + Debug + Clone` so they can flow
//! freely across the JSONL/SSE boundary between the Rust backend and any
//! frontend process.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentNode {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: String,
    pub is_background: bool,
    pub depth: usize,
    pub chain_id: String,
    pub spawned_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    pub had_error: bool,
    pub children: Vec<AgentNode>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentInfo {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    pub description: String,
    pub state: String,
    pub is_background: bool,
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TeamMemberInfo {
    pub agent_id: String,
    pub agent_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub is_active: bool,
    pub unread_messages: usize,
}
