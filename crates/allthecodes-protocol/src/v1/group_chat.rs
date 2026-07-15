//! Typed Group Chat REST and SSE contracts.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatCompressionState {
    pub enabled: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_compression: Option<GroupChatCompressionState>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatMember {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgent {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: String,
    pub running: bool,
    pub typing: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatMessage {
    pub id: String,
    pub room_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub author_name: String,
    pub author_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    #[serde(default)]
    pub target_agent_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatInvite {
    pub room_id: String,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GroupChatDispatchStatus {
    Dispatching,
    Accepted,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl GroupChatDispatchStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatDispatch {
    pub agent_id: String,
    pub task_id: String,
    pub run_id: String,
    pub child_session_id: String,
    pub status: GroupChatDispatchStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomsResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_room_id: Option<String>,
    pub rooms: Vec<GroupChatRoomSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomDetailResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub room: GroupChatRoomSummary,
    pub agents: Vec<GroupChatAgent>,
    pub members: Vec<GroupChatMember>,
    pub messages: Vec<GroupChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite: Option<GroupChatInvite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_compression: Option<GroupChatCompressionState>,
    #[serde(default)]
    pub dispatches: Vec<GroupChatDispatch>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomCloneRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub include_history: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomMutationResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<GroupChatRoomSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rooms: Option<Vec<GroupChatRoomSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_room_id: Option<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentCreateRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentMutationResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<GroupChatAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<GroupChatAgent>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<GroupChatRoomSummary>,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatMessageSendRequest {
    pub text: String,
    #[serde(default)]
    pub target_agent_ids: Vec<String>,
    pub client_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatMessageResponse {
    pub message: GroupChatMessage,
    pub agents: Vec<GroupChatAgent>,
    pub room: GroupChatRoomSummary,
    #[serde(default)]
    pub dispatches: Vec<GroupChatDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatInviteMutationRequest {
    pub request_id: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub rotate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatInviteMutationResponse {
    pub invite: GroupChatInvite,
    pub room_revision: u64,
    pub rotated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatCompressionUpdateRequest {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatCompressionResponse {
    pub context_compression: GroupChatCompressionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<GroupChatRoomSummary>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatProfileQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatStreamQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatProfileBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// Composite path/query parameters used by room detail and invite reads over
/// transports that do not have a separate HTTP path extractor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomCloneParams {
    pub id: String,
    #[serde(flatten)]
    pub request: GroupChatRoomCloneRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomDeleteParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatInviteMutationParams {
    pub id: String,
    #[serde(flatten)]
    pub request: GroupChatInviteMutationRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentCreateParams {
    pub room_id: String,
    #[serde(flatten)]
    pub request: GroupChatAgentCreateRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentUpdateParams {
    pub room_id: String,
    pub agent_id: String,
    #[serde(flatten)]
    pub request: GroupChatAgentUpdateRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentDeleteParams {
    pub room_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatMessageSendParams {
    pub id: String,
    #[serde(flatten)]
    pub request: GroupChatMessageSendRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatCompressionUpdateParams {
    pub id: String,
    #[serde(flatten)]
    pub request: GroupChatCompressionUpdateRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatRoomSnapshot {
    pub room: GroupChatRoomSummary,
    pub agents: Vec<GroupChatAgent>,
    pub members: Vec<GroupChatMember>,
    pub messages: Vec<GroupChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite: Option<GroupChatInvite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_compression: Option<GroupChatCompressionState>,
    #[serde(default)]
    pub dispatches: Vec<GroupChatDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatAgentOutputPayload {
    pub dispatch: GroupChatDispatch,
    pub sequence: u64,
    pub stream: String,
    pub chunk: String,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatTerminalPayload {
    pub dispatch: GroupChatDispatch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatReplayResetPayload {
    pub requested_cursor: u64,
    pub first_available_event_id: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum GroupChatStreamEvent {
    RoomSnapshot(Box<GroupChatRoomSnapshot>),
    MessageCreated {
        message: GroupChatMessage,
        dispatches: Vec<GroupChatDispatch>,
    },
    AgentStarted(GroupChatDispatch),
    AgentOutput(GroupChatAgentOutputPayload),
    AgentCompleted(GroupChatTerminalPayload),
    AgentFailed(GroupChatTerminalPayload),
    AgentCancelled(GroupChatTerminalPayload),
    CompressionUpdated(GroupChatCompressionState),
    ReplayReset(GroupChatReplayResetPayload),
}

impl GroupChatStreamEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::RoomSnapshot(_) => "room_snapshot",
            Self::MessageCreated { .. } => "message_created",
            Self::AgentStarted(_) => "agent_started",
            Self::AgentOutput(_) => "agent_output",
            Self::AgentCompleted(_) => "agent_completed",
            Self::AgentFailed(_) => "agent_failed",
            Self::AgentCancelled(_) => "agent_cancelled",
            Self::CompressionUpdated(_) => "compression_updated",
            Self::ReplayReset(_) => "replay_reset",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct GroupChatStreamEnvelope {
    pub event_id: u64,
    pub room_id: String,
    pub room_revision: u64,
    pub event: GroupChatStreamEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_union_is_publicly_tagged() {
        let event = GroupChatStreamEvent::ReplayReset(GroupChatReplayResetPayload {
            requested_cursor: 1,
            first_available_event_id: 4,
            reason: "retention_window_exceeded".to_string(),
        });
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["kind"], "replay_reset");
        assert_eq!(value["payload"]["first_available_event_id"], 4);
    }
}
