//! Durable Group Chat handlers backed by canonical delegated Agent runtime.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

use allthecodes_config::paths::data_root;
use allthecodes_protocol::v1::group_chat::*;
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod};
use allthecodes_types::output::EventSeq;

use crate::api_dispatcher::rest_processor_response;
use crate::processors::{protocol_error_response, Processor};
use crate::state::WebState;

static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static TEST_STORE_PATH: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

const MAX_ROOM_DISPATCHES: usize = 8;
const MAX_PROFILE_DISPATCHES: usize = 16;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_PROMPT_BYTES: usize = 32 * 1024;
const MAX_RUNTIME_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_RETAINED_EVENTS: usize = 256;
const MAX_INVITE_REQUESTS: usize = 128;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const RUNTIME_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub type GroupChatRuntimeFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct GroupChatLaunchRequest {
    pub coordinator_session_id: String,
    pub canonical_workspace: PathBuf,
    pub prompt: String,
    pub role: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GroupChatLaunchResult {
    pub task_id: String,
    pub child_session_id: String,
    pub runtime_agent_id: String,
}

#[derive(Debug, Clone)]
pub struct GroupChatObserveRequest {
    pub coordinator_session_id: String,
    pub canonical_workspace: PathBuf,
    pub runtime_agent_id: String,
    pub after_seq: Option<EventSeq>,
    pub limit_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct GroupChatRuntimeOutput {
    pub sequence: EventSeq,
    pub stream: String,
    pub chunk: String,
}

#[derive(Debug, Clone)]
pub struct GroupChatRuntimeObservation {
    pub status: GroupChatDispatchStatus,
    pub output: Vec<GroupChatRuntimeOutput>,
    pub next_seq: EventSeq,
    pub truncated: bool,
    pub summary: Option<String>,
    pub error: Option<String>,
}

pub trait GroupChatRuntimeHost: Send + Sync + 'static {
    fn launch(
        &self,
        engine: Arc<allthecodes_engine::lifecycle::QueryEngine>,
        request: GroupChatLaunchRequest,
    ) -> GroupChatRuntimeFuture<GroupChatLaunchResult>;

    fn observe(
        &self,
        request: GroupChatObserveRequest,
    ) -> Result<GroupChatRuntimeObservation, String>;
}

static RUNTIME_HOST: LazyLock<RwLock<Option<Arc<dyn GroupChatRuntimeHost>>>> =
    LazyLock::new(|| RwLock::new(None));

pub fn set_group_chat_runtime_host(host: Arc<dyn GroupChatRuntimeHost>) {
    match RUNTIME_HOST.write() {
        Ok(mut slot) => *slot = Some(host),
        Err(poisoned) => *poisoned.into_inner() = Some(host),
    }
}

fn runtime_host() -> Option<Arc<dyn GroupChatRuntimeHost>> {
    match RUNTIME_HOST.read() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
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
    #[serde(default)]
    coordinator_session_id: Option<String>,
    #[serde(default)]
    dispatches: Vec<GroupChatDispatchRecord>,
    #[serde(default)]
    invite_requests: Vec<GroupChatInviteRequestRecord>,
    #[serde(default)]
    event_log: Vec<GroupChatStreamEnvelope>,
    #[serde(default = "default_next_event_id")]
    next_event_id: u64,
    #[serde(default = "default_compression_state")]
    context_compression: GroupChatCompressionState,
    #[serde(default = "default_revision")]
    revision: u64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupChatDispatchRecord {
    message_id: String,
    client_message_id: String,
    dispatch: GroupChatDispatch,
    #[serde(default)]
    runtime_agent_id: Option<String>,
    #[serde(default)]
    canonical_workspace: Option<PathBuf>,
    #[serde(default)]
    last_output_seq: EventSeq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupChatInviteRequestRecord {
    request_id: String,
    expected_revision: u64,
    rotate: bool,
    response: GroupChatInviteMutationResponse,
}

#[derive(Default)]
struct GroupChatEventBroker {
    rooms: Mutex<HashMap<String, broadcast::Sender<GroupChatStreamEnvelope>>>,
}

impl GroupChatEventBroker {
    fn subscribe(&self, room_id: &str) -> broadcast::Receiver<GroupChatStreamEnvelope> {
        let mut rooms = match self.rooms.lock() {
            Ok(rooms) => rooms,
            Err(poisoned) => poisoned.into_inner(),
        };
        rooms
            .entry(room_id.to_string())
            .or_insert_with(|| broadcast::channel(MAX_RETAINED_EVENTS).0)
            .subscribe()
    }

    fn publish(&self, event: GroupChatStreamEnvelope) {
        let sender = {
            let mut rooms = match self.rooms.lock() {
                Ok(rooms) => rooms,
                Err(poisoned) => poisoned.into_inner(),
            };
            rooms
                .entry(event.room_id.clone())
                .or_insert_with(|| broadcast::channel(MAX_RETAINED_EVENTS).0)
                .clone()
        };
        let _ = sender.send(event);
    }
}

static EVENT_BROKER: LazyLock<GroupChatEventBroker> = LazyLock::new(GroupChatEventBroker::default);

macro_rules! define_group_chat_processor {
    ($name:ident, $request:ty, $response:ty, $handler_name:literal, $operation:path) => {
        #[derive(Clone, Copy, Default)]
        pub struct $name;

        impl From<WebState> for $name {
            fn from(_state: WebState) -> Self {
                Self
            }
        }

        #[async_trait]
        impl Processor for $name {
            type Request = $request;
            type Response = $response;
            type Error = ProtocolApiError;

            fn handler_name() -> &'static str {
                $handler_name
            }

            async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
                $operation(params)
            }
        }
    };
}

define_group_chat_processor!(
    GroupChatRoomsListProcessor,
    GroupChatProfileQuery,
    GroupChatRoomsResponse,
    "group_chat.rooms.list",
    group_chat_rooms
);
define_group_chat_processor!(
    GroupChatRoomDetailProcessor,
    GroupChatRoomParams,
    GroupChatRoomDetailResponse,
    "group_chat.rooms.detail",
    group_chat_room_detail
);
define_group_chat_processor!(
    GroupChatRoomCreateProcessor,
    GroupChatRoomCreateRequest,
    GroupChatRoomMutationResponse,
    "group_chat.rooms.create",
    group_chat_room_create
);
define_group_chat_processor!(
    GroupChatRoomCloneProcessor,
    GroupChatRoomCloneParams,
    GroupChatRoomMutationResponse,
    "group_chat.rooms.clone",
    group_chat_room_clone
);
define_group_chat_processor!(
    GroupChatRoomDeleteProcessor,
    GroupChatRoomDeleteParams,
    GroupChatRoomMutationResponse,
    "group_chat.rooms.delete",
    group_chat_room_delete
);
define_group_chat_processor!(
    GroupChatInviteReadProcessor,
    GroupChatRoomParams,
    GroupChatInvite,
    "group_chat.invites.read",
    group_chat_invite_read
);
define_group_chat_processor!(
    GroupChatInviteMutationProcessor,
    GroupChatInviteMutationParams,
    GroupChatInviteMutationResponse,
    "group_chat.invites.mutate",
    group_chat_invite_mutation
);
define_group_chat_processor!(
    GroupChatAgentAddProcessor,
    GroupChatAgentCreateParams,
    GroupChatAgentMutationResponse,
    "group_chat.agents.add",
    group_chat_agent_add
);
define_group_chat_processor!(
    GroupChatAgentUpdateProcessor,
    GroupChatAgentUpdateParams,
    GroupChatAgentMutationResponse,
    "group_chat.agents.update",
    group_chat_agent_update
);
define_group_chat_processor!(
    GroupChatAgentDeleteProcessor,
    GroupChatAgentDeleteParams,
    GroupChatAgentMutationResponse,
    "group_chat.agents.delete",
    group_chat_agent_delete
);
define_group_chat_processor!(
    GroupChatCompressionProcessor,
    GroupChatCompressionUpdateParams,
    GroupChatCompressionResponse,
    "group_chat.compression.update",
    group_chat_compression_update
);

#[derive(Clone)]
pub struct GroupChatMessageProcessor {
    state: WebState,
}

impl From<WebState> for GroupChatMessageProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for GroupChatMessageProcessor {
    type Request = GroupChatMessageSendParams;
    type Response = GroupChatMessageResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "group_chat.messages.dispatch"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        group_chat_message(&self.state, params).await
    }
}

