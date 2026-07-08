//! Dashboard-facing snapshots for subagent runtime activity.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent_runtime_record::AgentRuntimeExecutionRecord;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Query parameters accepted by the agent runtime dashboard endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeDashboardQuery {
    pub session_id: Option<String>,
    pub limit: Option<usize>,
}

/// Derived lifecycle state for one subagent.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeAgentStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeDashboardSummary {
    pub total_agents: usize,
    pub running_agents: usize,
    pub completed_agents: usize,
    pub failed_agents: usize,
    pub cancelled_agents: usize,
    pub tool_calls: usize,
    pub failed_tool_calls: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeAgentSummary {
    pub agent_id: String,
    pub parent_agent_id: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub depth: Option<usize>,
    pub background: bool,
    pub status: AgentRuntimeAgentStatus,
    pub started_at_ms: Option<i64>,
    pub last_event_at_ms: Option<i64>,
    pub tool_call_count: usize,
    pub failed_tool_call_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeEventItem {
    pub id: i64,
    pub timestamp: String,
    pub ts_millis: i64,
    pub session_id: Option<String>,
    pub agent_id: String,
    pub parent_agent_id: Option<String>,
    pub kind: String,
    pub description: Option<String>,
    pub model: Option<String>,
    pub depth: Option<usize>,
    pub background: bool,
    pub payload: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeExecutionRecordItem {
    pub id: i64,
    pub timestamp: String,
    pub ts_millis: i64,
    pub record: AgentRuntimeExecutionRecord,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeDashboardResponse {
    pub session_id: Option<String>,
    pub updated_at_ms: i64,
    pub limit: usize,
    pub summary: AgentRuntimeDashboardSummary,
    pub agents: Vec<AgentRuntimeAgentSummary>,
    pub events: Vec<AgentRuntimeEventItem>,
    pub execution_records: Vec<AgentRuntimeExecutionRecordItem>,
}
