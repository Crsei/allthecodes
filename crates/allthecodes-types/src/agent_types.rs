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

pub const FORK_BOILERPLATE_TAG: &str = "fork-boilerplate";
pub const TASK_NOTIFICATION_TAG: &str = "task-notification";
pub const FORK_ACTIVE_TOOL_PLACEHOLDER: &str = "Fork started — processing in background";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ForkContextMode {
    #[default]
    FullSnapshot,
    LastOutput,
    LiveReadonly,
}

impl ForkContextMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullSnapshot => "full_snapshot",
            Self::LastOutput => "last_output",
            Self::LiveReadonly => "live_readonly",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::FullSnapshot => "full snapshot",
            Self::LastOutput => "last output",
            Self::LiveReadonly => "live readonly",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LiveParentContextPaths {
    pub directory: String,
    pub snapshot: String,
    pub updates: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_diff: Option<String>,
    pub latest_seq: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ForkLaunchMetadata {
    pub is_fork: bool,
    #[serde(rename = "fork_context", alias = "context")]
    pub context: ForkContextMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_channel: Option<LiveParentContextPaths>,
}

impl ForkLaunchMetadata {
    pub fn display_label(&self) -> String {
        format!("fork · context: {}", self.context.display_name())
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_metadata: Option<ForkLaunchMetadata>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fork_context_mode_uses_snake_case_wire_values() {
        assert_eq!(
            serde_json::to_value(ForkContextMode::FullSnapshot).unwrap(),
            json!("full_snapshot")
        );
        assert_eq!(
            serde_json::from_value::<ForkContextMode>(json!("last_output")).unwrap(),
            ForkContextMode::LastOutput
        );
        assert_eq!(
            serde_json::from_value::<ForkContextMode>(json!("live_readonly")).unwrap(),
            ForkContextMode::LiveReadonly
        );

        let legacy: ForkLaunchMetadata = serde_json::from_value(json!({
            "is_fork": true,
            "context": "last_output"
        }))
        .unwrap();
        assert_eq!(legacy.context, ForkContextMode::LastOutput);
    }

    #[test]
    fn agent_node_serializes_fork_context_when_present() {
        let node = AgentNode {
            agent_id: "child".to_string(),
            parent_agent_id: Some("parent".to_string()),
            description: "fork task".to_string(),
            agent_type: Some("fork".to_string()),
            model: Some("model".to_string()),
            state: "running".to_string(),
            is_background: false,
            depth: 2,
            chain_id: "chain".to_string(),
            spawned_at: 100,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            children: vec![],
            fork_metadata: Some(ForkLaunchMetadata {
                is_fork: true,
                context: ForkContextMode::LastOutput,
                live_channel: None,
            }),
        };

        let value = serde_json::to_value(node).unwrap();
        assert_eq!(value["fork_metadata"]["fork_context"], json!("last_output"));
    }

    #[test]
    fn agent_node_omits_fork_context_for_normal_agent() {
        let node = AgentNode {
            agent_id: "child".to_string(),
            parent_agent_id: None,
            description: "normal task".to_string(),
            agent_type: None,
            model: None,
            state: "running".to_string(),
            is_background: false,
            depth: 1,
            chain_id: "chain".to_string(),
            spawned_at: 100,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            children: vec![],
            fork_metadata: None,
        };

        let value = serde_json::to_value(node).unwrap();
        assert!(value.get("fork_metadata").is_none());
    }
}