fn group_chat_rooms(
    query: GroupChatProfileQuery,
) -> Result<GroupChatRoomsResponse, ProtocolApiError> {
    with_store_read(|store| {
        let rooms = room_summaries(store, query.profile_id.as_deref());
        Ok(GroupChatRoomsResponse {
            profile_id: query.profile_id.clone(),
            active_room_id: active_room_id(store, query.profile_id.as_deref()),
            rooms,
        })
    })
}

fn group_chat_room_detail(
    params: GroupChatRoomParams,
) -> Result<GroupChatRoomDetailResponse, ProtocolApiError> {
    with_store_read(|store| match find_room(store, &params.id) {
        Some(room)
            if params.profile_id.as_deref().is_some()
                && room.profile_id.as_deref().is_some()
                && params.profile_id.as_deref() != room.profile_id.as_deref() =>
        {
            Err(not_found("Room not found"))
        }
        Some(room) => Ok(detail_response(room, params.profile_id.clone())),
        None => Err(not_found("Room not found")),
    })
}

pub async fn group_chat_rooms_handler(
    State(state): State<WebState>,
    Query(query): Query<GroupChatProfileQuery>,
) -> Response {
    rest_processor_response::<GroupChatRoomsListProcessor>(
        state,
        ApiMethod::GroupChatRoomsList,
        query,
    )
    .await
}

pub async fn group_chat_room_detail_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<GroupChatProfileQuery>,
) -> Response {
    rest_processor_response::<GroupChatRoomDetailProcessor>(
        state,
        ApiMethod::GroupChatRoomDetail,
        GroupChatRoomParams {
            id,
            profile_id: query.profile_id,
        },
    )
    .await
}

fn group_chat_room_create(
    req: GroupChatRoomCreateRequest,
) -> Result<GroupChatRoomMutationResponse, ProtocolApiError> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(bad_request("Room name is required"));
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
            coordinator_session_id: Some(generate_id("group-chat-session")),
            dispatches: Vec::new(),
            invite_requests: Vec::new(),
            event_log: Vec::new(),
            next_event_id: default_next_event_id(),
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
        Ok(GroupChatRoomMutationResponse {
            room: Some(summary),
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
    })
}

pub async fn group_chat_room_create_handler(
    State(state): State<WebState>,
    Json(req): Json<GroupChatRoomCreateRequest>,
) -> Response {
    rest_processor_response::<GroupChatRoomCreateProcessor>(
        state,
        ApiMethod::GroupChatRoomCreate,
        req,
    )
    .await
}

fn group_chat_room_clone(
    params: GroupChatRoomCloneParams,
) -> Result<GroupChatRoomMutationResponse, ProtocolApiError> {
    let GroupChatRoomCloneParams { id, request: req } = params;
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
        room.coordinator_session_id = Some(generate_id("group-chat-session"));
        room.dispatches.clear();
        room.invite_requests.clear();
        room.event_log.clear();
        room.next_event_id = default_next_event_id();
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
        Ok(GroupChatRoomMutationResponse {
            room: Some(summary),
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
    })
}

pub async fn group_chat_room_clone_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<GroupChatRoomCloneRequest>,
) -> Response {
    rest_processor_response::<GroupChatRoomCloneProcessor>(
        state,
        ApiMethod::GroupChatRoomClone,
        GroupChatRoomCloneParams { id, request },
    )
    .await
}

fn group_chat_room_delete(
    params: GroupChatRoomDeleteParams,
) -> Result<GroupChatRoomMutationResponse, ProtocolApiError> {
    let GroupChatRoomDeleteParams { id, profile_id } = params;
    let profile_id = clean_optional(profile_id);
    with_store_write(|store| {
        let Some(index) = store.rooms.iter().position(|room| room.id == id) else {
            return Err(not_found("Room not found"));
        };
        if let (Some(request_profile), Some(room_profile)) = (
            profile_id.as_deref(),
            store.rooms[index].profile_id.as_deref(),
        ) {
            if request_profile != room_profile {
                return Err(not_found("Room not found"));
            }
        }

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
        Ok(GroupChatRoomMutationResponse {
            room: None,
            rooms: Some(rooms),
            active_room_id: store.active_room_id.clone(),
            ok: true,
        })
    })
}

pub async fn group_chat_room_delete_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    body: Option<Json<GroupChatProfileBody>>,
) -> Response {
    rest_processor_response::<GroupChatRoomDeleteProcessor>(
        state,
        ApiMethod::GroupChatRoomDelete,
        GroupChatRoomDeleteParams {
            id,
            profile_id: body.and_then(|Json(body)| body.profile_id),
        },
    )
    .await
}

fn group_chat_invite_read(
    params: GroupChatRoomParams,
) -> Result<GroupChatInvite, ProtocolApiError> {
    let GroupChatRoomParams { id, profile_id } = params;
    with_store_read(|store| {
        let Some(room) = find_room(store, &id) else {
            return Err(not_found("Room not found"));
        };
        if let (Some(request_profile), Some(room_profile)) =
            (profile_id.as_deref(), room.profile_id.as_deref())
        {
            if request_profile != room_profile {
                return Err(not_found("Room not found"));
            }
        }
        match room.invite.clone() {
            Some(invite) => Ok(invite),
            None => Err(invite_creation_required(&id)),
        }
    })
}

fn group_chat_invite_mutation(
    params: GroupChatInviteMutationParams,
) -> Result<GroupChatInviteMutationResponse, ProtocolApiError> {
    let GroupChatInviteMutationParams { id, request: req } = params;
    let request_id = req.request_id.trim();
    if request_id.is_empty() || request_id.len() > 256 || request_id != req.request_id {
        return Err(bad_request_with_code(
            "invalid_request_id",
            "request_id must contain between 1 and 256 bytes",
        ));
    }

    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &id) else {
            return Err(not_found("Room not found"));
        };
        if let Some(previous) = room
            .invite_requests
            .iter()
            .find(|record| record.request_id == request_id)
        {
            if previous.expected_revision != req.expected_revision || previous.rotate != req.rotate
            {
                return Err(conflict_with_code(
                    "idempotency_conflict",
                    "request_id was already used with different input",
                ));
            }
            return Ok(previous.response.clone());
        }
        if room.revision != req.expected_revision {
            return Err(conflict_with_code(
                "revision_conflict",
                format!(
                    "expected room revision {}, current revision is {}",
                    req.expected_revision, room.revision
                ),
            ));
        }

        let had_invite = room.invite.is_some();
        let should_replace = !had_invite || req.rotate;
        if should_replace {
            let code = generate_id("invite");
            room.invite = Some(GroupChatInvite {
                room_id: room.id.clone(),
                code: code.clone(),
                url: Some(format!("/group-chat?invite={code}")),
                expires_at: None,
                created_at: Some(now_string()),
            });
            touch_room(room);
        }
        let invite = room.invite.clone().ok_or_else(|| {
            internal_error("Group chat invite mutation produced no invite".to_string())
        })?;
        let response = GroupChatInviteMutationResponse {
            invite,
            room_revision: room.revision,
            rotated: had_invite && req.rotate,
        };
        room.invite_requests.push(GroupChatInviteRequestRecord {
            request_id: request_id.to_string(),
            expected_revision: req.expected_revision,
            rotate: req.rotate,
            response: response.clone(),
        });
        if room.invite_requests.len() > MAX_INVITE_REQUESTS {
            let remove = room.invite_requests.len() - MAX_INVITE_REQUESTS;
            room.invite_requests.drain(0..remove);
        }
        Ok(response)
    })
}

pub async fn group_chat_invite_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<GroupChatProfileQuery>,
) -> Response {
    rest_processor_response::<GroupChatInviteReadProcessor>(
        state,
        ApiMethod::GroupChatInvite,
        GroupChatRoomParams {
            id,
            profile_id: query.profile_id,
        },
    )
    .await
}

