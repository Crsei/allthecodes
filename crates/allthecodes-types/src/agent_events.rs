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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentCompletionStatus {
    #[default]
    Completed,
    Failed,
    Killed,
}

impl AgentCompletionStatus {
    pub const fn from_had_error(had_error: bool) -> Self {
        if had_error {
            Self::Failed
        } else {
            Self::Completed
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }

    pub const fn had_error(self) -> bool {
        matches!(self, Self::Failed | Self::Killed)
    }
}

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
    },
    Completed {
        agent_id: String,
        result_preview: String,
        had_error: bool,
        completion_status: AgentCompletionStatus,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_uses: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
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
    RuntimeActivity {
        task_id: String,
        agent_id: String,
        child_session_id: String,
        phase: String,
        last_heartbeat_at_ms: i64,
        last_progress_at_ms: i64,
        partial_output_bytes: usize,
    },
    TreeSnapshot {
        roots: Vec<AgentNode>,
    },
    OutputBatch {
        agent_id: String,
        task_id: String,
        output: OutputReadBatch,
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
