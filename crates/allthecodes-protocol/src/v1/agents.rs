#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Agent Summary – lightweight descriptor returned in list contexts
// ---------------------------------------------------------------------------

/// Lightweight descriptor for an agent, suitable for list displays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentSummary {
    /// Unique agent name / type.
    pub name: String,
    /// Source of the agent definition (e.g. "builtin", "user", "project",
    /// "plugin:<id>").
    pub source: String,
}

// ---------------------------------------------------------------------------
// Agent detail query (GET /api/agents/{name})
// ---------------------------------------------------------------------------

/// Path parameters for fetching a single agent by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentDetailParams {
    /// Agent name from the URL path.
    pub name: String,
}

// ---------------------------------------------------------------------------
// List / Summary (GET /api/agents)
// ---------------------------------------------------------------------------

/// Response body for GET /api/agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentsListResponse {
    /// Full agent definitions (opaque Value – the concrete type lives in
    /// allthecodes-ipc-protocol).
    pub agents: Vec<Value>,
    /// Tool metadata for agent tool selection.
    pub tools: Vec<Value>,
}

// ---------------------------------------------------------------------------
// Create / Update (POST /api/agents, PATCH /api/agents/{name})
// ---------------------------------------------------------------------------

/// Request body for creating or updating an agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentUpsertRequest {
    /// Opaque Value – the concrete AgentDefinitionEntry type lives in
    /// allthecodes-ipc-protocol.
    pub entry: Value,
}

// ---------------------------------------------------------------------------
// Delete (DELETE /api/agents/{name}?source=...)
// ---------------------------------------------------------------------------

/// Query parameters for deleting an agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentDeleteParams {
    /// Which scope to delete from: "user" or "project".
    pub source: String,
}

// ---------------------------------------------------------------------------
// Restore overrides (POST /api/agents/{name}/restore)
// ---------------------------------------------------------------------------

/// Response body for restoring an agent's overridden definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AgentRestoreResponse {
    /// Number of override files that were removed.
    pub removed_overrides: usize,
    /// Full agent definitions after restoration (opaque Value – the concrete
    /// type lives in allthecodes-ipc-protocol).
    pub agents: Vec<Value>,
}
