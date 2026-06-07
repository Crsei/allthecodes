//! Group chat MVP handlers backed by a small durable JSON store.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use futures::stream;
use serde::{Deserialize, Serialize};
use tracing::warn;

use allthecodes_config::paths::data_root;

use crate::handlers::ApiError;

static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatCompressionState {
    pub enabled: bool,
    pub status: String,
    pub token_budget: Option<u64>,
    pub input_tokens: Option<u64>,
    pub compressed_tokens: Option<u64>,
    pub ratio: Option<f64>,
    pub summary: Option<String>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatRoomSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub profile_id: Option<String>,
    pub invite_code: Option<String>,
    pub member_count: Option<usize>,
    pub agent_count: Option<usize>,
    pub message_count: Option<usize>,
    pub context_compression: Option<GroupChatCompressionState>,
    pub revision: u64,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatMember {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub role: Option<String>,
    pub status: Option<String>,
    pub profile_id: Option<String>,
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatAgent {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub running: bool,
    pub typing: bool,
    pub enabled: bool,
    pub current_task: Option<String>,
    pub last_seen_at: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatMessage {
    pub id: String,
    pub room_id: String,
    pub author_id: Option<String>,
    pub author_name: String,
    pub author_kind: String,
    pub agent_id: Option<String>,
    pub text: String,
    pub mentions: Vec<String>,
    pub target_agent_ids: Vec<String>,
    pub status: Option<String>,
    pub run_id: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatInvite {
    pub room_id: String,
    pub code: String,
    pub url: Option<String>,
    pub expires_at: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatRoomsResponse {
    pub profile_id: Option<String>,
    pub active_room_id: Option<String>,
    pub rooms: Vec<GroupChatRoomSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatRoomDetailResponse {
    pub profile_id: Option<String>,
    pub room: GroupChatRoomSummary,
    pub agents: Vec<GroupChatAgent>,
    pub members: Vec<GroupChatMember>,
    pub messages: Vec<GroupChatMessage>,
    pub invite: Option<GroupChatInvite>,
    pub context_compression: Option<GroupChatCompressionState>,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatRoomCreateRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatRoomCloneRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub include_history: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatRoomMutationResponse {
    pub room: Option<GroupChatRoomSummary>,
    pub rooms: Option<Vec<GroupChatRoomSummary>>,
    pub active_room_id: Option<String>,
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatAgentCreateRequest {
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatAgentUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatAgentMutationResponse {
    pub agent: Option<GroupChatAgent>,
    pub agents: Option<Vec<GroupChatAgent>>,
    pub room: Option<GroupChatRoomSummary>,
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatMessageSendRequest {
    pub text: String,
    #[serde(default)]
    pub target_agent_ids: Vec<String>,
    #[serde(default)]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatMessageResponse {
    pub message: GroupChatMessage,
    pub agents: Vec<GroupChatAgent>,
    pub room: GroupChatRoomSummary,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatCompressionUpdateRequest {
    pub action: String,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupChatCompressionResponse {
    pub context_compression: GroupChatCompressionState,
    pub room: Option<GroupChatRoomSummary>,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatProfileQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupChatProfileBody {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupChatStore {
    #[serde(default)]
    active_room_id: Option<String>,
    #[serde(default)]
    rooms: Vec<RoomRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoomRecord {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    status: String,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    invite: Option<GroupChatInvite>,
    #[serde(default)]
    agents: Vec<GroupChatAgent>,
    #[serde(default)]
    members: Vec<GroupChatMember>,
    #[serde(default)]
    messages: Vec<GroupChatMessage>,
    #[serde(default = "default_compression_state")]
    context_compression: GroupChatCompressionState,
    #[serde(default = "default_revision")]
    revision: u64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct GroupChatStreamSnapshot {
    #[serde(rename = "type")]
    event_type: &'static str,
    room: GroupChatRoomSummary,
    agents: Vec<GroupChatAgent>,
    members: Vec<GroupChatMember>,
    messages: Vec<GroupChatMessage>,
    invite: Option<GroupChatInvite>,
    context_compression: Option<GroupChatCompressionState>,
}

pub async fn group_chat_rooms_handler(Query(query): Query<GroupChatProfileQuery>) -> Response {
    with_store_read(|store| {
        let rooms = room_summaries(store, query.profile_id.as_deref());
        Json(GroupChatRoomsResponse {
            profile_id: query.profile_id.clone(),
            active_room_id: active_room_id(store, query.profile_id.as_deref()),
            rooms,
        })
        .into_response()
    })
}

pub async fn group_chat_room_detail_handler(
    AxumPath(id): AxumPath<String>,
    Query(query): Query<GroupChatProfileQuery>,
) -> Response {
    with_store_read(|store| match find_room(store, &id) {
        Some(room) => Json(detail_response(room, query.profile_id.clone())).into_response(),
        None => not_found("Room not found"),
    })
}

pub async fn group_chat_room_create_handler(
    Json(req): Json<GroupChatRoomCreateRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return bad_request("Room name is required");
    }

    with_store_write(|store| {
        let now = now_string();
        let mut room = RoomRecord {
            id: generate_id("room"),
            name: name.to_string(),
            description: clean_optional(req.description.clone()),
            status: "idle".into(),
            profile_id: clean_optional(req.profile_id.clone()),
            invite: None,
            agents: Vec::new(),
            members: vec![GroupChatMember {
                id: "local-user".into(),
                name: "You".into(),
                kind: "user".into(),
                role: Some("owner".into()),
                status: Some("online".into()),
                profile_id: clean_optional(req.profile_id.clone()),
                joined_at: Some(now.clone()),
            }],
            messages: Vec::new(),
            context_compression: default_compression_state(),
            revision: 1,
            created_at: now.clone(),
            updated_at: now,
        };

        for agent_id in req.agent_ids.iter().filter(|id| !id.trim().is_empty()) {
            let agent = GroupChatAgent {
                id: agent_id.trim().to_string(),
                name: agent_id.trim().to_string(),
                role: None,
                profile_id: clean_optional(req.profile_id.clone()),
                profile_name: None,
                provider_id: None,
                model: None,
                status: "idle".into(),
                running: false,
                typing: false,
                enabled: true,
                current_task: None,
                last_seen_at: None,
                revision: 1,
            };
            room.agents.push(agent.clone());
            let joined_at = room.updated_at.clone();
            room.members.push(member_from_agent(&agent, joined_at));
        }

        let summary = room.summary();
        store.active_room_id = Some(room.id.clone());
        store.rooms.push(room);
        let rooms = room_summaries(store, req.profile_id.as_deref());
        Ok(Json(GroupChatRoomMutationResponse {
            room: Some(summary),
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_room_clone_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<GroupChatRoomCloneRequest>,
) -> Response {
    with_store_write(|store| {
        let source = match find_room(store, &id) {
            Some(room) => room.clone(),
            None => return Err(not_found("Room not found")),
        };

        let now = now_string();
        let mut room = source.clone();
        room.id = generate_id("room");
        room.name =
            clean_optional(req.name.clone()).unwrap_or_else(|| format!("{} copy", source.name));
        room.status = "idle".into();
        room.profile_id = clean_optional(req.profile_id.clone()).or(source.profile_id);
        room.invite = None;
        room.revision = 1;
        room.created_at = now.clone();
        room.updated_at = now.clone();
        room.context_compression.updated_at = Some(now.clone());
        for agent in &mut room.agents {
            agent.status = "idle".into();
            agent.running = false;
            agent.typing = false;
            agent.current_task = None;
            agent.last_seen_at = None;
            agent.revision = 1;
        }
        for member in &mut room.members {
            member.joined_at = Some(now.clone());
        }
        if !req.include_history {
            room.messages.clear();
        } else {
            for message in &mut room.messages {
                message.room_id = room.id.clone();
            }
        }

        let summary = room.summary();
        store.active_room_id = Some(room.id.clone());
        store.rooms.push(room);
        let rooms = room_summaries(store, req.profile_id.as_deref());
        Ok(Json(GroupChatRoomMutationResponse {
            room: Some(summary),
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_room_delete_handler(
    AxumPath(id): AxumPath<String>,
    body: Option<Json<GroupChatProfileBody>>,
) -> Response {
    let profile_id = body.and_then(|Json(body)| clean_optional(body.profile_id));
    with_store_write(|store| {
        let Some(index) = store.rooms.iter().position(|room| room.id == id) else {
            return Err(not_found("Room not found"));
        };

        store.rooms[index].status = "archived".into();
        touch_room(&mut store.rooms[index]);
        if store.active_room_id.as_deref() == Some(&id) {
            store.active_room_id = store
                .rooms
                .iter()
                .find(|room| room.status != "archived")
                .map(|room| room.id.clone());
        }
        let rooms = room_summaries(store, profile_id.as_deref());
        Ok(Json(GroupChatRoomMutationResponse {
            room: None,
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_invite_handler(
    AxumPath(id): AxumPath<String>,
    Query(_query): Query<GroupChatProfileQuery>,
) -> Response {
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &id) else {
            return Err(not_found("Room not found"));
        };

        if room.invite.is_none() {
            let code = generate_id("invite");
            let invite = GroupChatInvite {
                room_id: room.id.clone(),
                code: code.clone(),
                url: Some(format!("/group-chat?invite={code}")),
                expires_at: None,
                created_at: Some(now_string()),
            };
            room.invite = Some(invite);
            touch_room(room);
        }

        let Some(invite) = room.invite.clone() else {
            return Err(internal_error(
                "Failed to initialize room invite".to_string(),
            ));
        };
        Ok(Json(invite).into_response())
    })
}

pub async fn group_chat_agent_add_handler(
    AxumPath(room_id): AxumPath<String>,
    Json(req): Json<GroupChatAgentCreateRequest>,
) -> Response {
    let name = req.name.trim();
    if name.is_empty() {
        return bad_request("Agent name is required");
    }

    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };

        let now = now_string();
        let agent = GroupChatAgent {
            id: generate_id("agent"),
            name: name.to_string(),
            role: clean_optional(req.role.clone()),
            profile_id: clean_optional(req.profile_id.clone()),
            profile_name: None,
            provider_id: clean_optional(req.provider_id.clone()),
            model: clean_optional(req.model.clone()),
            status: "idle".into(),
            running: false,
            typing: false,
            enabled: true,
            current_task: None,
            last_seen_at: Some(now.clone()),
            revision: 1,
        };

        room.agents.push(agent.clone());
        room.members.push(member_from_agent(&agent, now));
        touch_room(room);
        let summary = room.summary();
        Ok(Json(GroupChatAgentMutationResponse {
            agent: Some(agent),
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_agent_update_handler(
    AxumPath((room_id, agent_id)): AxumPath<(String, String)>,
    Json(req): Json<GroupChatAgentUpdateRequest>,
) -> Response {
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };
        let Some(index) = room.agents.iter().position(|agent| agent.id == agent_id) else {
            return Err(not_found("Agent not found"));
        };

        let now = now_string();
        {
            let agent = &mut room.agents[index];
            if let Some(name) = clean_optional(req.name.clone()) {
                agent.name = name;
            }
            if req.role.is_some() {
                agent.role = clean_optional(req.role.clone());
            }
            if req.profile_id.is_some() {
                agent.profile_id = clean_optional(req.profile_id.clone());
            }
            if req.provider_id.is_some() {
                agent.provider_id = clean_optional(req.provider_id.clone());
            }
            if req.model.is_some() {
                agent.model = clean_optional(req.model.clone());
            }
            if let Some(status) = clean_optional(req.status.clone()) {
                agent.status = status.clone();
                agent.running = status == "running";
                agent.typing = status == "typing";
            }
            if let Some(enabled) = req.enabled {
                agent.enabled = enabled;
                if !enabled {
                    agent.status = "paused".into();
                    agent.running = false;
                    agent.typing = false;
                }
            }
            agent.last_seen_at = Some(now.clone());
            agent.revision = req.revision.unwrap_or(agent.revision).saturating_add(1);
        }

        let agent = room.agents[index].clone();
        if let Some(member) = room.members.iter_mut().find(|member| member.id == agent_id) {
            member.name = agent.name.clone();
            member.role = agent.role.clone();
            member.profile_id = agent.profile_id.clone();
            member.status = Some(agent.status.clone());
        }

        touch_room(room);
        let summary = room.summary();
        Ok(Json(GroupChatAgentMutationResponse {
            agent: Some(agent),
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_agent_delete_handler(
    AxumPath((room_id, agent_id)): AxumPath<(String, String)>,
    body: Option<Json<GroupChatProfileBody>>,
) -> Response {
    let _profile_id = body.and_then(|Json(body)| clean_optional(body.profile_id));
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };
        let original_len = room.agents.len();
        room.agents.retain(|agent| agent.id != agent_id);
        if room.agents.len() == original_len {
            return Err(not_found("Agent not found"));
        }
        room.members
            .retain(|member| !(member.kind == "agent" && member.id == agent_id));
        touch_room(room);
        let summary = room.summary();
        Ok(Json(GroupChatAgentMutationResponse {
            agent: None,
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
        .into_response())
    })
}

pub async fn group_chat_message_handler(
    AxumPath(room_id): AxumPath<String>,
    Json(req): Json<GroupChatMessageSendRequest>,
) -> Response {
    let text = req.text.trim();
    if text.is_empty() {
        return bad_request("Message text is required");
    }

    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };

        let known_agent_ids: Vec<&str> =
            room.agents.iter().map(|agent| agent.id.as_str()).collect();
        for target in &req.target_agent_ids {
            if !known_agent_ids.contains(&target.as_str()) {
                return Err(not_found("Agent not found"));
            }
        }

        let now = now_string();
        let message = GroupChatMessage {
            id: clean_optional(req.client_message_id.clone()).unwrap_or_else(|| generate_id("msg")),
            room_id: room_id.clone(),
            author_id: Some("local-user".into()),
            author_name: "You".into(),
            author_kind: "user".into(),
            agent_id: None,
            text: text.to_string(),
            mentions: req.target_agent_ids.clone(),
            target_agent_ids: req.target_agent_ids.clone(),
            status: Some("sent".into()),
            run_id: None,
            created_at: now.clone(),
            updated_at: Some(now),
        };
        room.messages.push(message.clone());
        if room.status != "archived" {
            room.status = "active".into();
        }
        touch_room(room);
        Ok(Json(GroupChatMessageResponse {
            message,
            agents: room.agents.clone(),
            room: room.summary(),
        })
        .into_response())
    })
}

pub async fn group_chat_compression_handler(
    AxumPath(room_id): AxumPath<String>,
    Json(req): Json<GroupChatCompressionUpdateRequest>,
) -> Response {
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };

        let now = now_string();
        let total_chars: usize = room.messages.iter().map(|message| message.text.len()).sum();
        match req.action.as_str() {
            "enable" => {
                room.context_compression.enabled = true;
                room.context_compression.status = "idle".into();
                room.context_compression.token_budget = req.token_budget;
                room.context_compression.error = None;
            }
            "disable" => {
                room.context_compression.enabled = false;
                room.context_compression.status = "idle".into();
                room.context_compression.error = None;
            }
            "compress" => {
                let input_tokens = estimate_tokens(total_chars);
                let compressed_tokens = input_tokens.saturating_div(3).max(1);
                let ratio = if input_tokens == 0 {
                    None
                } else {
                    Some(compressed_tokens as f64 / input_tokens as f64)
                };
                let summary = compression_summary(room);

                room.context_compression.enabled = true;
                room.context_compression.status = "compressed".into();
                room.context_compression.token_budget =
                    req.token_budget.or(room.context_compression.token_budget);
                room.context_compression.input_tokens = Some(input_tokens);
                room.context_compression.compressed_tokens = Some(compressed_tokens);
                room.context_compression.ratio = ratio;
                room.context_compression.summary = Some(summary);
                room.context_compression.error = None;
            }
            "reset" => {
                room.context_compression = default_compression_state();
            }
            _ => return Err(bad_request("Unsupported compression action")),
        }
        room.context_compression.updated_at = Some(now);
        touch_room(room);

        Ok(Json(GroupChatCompressionResponse {
            context_compression: room.context_compression.clone(),
            room: Some(room.summary()),
        })
        .into_response())
    })
}

pub async fn group_chat_stream_handler(
    AxumPath(room_id): AxumPath<String>,
    Query(_query): Query<GroupChatProfileQuery>,
) -> Response {
    with_store_read(|store| {
        let Some(room) = find_room(store, &room_id) else {
            return not_found("Room not found");
        };

        let snapshot = GroupChatStreamSnapshot {
            event_type: "room_snapshot",
            room: room.summary(),
            agents: room.agents.clone(),
            members: room.members.clone(),
            messages: room.messages.clone(),
            invite: room.invite.clone(),
            context_compression: Some(room.context_compression.clone()),
        };
        let event = match Event::default().event("room_snapshot").json_data(snapshot) {
            Ok(event) => event,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        error: format!("Failed to serialize room snapshot: {error}"),
                        code: "serialization_failed".into(),

                        details: serde_json::json!({}),
                    }),
                )
                    .into_response();
            }
        };
        let heartbeat = Event::default().comment("heartbeat");
        let events = stream::iter(vec![Ok::<Event, Infallible>(event), Ok(heartbeat)]);
        Sse::new(events)
            .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
            .into_response()
    })
}

impl RoomRecord {
    fn summary(&self) -> GroupChatRoomSummary {
        GroupChatRoomSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            status: self.status.clone(),
            profile_id: self.profile_id.clone(),
            invite_code: self.invite.as_ref().map(|invite| invite.code.clone()),
            member_count: Some(self.members.len()),
            agent_count: Some(self.agents.len()),
            message_count: Some(self.messages.len()),
            context_compression: Some(self.context_compression.clone()),
            revision: self.revision,
            created_at: Some(self.created_at.clone()),
            updated_at: Some(self.updated_at.clone()),
        }
    }
}

fn with_store_read<F>(f: F) -> Response
where
    F: FnOnce(&GroupChatStore) -> Response,
{
    let _guard = match store_lock().lock() {
        Ok(guard) => guard,
        Err(error) => {
            return internal_error(format!("Group chat store lock is poisoned: {error}"));
        }
    };
    let store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(format!("Failed to load group chat store: {error}")),
    };
    f(&store)
}

fn with_store_write<F>(f: F) -> Response
where
    F: FnOnce(&mut GroupChatStore) -> Result<Response, Response>,
{
    let _guard = match store_lock().lock() {
        Ok(guard) => guard,
        Err(error) => {
            return internal_error(format!("Group chat store lock is poisoned: {error}"));
        }
    };
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(format!("Failed to load group chat store: {error}")),
    };
    let response = match f(&mut store) {
        Ok(response) => response,
        Err(response) => return response,
    };
    if let Err(error) = save_store(&store) {
        return internal_error(format!("Failed to save group chat store: {error}"));
    }
    response
}

fn load_store() -> Result<GroupChatStore, String> {
    let path = store_path();
    if !path.exists() {
        return Ok(GroupChatStore {
            active_room_id: None,
            rooms: Vec::new(),
        });
    }

    let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    serde_json::from_str::<GroupChatStore>(&raw).map_err(|error| error.to_string())
}

fn save_store(store: &GroupChatStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(&path, raw).map_err(|error| error.to_string())
}

fn store_path() -> PathBuf {
    data_root().join("web").join("group-chat.json")
}

fn store_lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

fn find_room<'a>(store: &'a GroupChatStore, room_id: &str) -> Option<&'a RoomRecord> {
    store.rooms.iter().find(|room| room.id == room_id)
}

fn find_room_mut<'a>(store: &'a mut GroupChatStore, room_id: &str) -> Option<&'a mut RoomRecord> {
    store.rooms.iter_mut().find(|room| room.id == room_id)
}

fn room_summaries(store: &GroupChatStore, profile_id: Option<&str>) -> Vec<GroupChatRoomSummary> {
    let mut rooms: Vec<GroupChatRoomSummary> = store
        .rooms
        .iter()
        .filter(|room| room.status != "archived")
        .filter(|room| match profile_id {
            Some(profile_id) if !profile_id.is_empty() => {
                room.profile_id.as_deref() == Some(profile_id) || room.profile_id.is_none()
            }
            _ => true,
        })
        .map(RoomRecord::summary)
        .collect();

    rooms.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    rooms
}

fn active_room_id(store: &GroupChatStore, profile_id: Option<&str>) -> Option<String> {
    if let Some(active_room_id) = &store.active_room_id {
        if room_summaries(store, profile_id)
            .iter()
            .any(|room| room.id == *active_room_id)
        {
            return Some(active_room_id.clone());
        }
    }

    room_summaries(store, profile_id)
        .first()
        .map(|room| room.id.clone())
}

fn detail_response(room: &RoomRecord, profile_id: Option<String>) -> GroupChatRoomDetailResponse {
    GroupChatRoomDetailResponse {
        profile_id,
        room: room.summary(),
        agents: room.agents.clone(),
        members: room.members.clone(),
        messages: room.messages.clone(),
        invite: room.invite.clone(),
        context_compression: Some(room.context_compression.clone()),
    }
}

fn default_compression_state() -> GroupChatCompressionState {
    GroupChatCompressionState {
        enabled: false,
        status: "idle".into(),
        token_budget: None,
        input_tokens: None,
        compressed_tokens: None,
        ratio: None,
        summary: None,
        updated_at: None,
        error: None,
    }
}

fn default_revision() -> u64 {
    1
}

fn member_from_agent(agent: &GroupChatAgent, joined_at: String) -> GroupChatMember {
    GroupChatMember {
        id: agent.id.clone(),
        name: agent.name.clone(),
        kind: "agent".into(),
        role: agent.role.clone(),
        status: Some(agent.status.clone()),
        profile_id: agent.profile_id.clone(),
        joined_at: Some(joined_at),
    }
}

fn touch_room(room: &mut RoomRecord) {
    room.revision = room.revision.saturating_add(1);
    room.updated_at = now_string();
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn generate_id(prefix: &str) -> String {
    let timestamp = Utc::now().timestamp_millis();
    let pid = std::process::id();
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{timestamp}-{pid}-{counter}")
}

fn estimate_tokens(chars: usize) -> u64 {
    ((chars as u64).saturating_add(3) / 4).max(1)
}

fn compression_summary(room: &RoomRecord) -> String {
    if room.messages.is_empty() {
        return "No messages to compress yet.".into();
    }

    let message_count = room.messages.len();
    let agent_count = room.agents.len();
    format!("Compressed {message_count} message(s) for {agent_count} agent(s).")
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: message.into(),
            code: "bad_request".into(),

            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn not_found(message: impl Into<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: message.into(),
            code: "not_found".into(),

            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn internal_error(message: String) -> Response {
    warn!(error = %message, "group chat handler failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: message,
            code: "group_chat_store_failed".into(),

            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_summary_counts_current_state() {
        let now = now_string();
        let room = RoomRecord {
            id: "room-1".into(),
            name: "Engineering".into(),
            description: None,
            status: "idle".into(),
            profile_id: Some("profile-1".into()),
            invite: Some(GroupChatInvite {
                room_id: "room-1".into(),
                code: "invite-1".into(),
                url: None,
                expires_at: None,
                created_at: Some(now.clone()),
            }),
            agents: vec![GroupChatAgent {
                id: "agent-1".into(),
                name: "Planner".into(),
                role: None,
                profile_id: None,
                profile_name: None,
                provider_id: None,
                model: None,
                status: "idle".into(),
                running: false,
                typing: false,
                enabled: true,
                current_task: None,
                last_seen_at: None,
                revision: 1,
            }],
            members: Vec::new(),
            messages: vec![GroupChatMessage {
                id: "msg-1".into(),
                room_id: "room-1".into(),
                author_id: Some("local-user".into()),
                author_name: "You".into(),
                author_kind: "user".into(),
                agent_id: None,
                text: "hello".into(),
                mentions: Vec::new(),
                target_agent_ids: Vec::new(),
                status: Some("sent".into()),
                run_id: None,
                created_at: now.clone(),
                updated_at: Some(now.clone()),
            }],
            context_compression: default_compression_state(),
            revision: 3,
            created_at: now.clone(),
            updated_at: now,
        };

        let summary = room.summary();

        assert_eq!(summary.agent_count, Some(1));
        assert_eq!(summary.message_count, Some(1));
        assert_eq!(summary.invite_code.as_deref(), Some("invite-1"));
        assert_eq!(summary.profile_id.as_deref(), Some("profile-1"));
    }

    #[test]
    fn generated_ids_do_not_need_uuid() {
        let left = generate_id("room");
        let right = generate_id("room");

        assert!(left.starts_with("room-"));
        assert!(right.starts_with("room-"));
        assert_ne!(left, right);
    }
}