pub async fn group_chat_invite_mutation_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<GroupChatInviteMutationRequest>,
) -> Response {
    rest_processor_response::<GroupChatInviteMutationProcessor>(
        state,
        ApiMethod::GroupChatInviteMutation,
        GroupChatInviteMutationParams { id, request },
    )
    .await
}

fn group_chat_agent_add(
    params: GroupChatAgentCreateParams,
) -> Result<GroupChatAgentMutationResponse, ProtocolApiError> {
    let GroupChatAgentCreateParams {
        room_id,
        request: req,
    } = params;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(bad_request("Agent name is required"));
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
        Ok(GroupChatAgentMutationResponse {
            agent: Some(agent),
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
    })
}

pub async fn group_chat_agent_add_handler(
    State(state): State<WebState>,
    AxumPath(room_id): AxumPath<String>,
    Json(request): Json<GroupChatAgentCreateRequest>,
) -> Response {
    rest_processor_response::<GroupChatAgentAddProcessor>(
        state,
        ApiMethod::GroupChatAgentAdd,
        GroupChatAgentCreateParams { room_id, request },
    )
    .await
}

fn group_chat_agent_update(
    params: GroupChatAgentUpdateParams,
) -> Result<GroupChatAgentMutationResponse, ProtocolApiError> {
    let GroupChatAgentUpdateParams {
        room_id,
        agent_id,
        request: req,
    } = params;
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };
        let Some(index) = room.agents.iter().position(|agent| agent.id == agent_id) else {
            return Err(not_found("Agent not found"));
        };
        if req
            .revision
            .is_some_and(|revision| revision != room.agents[index].revision)
        {
            return Err(conflict_with_code(
                "revision_conflict",
                format!(
                    "expected agent revision {}, current revision is {}",
                    req.revision.unwrap_or_default(),
                    room.agents[index].revision
                ),
            ));
        }
        if req.enabled == Some(false)
            && room.dispatches.iter().any(|record| {
                record.dispatch.agent_id == agent_id && !record.dispatch.status.is_terminal()
            })
        {
            return Err(conflict_with_code(
                "agent_active",
                "an agent with active delegated work cannot be disabled",
            ));
        }

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
            if let Some(enabled) = req.enabled {
                agent.enabled = enabled;
                if !enabled {
                    agent.status = "paused".into();
                    agent.running = false;
                    agent.typing = false;
                }
            }
            agent.last_seen_at = Some(now.clone());
            agent.revision = agent.revision.saturating_add(1);
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
        Ok(GroupChatAgentMutationResponse {
            agent: Some(agent),
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
    })
}

pub async fn group_chat_agent_update_handler(
    State(state): State<WebState>,
    AxumPath((room_id, agent_id)): AxumPath<(String, String)>,
    Json(request): Json<GroupChatAgentUpdateRequest>,
) -> Response {
    rest_processor_response::<GroupChatAgentUpdateProcessor>(
        state,
        ApiMethod::GroupChatAgentUpdate,
        GroupChatAgentUpdateParams {
            room_id,
            agent_id,
            request,
        },
    )
    .await
}

fn group_chat_agent_delete(
    params: GroupChatAgentDeleteParams,
) -> Result<GroupChatAgentMutationResponse, ProtocolApiError> {
    let GroupChatAgentDeleteParams {
        room_id,
        agent_id,
        profile_id,
    } = params;
    let profile_id = clean_optional(profile_id);
    with_store_write(|store| {
        let Some(room) = find_room_mut(store, &room_id) else {
            return Err(not_found("Room not found"));
        };
        if let (Some(request_profile), Some(room_profile)) =
            (profile_id.as_deref(), room.profile_id.as_deref())
        {
            if request_profile != room_profile {
                return Err(not_found("Room not found"));
            }
        }
        if room.dispatches.iter().any(|record| {
            record.dispatch.agent_id == agent_id && !record.dispatch.status.is_terminal()
        }) {
            return Err(conflict_with_code(
                "agent_active",
                "an agent with active delegated work cannot be removed",
            ));
        }
        let original_len = room.agents.len();
        room.agents.retain(|agent| agent.id != agent_id);
        if room.agents.len() == original_len {
            return Err(not_found("Agent not found"));
        }
        room.members
            .retain(|member| !(member.kind == "agent" && member.id == agent_id));
        touch_room(room);
        let summary = room.summary();
        Ok(GroupChatAgentMutationResponse {
            agent: None,
            agents: Some(room.agents.clone()),
            room: Some(summary),
            ok: true,
        })
    })
}

pub async fn group_chat_agent_delete_handler(
    State(state): State<WebState>,
    AxumPath((room_id, agent_id)): AxumPath<(String, String)>,
    body: Option<Json<GroupChatProfileBody>>,
) -> Response {
    rest_processor_response::<GroupChatAgentDeleteProcessor>(
        state,
        ApiMethod::GroupChatAgentDelete,
        GroupChatAgentDeleteParams {
            room_id,
            agent_id,
            profile_id: body.and_then(|Json(body)| body.profile_id),
        },
    )
    .await
}

async fn group_chat_message(
    state: &WebState,
    params: GroupChatMessageSendParams,
) -> Result<GroupChatMessageResponse, ProtocolApiError> {
    let GroupChatMessageSendParams {
        id: room_id,
        request: req,
    } = params;
    let text = req.text.trim();
    if text.is_empty() {
        return Err(bad_request("Message text is required"));
    }
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(payload_too_large(
            "group_chat_message_too_large",
            format!("message exceeds the {MAX_MESSAGE_BYTES}-byte limit"),
        ));
    }
    let client_message_id = req.client_message_id.trim();
    if client_message_id.is_empty()
        || client_message_id.len() > 256
        || client_message_id != req.client_message_id
    {
        return Err(bad_request_with_code(
            "invalid_client_message_id",
            "client_message_id must contain between 1 and 256 bytes",
        ));
    }
    if req.target_agent_ids.is_empty() {
        return Err(bad_request_with_code(
            "targets_required",
            "at least one target agent is required",
        ));
    }

    let engine = state.engine();
    let canonical_workspace = std::fs::canonicalize(engine.cwd()).map_err(|error| {
        service_unavailable(
            "group_chat_workspace_unavailable",
            format!("failed to resolve server workspace: {error}"),
        )
    })?;
    let prepared = prepare_message_dispatch(&room_id, &req, text, canonical_workspace.clone())?;
    if prepared.launches.is_empty() {
        return Ok(prepared.response);
    }

    let runtime = runtime_host();
    let mut launched_monitors = Vec::new();
    for launch in prepared.launches {
        let result = match &runtime {
            Some(runtime) => {
                runtime
                    .launch(
                        engine.clone(),
                        GroupChatLaunchRequest {
                            coordinator_session_id: prepared.coordinator_session_id.clone(),
                            canonical_workspace: canonical_workspace.clone(),
                            prompt: launch.prompt,
                            role: launch.role,
                            model: launch.model,
                        },
                    )
                    .await
            }
            None => Err("canonical delegated Agent runtime is unavailable".to_string()),
        };
        match result {
            Ok(result) => {
                let monitor = commit_launch_success(
                    &room_id,
                    client_message_id,
                    &launch.agent_id,
                    &prepared.coordinator_session_id,
                    canonical_workspace.clone(),
                    result,
                )?;
                launched_monitors.push(monitor);
            }
            Err(error) => {
                commit_launch_failure(&room_id, client_message_id, &launch.agent_id, &error)?;
            }
        }
    }

    if let Some(runtime) = runtime {
        for monitor in launched_monitors {
            spawn_runtime_monitor(runtime.clone(), monitor);
        }
    }
    read_message_response(&room_id, client_message_id)
}

pub async fn group_chat_message_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<GroupChatMessageSendRequest>,
) -> Response {
    rest_processor_response::<GroupChatMessageProcessor>(
        state,
        ApiMethod::GroupChatMessage,
        GroupChatMessageSendParams { id, request },
    )
    .await
}

fn group_chat_compression_update(
    params: GroupChatCompressionUpdateParams,
) -> Result<GroupChatCompressionResponse, ProtocolApiError> {
    let GroupChatCompressionUpdateParams {
        id: room_id,
        request: req,
    } = params;
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
        let compression = room.context_compression.clone();
        append_room_event(
            room,
            GroupChatStreamEvent::CompressionUpdated(compression.clone()),
        );

        Ok(GroupChatCompressionResponse {
            context_compression: compression,
            room: Some(room.summary()),
        })
    })
}

