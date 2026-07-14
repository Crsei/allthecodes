//! Agent and team event/command enums for IPC.
//!
//! **Event enums** (Backend -> Frontend): `Serialize + Debug + Clone`, tagged by `"kind"`.
//! **Command enums** (Frontend -> Backend): `Deserialize + Debug`, tagged by `"kind"`.
//!
//! Moved from `src/ipc/agent_events.rs` to cc-types in Phase 6 so the engine
//! crate's `sdk_to_agent_event` helper can depend on these types without a
//! reverse edge into the future cc-ipc crate.

use serde::{Deserialize, Serialize};

use super::agent_runtime_record::AgentRuntimeExecutionRecord;
use super::agent_types::*;
use super::output::{EventSeq, OutputReadBatch};

// ===========================================================================
// AgentEvent (Backend → Frontend)
// ===========================================================================

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    Spawned {
        agent_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        is_background: bool,
        depth: usize,
        chain_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fork_metadata: Option<ForkLaunchMetadata>,
    },
    Completed {
        agent_id: String,
        result_preview: String,
        had_error: bool,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
    },
    Error {
        agent_id: String,
        error: String,
        duration_ms: u64,
    },
    Aborted {
        agent_id: String,
    },
    StreamDelta {
        agent_id: String,
        text: String,
    },
    ThinkingDelta {
        agent_id: String,
        thinking: String,
    },
    ToolUse {
        agent_id: String,
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    ToolResult {
        agent_id: String,
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
    PermissionQueued {
        agent_id: String,
        tool_use_id: String,
        tool_name: String,
        summary: String,
        queue_position: usize,
        pending_count: usize,
    },
    PermissionResolved {
        agent_id: String,
        tool_use_id: String,
        tool_name: String,
        decision: String,
        pending_count: usize,
    },
    TreeSnapshot {
        roots: Vec<AgentNode>,
    },
    OutputBatch {
        agent_id: String,
        task_id: String,
        output: OutputReadBatch,
        #[serde(skip_serializing_if = "Option::is_none")]
        fork_metadata: Option<ForkLaunchMetadata>,
    },
    ExecutionRecord {
        agent_id: String,
        record: Box<AgentRuntimeExecutionRecord>,
    },
}

// ===========================================================================
// AgentCommand (Frontend → Backend)
// ===========================================================================

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentCommand {
    AbortAgent {
        agent_id: String,
    },
    QueryActiveAgents,
    QueryAgentOutput {
        agent_id: String,
        #[serde(default)]
        after_seq: Option<EventSeq>,
        #[serde(default)]
        limit_bytes: Option<usize>,
    },
}

// ===========================================================================
// TeamEvent (Backend → Frontend)
// ===========================================================================

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvent {
    MemberJoined {
        team_name: String,
        agent_id: String,
        agent_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    MemberLeft {
        team_name: String,
        agent_id: String,
        agent_name: String,
    },
    MessageRouted {
        team_name: String,
        from: String,
        to: String,
        text: String,
        timestamp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    StatusSnapshot {
        team_name: String,
        members: Vec<TeamMemberInfo>,
        pending_messages: usize,
    },
}

// ===========================================================================
// TeamCommand (Frontend → Backend)
// ===========================================================================

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamCommand {
    InjectMessage {
        team_name: String,
        to: String,
        text: String,
    },
    QueryTeamStatus {
        team_name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spawned_event_serializes_fork_context_when_present() {
        let event = AgentEvent::Spawned {
            agent_id: "child".to_string(),
            parent_agent_id: Some("parent".to_string()),
            description: "fork task".to_string(),
            agent_type: Some("fork".to_string()),
            model: Some("model".to_string()),
            is_background: false,
            depth: 2,
            chain_id: "chain".to_string(),
            fork_metadata: Some(ForkLaunchMetadata {
                is_fork: true,
                context: ForkContextMode::LiveReadonly,
                live_channel: None,
            }),
        };

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(
            value["fork_metadata"]["fork_context"],
            json!("live_readonly")
        );
    }

    #[test]
    fn spawned_event_omits_fork_context_for_normal_agent() {
        let event = AgentEvent::Spawned {
            agent_id: "child".to_string(),
            parent_agent_id: None,
            description: "normal task".to_string(),
            agent_type: None,
            model: None,
            is_background: false,
            depth: 1,
            chain_id: "chain".to_string(),
            fork_metadata: None,
        };

        let value = serde_json::to_value(event).unwrap();
        assert!(value.get("fork_metadata").is_none());
    }
}