pub async fn group_chat_compression_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<GroupChatCompressionUpdateRequest>,
) -> Response {
    rest_processor_response::<GroupChatCompressionProcessor>(
        state,
        ApiMethod::GroupChatCompression,
        GroupChatCompressionUpdateParams { id, request },
    )
    .await
}

pub async fn group_chat_stream_handler(
    AxumPath(room_id): AxumPath<String>,
    Query(query): Query<GroupChatStreamQuery>,
    headers: HeaderMap,
) -> Response {
    let cursor = query.cursor.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    });
    let mut receiver = EVENT_BROKER.subscribe(&room_id);
    let initial = match stream_initial_events(&room_id, query.profile_id.as_deref(), cursor) {
        Ok(initial) => initial,
        Err(error) => return protocol_error_response(error),
    };
    let mut high_watermark = initial.high_watermark;
    let initial_events = initial.events;
    let stream_room_id = room_id.clone();
    let events = async_stream::stream! {
        for envelope in initial_events {
            match sse_data_event(&envelope) {
                Ok(event) => yield Ok::<Event, Infallible>(event),
                Err(error) => {
                    warn!(room_id = %stream_room_id, error = %error, "failed to encode group chat SSE event");
                    return;
                }
            }
        }
        let start = tokio::time::Instant::now() + HEARTBEAT_INTERVAL;
        let mut heartbeat = tokio::time::interval_at(start, HEARTBEAT_INTERVAL);
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
                received = receiver.recv() => {
                    match received {
                        Ok(envelope) if envelope.event_id > high_watermark => {
                            high_watermark = envelope.event_id;
                            match sse_data_event(&envelope) {
                                Ok(event) => yield Ok(event),
                                Err(error) => {
                                    warn!(room_id = %stream_room_id, error = %error, "failed to encode group chat SSE event");
                                    return;
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            match stream_reset_events(&stream_room_id, high_watermark) {
                                Ok(reset) => {
                                    high_watermark = reset.high_watermark;
                                    for envelope in reset.events {
                                        match sse_data_event(&envelope) {
                                            Ok(event) => yield Ok(event),
                                            Err(error) => {
                                                warn!(room_id = %stream_room_id, error = %error, "failed to encode group chat SSE reset");
                                                return;
                                            }
                                        }
                                    }
                                }
                                Err(_) => return,
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        }
    };
    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(HEARTBEAT_INTERVAL)
                .text("heartbeat"),
        )
        .into_response()
}

#[derive(Debug)]
struct PendingGroupChatLaunch {
    agent_id: String,
    role: String,
    model: Option<String>,
    prompt: String,
}

#[derive(Debug)]
struct PreparedGroupChatMessage {
    coordinator_session_id: String,
    launches: Vec<PendingGroupChatLaunch>,
    response: GroupChatMessageResponse,
}

#[derive(Clone)]
struct GroupChatRuntimeMonitor {
    room_id: String,
    client_message_id: String,
    agent_id: String,
    coordinator_session_id: String,
    canonical_workspace: PathBuf,
    runtime_agent_id: String,
}

struct GroupChatInitialEvents {
    events: Vec<GroupChatStreamEnvelope>,
    high_watermark: u64,
}

fn prepare_message_dispatch(
    room_id: &str,
    request: &GroupChatMessageSendRequest,
    text: &str,
    canonical_workspace: PathBuf,
) -> Result<PreparedGroupChatMessage, ProtocolApiError> {
    mutate_store(|store| {
        let Some(room_index) = store.rooms.iter().position(|room| room.id == room_id) else {
            return Err(not_found("Room not found"));
        };
        if store.rooms[room_index].status == "archived" {
            return Err(conflict_with_code(
                "room_archived",
                "archived rooms cannot dispatch messages",
            ));
        }
        if let (Some(request_profile), Some(room_profile)) = (
            request.profile_id.as_deref(),
            store.rooms[room_index].profile_id.as_deref(),
        ) {
            if request_profile != room_profile {
                return Err(forbidden_with_code(
                    "profile_scope_mismatch",
                    "room does not belong to the requested profile",
                ));
            }
        }

        if let Some(existing) = store.rooms[room_index]
            .messages
            .iter()
            .find(|message| message.id == request.client_message_id)
            .cloned()
        {
            if existing.text != text || existing.target_agent_ids != request.target_agent_ids {
                return Err(conflict_with_code(
                    "idempotency_conflict",
                    "client_message_id was already used with different message input",
                ));
            }
            let room = &store.rooms[room_index];
            return Ok(PreparedGroupChatMessage {
                coordinator_session_id: room.coordinator_session_id.clone().unwrap_or_default(),
                launches: Vec::new(),
                response: message_response(room, existing),
            });
        }

        let mut seen = HashSet::new();
        if request.target_agent_ids.len() > MAX_ROOM_DISPATCHES
            || request
                .target_agent_ids
                .iter()
                .any(|agent_id| !seen.insert(agent_id.clone()))
        {
            return Err(bad_request_with_code(
                "invalid_targets",
                format!("target agents must be unique and no more than {MAX_ROOM_DISPATCHES}"),
            ));
        }

        let room_profile = store.rooms[room_index].profile_id.clone();
        let active_profile_dispatches = store
            .rooms
            .iter()
            .filter(|room| room.profile_id == room_profile)
            .flat_map(|room| room.dispatches.iter())
            .filter(|record| !record.dispatch.status.is_terminal())
            .count();
        let active_room_dispatches = store.rooms[room_index]
            .dispatches
            .iter()
            .filter(|record| !record.dispatch.status.is_terminal())
            .count();
        if active_room_dispatches + request.target_agent_ids.len() > MAX_ROOM_DISPATCHES
            || active_profile_dispatches + request.target_agent_ids.len() > MAX_PROFILE_DISPATCHES
        {
            return Err(conflict_with_code(
                "dispatch_limit_exceeded",
                "room or profile Agent concurrency limit would be exceeded",
            ));
        }

        let target_agents = request
            .target_agent_ids
            .iter()
            .map(|target| {
                let agent = store.rooms[room_index]
                    .agents
                    .iter()
                    .find(|agent| agent.id == *target)
                    .cloned()
                    .ok_or_else(|| not_found("Agent not found"))?;
                if !agent.enabled {
                    return Err(conflict_with_code(
                        "agent_disabled",
                        format!("agent {} is disabled", agent.id),
                    ));
                }
                if agent.role.as_deref().is_some_and(|role| role.len() > 512)
                    || agent
                        .model
                        .as_deref()
                        .is_some_and(|model| model.len() > 256)
                {
                    return Err(bad_request_with_code(
                        "invalid_agent_configuration",
                        format!("agent {} role or model exceeds its bound", agent.id),
                    ));
                }
                Ok(agent)
            })
            .collect::<Result<Vec<_>, ProtocolApiError>>()?;

        let room = &mut store.rooms[room_index];
        let coordinator_session_id = room
            .coordinator_session_id
            .get_or_insert_with(|| generate_id("group-chat-session"))
            .clone();
        let now = now_string();
        let message_run_id = generate_id("group-chat-run");
        let message = GroupChatMessage {
            id: request.client_message_id.clone(),
            room_id: room_id.to_string(),
            author_id: Some("local-user".into()),
            author_name: "You".into(),
            author_kind: "user".into(),
            agent_id: None,
            text: text.to_string(),
            mentions: request.target_agent_ids.clone(),
            target_agent_ids: request.target_agent_ids.clone(),
            status: Some("dispatching".into()),
            run_id: Some(message_run_id),
            created_at: now.clone(),
            updated_at: Some(now),
        };
        let launches = target_agents
            .iter()
            .map(|agent| PendingGroupChatLaunch {
                agent_id: agent.id.clone(),
                role: agent
                    .role
                    .clone()
                    .unwrap_or_else(|| "general-purpose".to_string()),
                model: agent.model.clone(),
                prompt: build_agent_prompt(room, agent, text),
            })
            .collect::<Vec<_>>();
        let dispatches = target_agents
            .iter()
            .map(|agent| GroupChatDispatch {
                agent_id: agent.id.clone(),
                task_id: String::new(),
                run_id: generate_id("run"),
                child_session_id: String::new(),
                status: GroupChatDispatchStatus::Dispatching,
                error: None,
            })
            .collect::<Vec<_>>();
        room.dispatches
            .extend(
                dispatches
                    .iter()
                    .cloned()
                    .map(|dispatch| GroupChatDispatchRecord {
                        message_id: message.id.clone(),
                        client_message_id: request.client_message_id.clone(),
                        dispatch,
                        runtime_agent_id: None,
                        canonical_workspace: Some(canonical_workspace.clone()),
                        last_output_seq: 0,
                    }),
            );
        room.messages.push(message.clone());
        room.status = "active".into();
        touch_room(room);
        append_room_event(
            room,
            GroupChatStreamEvent::MessageCreated {
                message: message.clone(),
                dispatches,
            },
        );

        Ok(PreparedGroupChatMessage {
            coordinator_session_id,
            launches,
            response: message_response(room, message),
        })
    })
}

fn commit_launch_success(
    room_id: &str,
    client_message_id: &str,
    agent_id: &str,
    coordinator_session_id: &str,
    canonical_workspace: PathBuf,
    result: GroupChatLaunchResult,
) -> Result<GroupChatRuntimeMonitor, ProtocolApiError> {
    mutate_store(|store| {
        let room = find_room_mut(store, room_id).ok_or_else(|| not_found("Room not found"))?;
        let record_index = room
            .dispatches
            .iter()
            .position(|record| {
                record.client_message_id == client_message_id
                    && record.dispatch.agent_id == agent_id
            })
            .ok_or_else(|| not_found("Message dispatch not found"))?;
        {
            let record = &mut room.dispatches[record_index];
            record.dispatch.task_id = result.task_id;
            record.dispatch.child_session_id = result.child_session_id;
            record.dispatch.status = GroupChatDispatchStatus::Accepted;
            record.runtime_agent_id = Some(result.runtime_agent_id.clone());
            record.canonical_workspace = Some(canonical_workspace.clone());
        }
        let dispatch = room.dispatches[record_index].dispatch.clone();
        if let Some(agent) = room.agents.iter_mut().find(|agent| agent.id == agent_id) {
            agent.status = "running".into();
            agent.running = true;
            agent.typing = false;
            agent.current_task = Some(dispatch.task_id.clone());
            agent.last_seen_at = Some(now_string());
        }
        update_message_status(room, client_message_id);
        touch_room(room);
        append_room_event(room, GroupChatStreamEvent::AgentStarted(dispatch));
        Ok(GroupChatRuntimeMonitor {
            room_id: room_id.to_string(),
            client_message_id: client_message_id.to_string(),
            agent_id: agent_id.to_string(),
            coordinator_session_id: coordinator_session_id.to_string(),
            canonical_workspace,
            runtime_agent_id: result.runtime_agent_id,
        })
    })
}

fn commit_launch_failure(
    room_id: &str,
    client_message_id: &str,
    agent_id: &str,
    error: &str,
) -> Result<(), ProtocolApiError> {
    mutate_store(|store| {
        let room = find_room_mut(store, room_id).ok_or_else(|| not_found("Room not found"))?;
        let record = room
            .dispatches
            .iter_mut()
            .find(|record| {
                record.client_message_id == client_message_id
                    && record.dispatch.agent_id == agent_id
            })
            .ok_or_else(|| not_found("Message dispatch not found"))?;
        record.dispatch.status = GroupChatDispatchStatus::Failed;
        record.dispatch.error = Some(bounded_public_error(error));
        let dispatch = record.dispatch.clone();
        update_message_status(room, client_message_id);
        touch_room(room);
        append_room_event(
            room,
            GroupChatStreamEvent::AgentFailed(GroupChatTerminalPayload {
                dispatch,
                summary: Some("Agent launch failed".to_string()),
            }),
        );
        Ok(())
    })
}

fn read_message_response(
    room_id: &str,
    client_message_id: &str,
) -> Result<GroupChatMessageResponse, ProtocolApiError> {
    read_store(|store| {
        let room = find_room(store, room_id).ok_or_else(|| not_found("Room not found"))?;
        let message = room
            .messages
            .iter()
            .find(|message| message.id == client_message_id)
            .cloned()
            .ok_or_else(|| not_found("Message not found"))?;
        Ok(message_response(room, message))
    })
}

fn spawn_runtime_monitor(runtime: Arc<dyn GroupChatRuntimeHost>, monitor: GroupChatRuntimeMonitor) {
    tokio::spawn(async move {
        let mut failures = 0usize;
        loop {
            let after_seq = read_dispatch_last_output_seq(&monitor).unwrap_or_default();
            match runtime.observe(GroupChatObserveRequest {
                coordinator_session_id: monitor.coordinator_session_id.clone(),
                canonical_workspace: monitor.canonical_workspace.clone(),
                runtime_agent_id: monitor.runtime_agent_id.clone(),
                after_seq: (after_seq > 0).then_some(after_seq),
                limit_bytes: MAX_RUNTIME_OUTPUT_BYTES,
            }) {
                Ok(observation) => {
                    failures = 0;
                    match apply_runtime_observation(&monitor, observation) {
                        Ok(true) => return,
                        Ok(false) => {}
                        Err(error) => {
                            warn!(room_id = %monitor.room_id, agent_id = %monitor.agent_id, status = error.status_code(), "failed to persist group chat runtime observation");
                            return;
                        }
                    }
                }
                Err(error) => {
                    failures += 1;
                    if failures >= 3 {
                        let _ = commit_launch_failure(
                            &monitor.room_id,
                            &monitor.client_message_id,
                            &monitor.agent_id,
                            &error,
                        );
                        return;
                    }
                }
            }
            tokio::time::sleep(RUNTIME_POLL_INTERVAL).await;
        }
    });
}

fn read_dispatch_last_output_seq(monitor: &GroupChatRuntimeMonitor) -> Option<EventSeq> {
    read_store(|store| {
        Ok(find_room(store, &monitor.room_id)
            .and_then(|room| {
                room.dispatches.iter().find(|record| {
                    record.client_message_id == monitor.client_message_id
                        && record.dispatch.agent_id == monitor.agent_id
                })
            })
            .map(|record| record.last_output_seq))
    })
    .ok()
    .flatten()
}

fn apply_runtime_observation(
    monitor: &GroupChatRuntimeMonitor,
    observation: GroupChatRuntimeObservation,
) -> Result<bool, ProtocolApiError> {
    mutate_store(|store| {
        let room =
            find_room_mut(store, &monitor.room_id).ok_or_else(|| not_found("Room not found"))?;
        let record_index = room
            .dispatches
            .iter()
            .position(|record| {
                record.client_message_id == monitor.client_message_id
                    && record.dispatch.agent_id == monitor.agent_id
            })
            .ok_or_else(|| not_found("Message dispatch not found"))?;
        let mut output_events = Vec::new();
        {
            let record = &mut room.dispatches[record_index];
            for output in observation.output {
                if output.sequence <= record.last_output_seq {
                    continue;
                }
                record.last_output_seq = output.sequence;
                output_events.push(GroupChatStreamEvent::AgentOutput(
                    GroupChatAgentOutputPayload {
                        dispatch: record.dispatch.clone(),
                        sequence: output.sequence,
                        stream: output.stream,
                        chunk: truncate_utf8(output.chunk, MAX_RUNTIME_OUTPUT_BYTES),
                        truncated: observation.truncated,
                    },
                ));
            }
            if !record.dispatch.status.is_terminal() {
                record.dispatch.status = observation.status;
                record.dispatch.error = observation.error.map(|error| bounded_public_error(&error));
            }
        }
        for event in output_events {
            append_room_event(room, event);
        }
        let dispatch = room.dispatches[record_index].dispatch.clone();
        let terminal = dispatch.status.is_terminal();
        if let Some(agent) = room
            .agents
            .iter_mut()
            .find(|agent| agent.id == monitor.agent_id)
        {
            agent.status = dispatch_status_label(dispatch.status).to_string();
            agent.running = !terminal;
            agent.typing = false;
            agent.current_task = (!terminal).then(|| dispatch.task_id.clone());
            agent.last_seen_at = Some(now_string());
        }
        if terminal {
            update_message_status(room, &monitor.client_message_id);
            touch_room(room);
            let payload = GroupChatTerminalPayload {
                dispatch: dispatch.clone(),
                summary: observation
                    .summary
                    .map(|summary| truncate_utf8(summary, 2 * 1024)),
            };
            let event = match dispatch.status {
                GroupChatDispatchStatus::Completed => GroupChatStreamEvent::AgentCompleted(payload),
                GroupChatDispatchStatus::Cancelled => GroupChatStreamEvent::AgentCancelled(payload),
                _ => GroupChatStreamEvent::AgentFailed(payload),
            };
            if !room
                .event_log
                .iter()
                .any(|envelope| terminal_event_matches(&envelope.event, &dispatch.run_id))
            {
                append_room_event(room, event);
            }
        }
        Ok(terminal)
    })
}

fn terminal_event_matches(event: &GroupChatStreamEvent, run_id: &str) -> bool {
    match event {
        GroupChatStreamEvent::AgentCompleted(payload)
        | GroupChatStreamEvent::AgentFailed(payload)
        | GroupChatStreamEvent::AgentCancelled(payload) => payload.dispatch.run_id == run_id,
        _ => false,
    }
}

fn stream_initial_events(
    room_id: &str,
    profile_id: Option<&str>,
    cursor: Option<u64>,
) -> Result<GroupChatInitialEvents, ProtocolApiError> {
    read_store(|store| {
        let room = find_room(store, room_id).ok_or_else(|| not_found("Room not found"))?;
        if let (Some(request_profile), Some(room_profile)) =
            (profile_id, room.profile_id.as_deref())
        {
            if request_profile != room_profile {
                return Err(not_found("Room not found"));
            }
        }
        let high_watermark = room.next_event_id.saturating_sub(1);
        let events = match cursor {
            None => vec![snapshot_envelope(room, high_watermark)],
            Some(cursor) => {
                let first_available = room
                    .event_log
                    .first()
                    .map(|event| event.event_id)
                    .unwrap_or(room.next_event_id);
                if cursor.saturating_add(1) < first_available {
                    vec![
                        reset_envelope(room, cursor, first_available),
                        snapshot_envelope(room, high_watermark),
                    ]
                } else {
                    room.event_log
                        .iter()
                        .filter(|event| event.event_id > cursor)
                        .cloned()
                        .collect()
                }
            }
        };
        Ok(GroupChatInitialEvents {
            events,
            high_watermark,
        })
    })
}

fn stream_reset_events(
    room_id: &str,
    requested_cursor: u64,
) -> Result<GroupChatInitialEvents, ProtocolApiError> {
    read_store(|store| {
        let room = find_room(store, room_id).ok_or_else(|| not_found("Room not found"))?;
        let high_watermark = room.next_event_id.saturating_sub(1);
        let first_available = room
            .event_log
            .first()
            .map(|event| event.event_id)
            .unwrap_or(room.next_event_id);
        Ok(GroupChatInitialEvents {
            events: vec![
                reset_envelope(room, requested_cursor, first_available),
                snapshot_envelope(room, high_watermark),
            ],
            high_watermark,
        })
    })
}

fn sse_data_event(envelope: &GroupChatStreamEnvelope) -> Result<Event, axum::Error> {
    Event::default()
        .id(envelope.event_id.to_string())
        .event(envelope.event.kind())
        .json_data(envelope)
}

fn read_store<T, F>(f: F) -> Result<T, ProtocolApiError>
where
    F: FnOnce(&GroupChatStore) -> Result<T, ProtocolApiError>,
{
    let _guard = store_lock()
        .lock()
        .map_err(|error| internal_error(format!("Group chat store lock is poisoned: {error}")))?;
    let store = load_store()
        .map_err(|error| internal_error(format!("Failed to load group chat store: {error}")))?;
    f(&store)
}

fn mutate_store<T, F>(f: F) -> Result<T, ProtocolApiError>
where
    F: FnOnce(&mut GroupChatStore) -> Result<T, ProtocolApiError>,
{
    let (result, events) = {
        let _guard = store_lock().lock().map_err(|error| {
            internal_error(format!("Group chat store lock is poisoned: {error}"))
        })?;
        let mut store = load_store()
            .map_err(|error| internal_error(format!("Failed to load group chat store: {error}")))?;
        let previous_high_watermarks = store
            .rooms
            .iter()
            .map(|room| (room.id.clone(), room.next_event_id.saturating_sub(1)))
            .collect::<HashMap<_, _>>();
        let result = f(&mut store)?;
        save_store(&store)
            .map_err(|error| internal_error(format!("Failed to save group chat store: {error}")))?;
        let events = store
            .rooms
            .iter()
            .flat_map(|room| {
                let previous = previous_high_watermarks
                    .get(&room.id)
                    .copied()
                    .unwrap_or_default();
                room.event_log
                    .iter()
                    .filter(move |envelope| envelope.event_id > previous)
                    .cloned()
            })
            .collect::<Vec<_>>();
        (result, events)
    };
    for event in events {
        EVENT_BROKER.publish(event);
    }
    Ok(result)
}

fn message_response(room: &RoomRecord, message: GroupChatMessage) -> GroupChatMessageResponse {
    let dispatches = room
        .dispatches
        .iter()
        .filter(|record| record.client_message_id == message.id)
        .map(|record| record.dispatch.clone())
        .collect();
    GroupChatMessageResponse {
        message,
        agents: room.agents.clone(),
        room: room.summary(),
        dispatches,
    }
}

fn build_agent_prompt(room: &RoomRecord, agent: &GroupChatAgent, text: &str) -> String {
    let role = agent.role.as_deref().unwrap_or("general-purpose");
    let mut prompt = format!(
        "You are {} in group chat room {:?}. Your assigned role is {:?}. \
Respond to the latest user request as this agent. Do not claim work that the runtime did not perform.\n\n",
        agent.name, room.name, role
    );
    let mut context = room
        .messages
        .iter()
        .rev()
        .take(20)
        .map(|message| format!("{}: {}", message.author_name, message.text))
        .collect::<Vec<_>>();
    context.reverse();
    if !context.is_empty() {
        prompt.push_str("Recent room context:\n");
        for line in context {
            if prompt.len().saturating_add(line.len()).saturating_add(1) > MAX_PROMPT_BYTES {
                break;
            }
            prompt.push_str(&line);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nLatest user request:\n");
    prompt.push_str(text);
    truncate_utf8(prompt, MAX_PROMPT_BYTES)
}

fn update_message_status(room: &mut RoomRecord, client_message_id: &str) {
    let statuses = room
        .dispatches
        .iter()
        .filter(|record| record.client_message_id == client_message_id)
        .map(|record| record.dispatch.status)
        .collect::<Vec<_>>();
    let status = if statuses.is_empty()
        || statuses
            .iter()
            .all(|status| *status == GroupChatDispatchStatus::Dispatching)
    {
        "dispatching"
    } else if statuses.iter().any(|status| !status.is_terminal()) {
        "running"
    } else if statuses
        .iter()
        .all(|status| *status == GroupChatDispatchStatus::Completed)
    {
        "completed"
    } else if statuses
        .iter()
        .all(|status| *status == GroupChatDispatchStatus::Failed)
    {
        "failed"
    } else if statuses
        .iter()
        .all(|status| *status == GroupChatDispatchStatus::Cancelled)
    {
        "cancelled"
    } else {
        "partial"
    };
    if let Some(message) = room
        .messages
        .iter_mut()
        .find(|message| message.id == client_message_id)
    {
        message.status = Some(status.to_string());
        message.updated_at = Some(now_string());
    }
}

fn bounded_public_error(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("permission")
        || normalized.contains("denied")
        || normalized.contains("taint")
        || normalized.contains("security")
        || normalized.contains("approval")
    {
        "Agent execution was denied by runtime policy".to_string()
    } else if normalized.contains("cancel") {
        "Agent execution was cancelled".to_string()
    } else if normalized.contains("timeout") || normalized.contains("timed out") {
        "Agent runtime timed out".to_string()
    } else if normalized.contains("unavailable") {
        "Delegated Agent runtime is unavailable".to_string()
    } else {
        "Agent execution failed".to_string()
    }
}

const fn dispatch_status_label(status: GroupChatDispatchStatus) -> &'static str {
    match status {
        GroupChatDispatchStatus::Dispatching => "dispatching",
        GroupChatDispatchStatus::Accepted | GroupChatDispatchStatus::Running => "running",
        GroupChatDispatchStatus::Completed => "completed",
        GroupChatDispatchStatus::Failed => "failed",
        GroupChatDispatchStatus::Cancelled => "cancelled",
    }
}

fn append_room_event(room: &mut RoomRecord, event: GroupChatStreamEvent) {
    let event_id = room.next_event_id.max(1);
    room.next_event_id = event_id.saturating_add(1);
    room.event_log.push(GroupChatStreamEnvelope {
        event_id,
        room_id: room.id.clone(),
        room_revision: room.revision,
        event,
    });
    if room.event_log.len() > MAX_RETAINED_EVENTS {
        let remove = room.event_log.len() - MAX_RETAINED_EVENTS;
        room.event_log.drain(0..remove);
    }
}

fn snapshot_envelope(room: &RoomRecord, event_id: u64) -> GroupChatStreamEnvelope {
    GroupChatStreamEnvelope {
        event_id,
        room_id: room.id.clone(),
        room_revision: room.revision,
        event: GroupChatStreamEvent::RoomSnapshot(Box::new(room_snapshot(room))),
    }
}

fn reset_envelope(
    room: &RoomRecord,
    requested_cursor: u64,
    first_available_event_id: u64,
) -> GroupChatStreamEnvelope {
    GroupChatStreamEnvelope {
        event_id: first_available_event_id.saturating_sub(1),
        room_id: room.id.clone(),
        room_revision: room.revision,
        event: GroupChatStreamEvent::ReplayReset(GroupChatReplayResetPayload {
            requested_cursor,
            first_available_event_id,
            reason: "retention_window_exceeded".to_string(),
        }),
    }
}

fn room_snapshot(room: &RoomRecord) -> GroupChatRoomSnapshot {
    GroupChatRoomSnapshot {
        room: room.summary(),
        agents: room.agents.clone(),
        members: room.members.clone(),
        messages: room.messages.clone(),
        invite: room.invite.clone(),
        context_compression: Some(room.context_compression.clone()),
        dispatches: room
            .dispatches
            .iter()
            .map(|record| record.dispatch.clone())
            .collect(),
    }
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

const fn default_next_event_id() -> u64 {
    1
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

fn with_store_read<T, F>(f: F) -> Result<T, ProtocolApiError>
where
    F: FnOnce(&GroupChatStore) -> Result<T, ProtocolApiError>,
{
    read_store(f)
}

fn with_store_write<T, F>(f: F) -> Result<T, ProtocolApiError>
where
    F: FnOnce(&mut GroupChatStore) -> Result<T, ProtocolApiError>,
{
    mutate_store(|store| {
        let previous = store
            .rooms
            .iter()
            .map(|room| {
                (
                    room.id.clone(),
                    (room.revision, room.next_event_id.saturating_sub(1)),
                )
            })
            .collect::<HashMap<_, _>>();
        let response = f(store)?;
        for room in &mut store.rooms {
            let (previous_revision, previous_event_id) =
                previous.get(&room.id).copied().unwrap_or_default();
            let changed = previous_revision != room.revision;
            let already_emitted = room.next_event_id.saturating_sub(1) > previous_event_id;
            if changed && !already_emitted {
                let snapshot = room_snapshot(room);
                append_room_event(room, GroupChatStreamEvent::RoomSnapshot(Box::new(snapshot)));
            }
        }
        Ok(response)
    })
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
    #[cfg(test)]
    if let Some(path) = TEST_STORE_PATH
        .lock()
        .expect("group chat test store lock")
        .clone()
    {
        return path;
    }
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
        dispatches: room
            .dispatches
            .iter()
            .map(|record| record.dispatch.clone())
            .collect(),
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

fn bad_request(message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::BadRequest {
        code: "bad_request",
        message: message.into(),
    }
}

fn bad_request_with_code(code: &'static str, message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::BadRequest {
        code,
        message: message.into(),
    }
}

fn conflict_with_code(_code: &'static str, message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::Conflict {
        reason: message.into(),
    }
}

fn forbidden_with_code(code: &'static str, message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::Forbidden {
        code,
        message: message.into(),
    }
}

fn payload_too_large(code: &'static str, message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::PayloadTooLarge {
        code,
        message: message.into(),
    }
}

fn service_unavailable(code: &'static str, message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::ServiceUnavailable {
        code,
        message: message.into(),
    }
}

fn invite_creation_required(room_id: &str) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "group_chat_invite",
        id: room_id.to_string(),
    }
}

fn not_found(message: impl Into<String>) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "group_chat_resource",
        id: message.into(),
    }
}

fn internal_error(message: String) -> ProtocolApiError {
    warn!(error = %message, "group chat handler failed");
    ProtocolApiError::Internal { message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serial_test::serial;
    use tempfile::TempDir;

    struct TestStoreScope {
        dir: TempDir,
    }

    impl TestStoreScope {
        fn install(store: GroupChatStore) -> Self {
            let dir = TempDir::new().expect("group chat test tempdir");
            *TEST_STORE_PATH.lock().expect("group chat test store lock") =
                Some(dir.path().join("group-chat.json"));
            save_store(&store).expect("save group chat test store");
            Self { dir }
        }

        fn workspace(&self) -> PathBuf {
            self.dir.path().to_path_buf()
        }

        fn bytes(&self) -> Vec<u8> {
            std::fs::read(store_path()).expect("read group chat test store")
        }
    }

    impl Drop for TestStoreScope {
        fn drop(&mut self) {
            *TEST_STORE_PATH.lock().expect("group chat test store lock") = None;
        }
    }

    fn sample_agent(id: &str, enabled: bool) -> GroupChatAgent {
        GroupChatAgent {
            id: id.to_string(),
            name: format!("Agent {id}"),
            role: Some("planner".into()),
            profile_id: Some("profile-1".into()),
            profile_name: None,
            provider_id: None,
            model: None,
            status: if enabled { "idle" } else { "paused" }.into(),
            running: false,
            typing: false,
            enabled,
            current_task: None,
            last_seen_at: None,
            revision: 1,
        }
    }

    fn sample_room(agents: Vec<GroupChatAgent>) -> RoomRecord {
        let now = now_string();
        RoomRecord {
            id: "room-1".into(),
            name: "Engineering".into(),
            description: None,
            status: "idle".into(),
            profile_id: Some("profile-1".into()),
            invite: None,
            members: agents
                .iter()
                .map(|agent| member_from_agent(agent, now.clone()))
                .collect(),
            agents,
            messages: Vec::new(),
            coordinator_session_id: Some("session-1".into()),
            dispatches: Vec::new(),
            invite_requests: Vec::new(),
            event_log: Vec::new(),
            next_event_id: default_next_event_id(),
            context_compression: default_compression_state(),
            revision: 1,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn sample_store(room: RoomRecord) -> GroupChatStore {
        GroupChatStore {
            active_room_id: Some(room.id.clone()),
            rooms: vec![room],
        }
    }

    #[test]
    fn room_summary_counts_current_state() {
        let now = now_string();
        let mut room = sample_room(vec![sample_agent("agent-1", true)]);
        room.invite = Some(GroupChatInvite {
            room_id: "room-1".into(),
            code: "invite-1".into(),
            url: None,
            expires_at: None,
            created_at: Some(now.clone()),
        });
        room.messages = vec![GroupChatMessage {
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
        }];
        room.revision = 3;

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

    #[tokio::test]
    #[serial]
    async fn invite_get_is_pure_and_mutation_is_revisioned_idempotent() {
        let scope = TestStoreScope::install(sample_store(sample_room(Vec::new())));
        let before = scope.bytes();
        let response = group_chat_invite_read(GroupChatRoomParams {
            id: "room-1".to_string(),
            profile_id: None,
        })
        .unwrap_err();
        assert_eq!(response.status_code(), StatusCode::NOT_FOUND.as_u16());
        assert_eq!(scope.bytes(), before);

        let create = GroupChatInviteMutationRequest {
            request_id: "invite-request-1".into(),
            expected_revision: 1,
            rotate: false,
        };
        group_chat_invite_mutation(GroupChatInviteMutationParams {
            id: "room-1".to_string(),
            request: create.clone(),
        })
        .unwrap();
        let first_store = load_store().expect("load created invite");
        let first_room = &first_store.rooms[0];
        let first_code = first_room.invite.as_ref().unwrap().code.clone();
        assert_eq!(first_room.revision, 2);

        group_chat_invite_mutation(GroupChatInviteMutationParams {
            id: "room-1".to_string(),
            request: create.clone(),
        })
        .unwrap();
        assert_eq!(load_store().unwrap().rooms[0].revision, 2);

        let conflict = group_chat_invite_mutation(GroupChatInviteMutationParams {
            id: "room-1".to_string(),
            request: GroupChatInviteMutationRequest {
                rotate: true,
                ..create
            },
        })
        .unwrap_err();
        assert_eq!(conflict.status_code(), StatusCode::CONFLICT.as_u16());

        group_chat_invite_mutation(GroupChatInviteMutationParams {
            id: "room-1".to_string(),
            request: GroupChatInviteMutationRequest {
                request_id: "invite-request-2".into(),
                expected_revision: 2,
                rotate: true,
            },
        })
        .unwrap();
        let rotated = load_store().unwrap();
        assert_eq!(rotated.rooms[0].revision, 3);
        assert_ne!(rotated.rooms[0].invite.as_ref().unwrap().code, first_code);
    }

    #[test]
    #[serial]
    fn dispatch_prevalidation_and_retry_do_not_duplicate_launches() {
        let scope = TestStoreScope::install(sample_store(sample_room(vec![
            sample_agent("enabled", true),
            sample_agent("disabled", false),
        ])));
        let request = |client_message_id: &str, targets: Vec<&str>| GroupChatMessageSendRequest {
            text: "Plan the release".into(),
            target_agent_ids: targets.into_iter().map(str::to_string).collect(),
            client_message_id: client_message_id.into(),
            profile_id: Some("profile-1".into()),
        };

        let unknown = prepare_message_dispatch(
            "room-1",
            &request("unknown-message", vec!["missing"]),
            "Plan the release",
            scope.workspace(),
        )
        .unwrap_err();
        assert_eq!(unknown.status_code(), StatusCode::NOT_FOUND.as_u16());
        assert!(load_store().unwrap().rooms[0].messages.is_empty());

        let disabled = prepare_message_dispatch(
            "room-1",
            &request("disabled-message", vec!["disabled"]),
            "Plan the release",
            scope.workspace(),
        )
        .unwrap_err();
        assert_eq!(disabled.status_code(), StatusCode::CONFLICT.as_u16());
        assert!(load_store().unwrap().rooms[0].messages.is_empty());

        let too_many = prepare_message_dispatch(
            "room-1",
            &request("over-limit", vec!["enabled"; MAX_ROOM_DISPATCHES + 1]),
            "Plan the release",
            scope.workspace(),
        )
        .unwrap_err();
        assert_eq!(too_many.status_code(), StatusCode::BAD_REQUEST.as_u16());

        let valid = request("message-1", vec!["enabled"]);
        let first =
            prepare_message_dispatch("room-1", &valid, "Plan the release", scope.workspace())
                .expect("prepare first dispatch");
        assert_eq!(first.launches.len(), 1);
        let retry =
            prepare_message_dispatch("room-1", &valid, "Plan the release", scope.workspace())
                .expect("prepare idempotent retry");
        assert!(retry.launches.is_empty());
        let stored = load_store().unwrap();
        assert_eq!(stored.rooms[0].messages.len(), 1);
        assert_eq!(stored.rooms[0].dispatches.len(), 1);
    }

    #[test]
    #[serial]
    fn partial_launch_and_runtime_output_preserve_successful_targets() {
        let scope = TestStoreScope::install(sample_store(sample_room(vec![
            sample_agent("agent-a", true),
            sample_agent("agent-b", true),
        ])));
        let request = GroupChatMessageSendRequest {
            text: "Review the change".into(),
            target_agent_ids: vec!["agent-a".into(), "agent-b".into()],
            client_message_id: "message-1".into(),
            profile_id: Some("profile-1".into()),
        };
        prepare_message_dispatch("room-1", &request, "Review the change", scope.workspace())
            .unwrap();
        let monitor = commit_launch_success(
            "room-1",
            "message-1",
            "agent-a",
            "session-1",
            scope.workspace(),
            GroupChatLaunchResult {
                task_id: "task-a".into(),
                child_session_id: "child-a".into(),
                runtime_agent_id: "runtime-a".into(),
            },
        )
        .unwrap();
        commit_launch_failure(
            "room-1",
            "message-1",
            "agent-b",
            "permission denied: raw taint digest secret",
        )
        .unwrap();
        let terminal = apply_runtime_observation(
            &monitor,
            GroupChatRuntimeObservation {
                status: GroupChatDispatchStatus::Completed,
                output: vec![GroupChatRuntimeOutput {
                    sequence: 1,
                    stream: "stdout".into(),
                    chunk: "review complete".into(),
                }],
                next_seq: 1,
                truncated: false,
                summary: Some("completed".into()),
                error: None,
            },
        )
        .unwrap();
        assert!(terminal);

        let room = &load_store().unwrap().rooms[0];
        assert_eq!(room.messages[0].status.as_deref(), Some("partial"));
        assert!(room.dispatches.iter().any(|record| {
            record.dispatch.agent_id == "agent-a"
                && record.dispatch.status == GroupChatDispatchStatus::Completed
        }));
        let failed = room
            .dispatches
            .iter()
            .find(|record| record.dispatch.agent_id == "agent-b")
            .unwrap();
        assert_eq!(failed.dispatch.status, GroupChatDispatchStatus::Failed);
        assert_eq!(
            failed.dispatch.error.as_deref(),
            Some("Agent execution was denied by runtime policy")
        );
        assert!(!serde_json::to_string(&room.event_log)
            .unwrap()
            .contains("taint digest"));
        assert!(room
            .event_log
            .iter()
            .any(|envelope| matches!(envelope.event, GroupChatStreamEvent::AgentOutput(_))));
        assert!(room
            .event_log
            .iter()
            .any(|envelope| matches!(envelope.event, GroupChatStreamEvent::AgentCompleted(_))));
    }

    #[tokio::test]
    #[serial]
    async fn sse_replay_resets_old_cursors_and_broker_is_bounded() {
        let mut room = sample_room(Vec::new());
        for _ in 0..(MAX_RETAINED_EVENTS + 8) {
            append_room_event(
                &mut room,
                GroupChatStreamEvent::CompressionUpdated(default_compression_state()),
            );
        }
        let scope = TestStoreScope::install(sample_store(room));
        let replay = stream_initial_events("room-1", Some("profile-1"), Some(1)).unwrap();
        assert_eq!(replay.events.len(), 2);
        assert!(matches!(
            replay.events[0].event,
            GroupChatStreamEvent::ReplayReset(_)
        ));
        assert!(matches!(
            replay.events[1].event,
            GroupChatStreamEvent::RoomSnapshot(_)
        ));

        let response = group_chat_stream_handler(
            AxumPath("room-1".to_string()),
            Query(GroupChatStreamQuery {
                profile_id: Some("profile-1".into()),
                cursor: None,
            }),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream")));

        let broker = GroupChatEventBroker::default();
        let mut receiver = broker.subscribe("slow-room");
        for event_id in 1..=(MAX_RETAINED_EVENTS as u64 + 1) {
            broker.publish(GroupChatStreamEnvelope {
                event_id,
                room_id: "slow-room".into(),
                room_revision: 1,
                event: GroupChatStreamEvent::CompressionUpdated(default_compression_state()),
            });
        }
        assert!(matches!(
            receiver.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
        drop(scope);
    }
}
