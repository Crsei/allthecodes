//! Session management handlers — list, detail, new, resume.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use allthecodes_protocol::v1::{SessionArchiveParams, SessionDetailParams, SessionResumeParams};
use allthecodes_protocol::{ApiError as ProtocolApiError, NoParams, SerializationScope};
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use allthecodes_bootstrap::SessionId;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_session::{fork, resume as session_resume, storage};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

use crate::handlers::workspaces::resolve_workspace_root;
use crate::handlers::ApiError;
use crate::processors::{process_processor, Processor};
use crate::state::{SessionOwner, WebState};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Lightweight description of a workspace used to group sessions.
#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub key: String,
    pub root: String,
    pub name: String,
}

/// Response shape for `GET /api/sessions`.
#[derive(Serialize)]
pub struct SessionListResponse {
    /// Workspace derived from the engine's cwd
    pub current_workspace: WorkspaceInfo,
    /// Session id currently loaded in the engine.
    pub active_session_id: String,
    /// All known sessions on disk, sorted by last_modified desc.
    pub sessions: Vec<SessionSummary>,
}

/// Serializable session summary including derived grouping fields.
#[derive(Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    pub message_count: usize,
    pub cwd: String,
    pub title: String,
    pub workspace_key: String,
    pub workspace_root: String,
    pub workspace_name: String,
}

impl From<storage::SessionInfo> for SessionSummary {
    fn from(s: storage::SessionInfo) -> Self {
        Self {
            session_id: s.session_id,
            created_at: s.created_at,
            last_modified: s.last_modified,
            message_count: s.message_count,
            cwd: s.cwd,
            title: s.title,
            workspace_key: s.workspace_key,
            workspace_root: s.workspace_root,
            workspace_name: s.workspace_name,
        }
    }
}

/// Simplified message shape used by session detail / resume responses.
#[derive(Serialize)]
pub struct StoredMessage {
    pub uuid: String,
    pub timestamp: i64,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,
}

#[derive(Serialize)]
pub struct SessionDetailResponse {
    pub session_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    pub cwd: String,
    pub title: String,
    pub workspace_name: String,
    pub messages: Vec<StoredMessage>,
}

#[derive(Serialize)]
pub struct NewSessionResponse {
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct NewSessionRequest {
    #[serde(default)]
    pub workspace_key: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Serialize)]
pub struct SessionMutationResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct SessionBranchResponse {
    pub session_id: String,
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct MessageActionRequest {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Serialize)]
pub struct MessageRegeneratePrepareResponse {
    pub session_id: String,
    pub prompt: String,
}

#[derive(Serialize)]
pub struct RollbackPreviewResponse {
    pub available: bool,
    pub message: Option<String>,
    pub files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SessionListProcessor {
    state: WebState,
}

impl From<WebState> for SessionListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionListProcessor {
    type Request = NoParams;
    type Response = SessionListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.list"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        Ok(build_session_list_response(&self.state))
    }
}

#[derive(Clone)]
pub struct SessionDetailProcessor {
    state: WebState,
}

impl From<WebState> for SessionDetailProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionDetailProcessor {
    type Request = SessionDetailParams;
    type Response = SessionDetailResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.detail"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        load_session_detail(params.id)
    }
}

#[derive(Clone)]
pub struct SessionResumeProcessor {
    state: WebState,
}

impl From<WebState> for SessionResumeProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionResumeProcessor {
    type Request = SessionResumeParams;
    type Response = SessionDetailResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.resume"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        if self.state.is_streaming.load(Ordering::SeqCst) {
            return Err(ProtocolApiError::EngineBusy);
        }
        if let Some(error) = ownership_conflict_error(&self.state) {
            return Err(error);
        }

        info!(session_id = %params.id, "POST /api/sessions/:id/resume");

        let messages = load_session_messages(&params.id)?;
        let response = session_detail_from_messages(params.id.clone(), &messages);
        let engine = rebuild_engine_with_session_id(&self.state, Some(messages), Some(&params.id));
        self.state.replace_engine(engine);

        Ok(response)
    }
}

#[derive(Clone)]
pub struct SessionArchiveProcessor {
    state: WebState,
}

impl From<WebState> for SessionArchiveProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionArchiveProcessor {
    type Request = SessionArchiveParams;
    type Response = SessionMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.archive"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        if self.state.is_streaming.load(Ordering::SeqCst) {
            return Err(ProtocolApiError::EngineBusy);
        }
        if let Some(error) = ownership_conflict_error(&self.state) {
            return Err(error);
        }
        if self.state.engine().current_session_id().to_string() == params.id {
            return Err(ProtocolApiError::Conflict {
                reason: "Cannot archive the active session".to_string(),
            });
        }

        info!(session_id = %params.id, "POST /api/sessions/:id/archive");

        storage::load_session_info(&params.id).map_err(|_| ProtocolApiError::NotFound {
            entity: "session",
            id: params.id.clone(),
        })?;

        storage::archive_session(&params.id).map_err(|error| {
            warn!(session_id = %params.id, error = %error, "failed to archive session");
            ProtocolApiError::Internal {
                message: format!("Failed to archive session: {error}"),
            }
        })?;

        Ok(SessionMutationResponse {
            ok: true,
            message: "Session archived".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// GET /api/sessions -- List all sessions with workspace grouping metadata.
pub async fn sessions_list_handler(State(state): State<WebState>) -> Response {
    process_processor(SessionListProcessor::from(state), NoParams {}).await
}

/// GET /api/sessions/:id -- Load a session's message history for preview.
pub async fn session_detail_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    process_processor(
        SessionDetailProcessor::from(state),
        SessionDetailParams { id },
    )
    .await
}

/// POST /api/sessions/new -- Start a fresh session in the current workspace.
pub async fn session_new_handler(
    State(state): State<WebState>,
    body: Option<Json<NewSessionRequest>>,
) -> impl IntoResponse {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is in progress — abort it before starting a new session".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }
    if let Some(response) = ownership_conflict_response(&state) {
        return response;
    }

    let target_cwd = match body.as_ref() {
        Some(Json(req)) => match (&req.workspace_key, &req.cwd) {
            (Some(workspace_key), cwd) => {
                match resolve_workspace_root(&state, workspace_key, cwd.as_deref()) {
                    Ok(root) => Some(root.to_string_lossy().to_string()),
                    Err(response) => return response,
                }
            }
            (None, Some(cwd)) => {
                let cwd_path = Path::new(cwd);
                if !cwd_path.exists() || !cwd_path.is_dir() {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiError {
                            error: "cwd must be an existing directory".into(),
                            code: "cwd_invalid".into(),
                        }),
                    )
                        .into_response();
                }
                let workspace_key = storage::workspace_key(cwd_path);
                match resolve_workspace_root(&state, &workspace_key, Some(cwd)) {
                    Ok(root) => Some(root.to_string_lossy().to_string()),
                    Err(response) => return response,
                }
            }
            (None, None) => None,
        },
        None => None,
    };

    let engine = rebuild_engine(&state, None, target_cwd);
    let new_id = engine.current_session_id().to_string();
    state.replace_engine(engine);

    info!(session_id = %new_id, "POST /api/sessions/new");
    Json(NewSessionResponse { session_id: new_id }).into_response()
}

/// POST /api/sessions/:id/resume -- Load an existing session into the engine.
pub async fn session_resume_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    process_processor(
        SessionResumeProcessor::from(state),
        SessionResumeParams { id },
    )
    .await
}

/// POST /api/sessions/:id/archive -- Hide a saved session from the default list.
pub async fn session_archive_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    process_processor(
        SessionArchiveProcessor::from(state),
        SessionArchiveParams { id },
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/branch
pub async fn session_message_branch_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let _ = body
        .as_ref()
        .and_then(|Json(req)| req.profile_id.as_deref());
    if let Some(response) = mutation_guard(&state, &id, "branching a message").await {
        return response;
    }

    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    if !messages
        .iter()
        .any(|msg| msg.uuid().to_string() == message_id)
    {
        return message_not_found_response();
    }
    let cwd = storage::load_session_info(&id)
        .map(|info| info.cwd)
        .unwrap_or_else(|_| state.engine().cwd().to_string());
    let new_id = SessionId::new().to_string();

    match fork::fork_session(&id, &new_id, &messages, &cwd, Some(&message_id)) {
        Ok(outcome) => Json(SessionBranchResponse {
            session_id: outcome.new_session_id,
            title: Some(outcome.title),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("Failed to branch session: {}", e),
                code: "session_branch_failed".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/sessions/:id/messages/:message_id/feedback
pub async fn session_message_feedback_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(_state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let rating = body
        .as_ref()
        .and_then(|Json(req)| req.rating.as_deref())
        .unwrap_or("none");
    if storage::load_session_info(&id).is_err() {
        return session_not_found_response();
    }
    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    if !messages
        .iter()
        .any(|msg| msg.uuid().to_string() == message_id)
    {
        return message_not_found_response();
    }
    info!(session_id = %id, message_id = %message_id, rating = %rating, "message feedback recorded");
    Json(SessionMutationResponse {
        ok: true,
        message: "Feedback recorded".into(),
    })
    .into_response()
}

/// POST /api/sessions/:id/messages/:message_id/delete
pub async fn session_message_delete_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let _ = body
        .as_ref()
        .and_then(|Json(req)| req.profile_id.as_deref());
    if let Some(response) = mutation_guard(&state, &id, "deleting a message").await {
        return response;
    }
    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    let Some(index) = message_index(&messages, &message_id) else {
        return message_not_found_response();
    };
    match storage::truncate_session(&id, index) {
        Ok(_) => Json(SessionMutationResponse {
            ok: true,
            message: "Message deleted".into(),
        })
        .into_response(),
        Err(e) => storage_error_response("Failed to delete message", "message_delete_failed", e),
    }
}

/// POST /api/sessions/:id/messages/:message_id/regenerate/prepare
pub async fn session_message_regenerate_prepare_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let _ = body
        .as_ref()
        .and_then(|Json(req)| req.profile_id.as_deref());
    if let Some(response) = mutation_guard(&state, &id, "regenerating a message").await {
        return response;
    }
    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    let Some(assistant_index) = message_index(&messages, &message_id) else {
        return message_not_found_response();
    };
    let Some((user_index, prompt)) = preceding_user_prompt(&messages, assistant_index) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "No preceding user message found for regeneration".into(),
                code: "message_regenerate_unavailable".into(),
            }),
        )
            .into_response();
    };
    match storage::truncate_session(&id, user_index) {
        Ok(_) => Json(MessageRegeneratePrepareResponse {
            session_id: id,
            prompt,
        })
        .into_response(),
        Err(e) => storage_error_response(
            "Failed to prepare regeneration",
            "message_regenerate_failed",
            e,
        ),
    }
}

/// POST /api/sessions/:id/messages/:message_id/edit/prepare
pub async fn session_message_edit_prepare_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    if let Some(response) = mutation_guard(&state, &id, "editing a message").await {
        return response;
    }
    let edited_text = body
        .as_ref()
        .and_then(|Json(req)| req.text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    if edited_text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Edited message text is required".into(),
                code: "message_edit_empty".into(),
            }),
        )
            .into_response();
    }
    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    let Some(index) = message_index(&messages, &message_id) else {
        return message_not_found_response();
    };
    if !matches!(messages.get(index), Some(Message::User(_))) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Only user messages can be edited".into(),
                code: "message_edit_role_invalid".into(),
            }),
        )
            .into_response();
    }
    match storage::truncate_session(&id, index) {
        Ok(_) => Json(MessageRegeneratePrepareResponse {
            session_id: id,
            prompt: edited_text,
        })
        .into_response(),
        Err(e) => storage_error_response("Failed to prepare edit", "message_edit_failed", e),
    }
}

/// POST /api/sessions/:id/messages/:message_id/rollback/preview
pub async fn session_message_rollback_preview_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(_state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let _ = body
        .as_ref()
        .and_then(|Json(req)| req.profile_id.as_deref());
    let messages = match load_session_messages_response(&id) {
        Ok(messages) => messages,
        Err(response) => return response,
    };
    if !messages
        .iter()
        .any(|msg| msg.uuid().to_string() == message_id)
    {
        return message_not_found_response();
    }
    Json(RollbackPreviewResponse {
        available: false,
        message: Some("Rollback checkpoint is not available for this session yet".into()),
        files: Vec::new(),
    })
    .into_response()
}

/// POST /api/sessions/:id/messages/:message_id/rollback
pub async fn session_message_rollback_handler(
    AxumPath((_id, _message_id)): AxumPath<(String, String)>,
    State(_state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    let _files = body.map(|Json(req)| req.files).unwrap_or_default();
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: "Rollback checkpoint storage is not implemented yet".into(),
            code: "rollback_checkpoint_unavailable".into(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_session_list_response(state: &WebState) -> SessionListResponse {
    let engine = state.engine();
    let cwd_str = engine.cwd().to_string();
    let cwd_path = Path::new(&cwd_str);

    let ws_key = storage::workspace_key(cwd_path);
    let ws_root = storage::workspace_root(cwd_path);
    let ws_name = storage::workspace_name(&ws_root);

    let sessions = match storage::list_sessions() {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to list sessions");
            Vec::new()
        }
    };

    let summaries: Vec<SessionSummary> = sessions.into_iter().map(SessionSummary::from).collect();

    SessionListResponse {
        current_workspace: WorkspaceInfo {
            key: ws_key,
            root: ws_root.to_string_lossy().to_string(),
            name: ws_name,
        },
        active_session_id: engine.current_session_id().to_string(),
        sessions: summaries,
    }
}

fn load_session_detail(id: String) -> Result<SessionDetailResponse, ProtocolApiError> {
    info!(session_id = %id, "GET /api/sessions/:id");
    let messages = load_session_messages(&id)?;
    Ok(session_detail_from_messages(id, &messages))
}

fn load_session_messages(session_id: &str) -> Result<Vec<Message>, ProtocolApiError> {
    session_resume::resume_session(session_id).map_err(|_| ProtocolApiError::NotFound {
        entity: "session",
        id: session_id.to_string(),
    })
}

fn session_detail_from_messages(id: String, messages: &[Message]) -> SessionDetailResponse {
    let info = storage::list_sessions()
        .ok()
        .and_then(|list| list.into_iter().find(|s| s.session_id == id));

    let (title, cwd, created_at, last_modified, workspace_name) = match info {
        Some(i) => (
            i.title,
            i.cwd,
            i.created_at,
            i.last_modified,
            i.workspace_name,
        ),
        None => (String::new(), String::new(), 0, 0, String::new()),
    };

    let rendered: Vec<StoredMessage> = messages.iter().map(stored_message_from).collect();

    SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        messages: rendered,
    }
}

fn ownership_conflict_error(state: &WebState) -> Option<ProtocolApiError> {
    let owner = state.ownership_snapshot();
    if owner.owner == SessionOwner::None {
        return None;
    }

    Some(ProtocolApiError::Conflict {
        reason: ownership_conflict_message(&owner),
    })
}

fn ownership_conflict_message(owner: &crate::state::SessionOwnership) -> String {
    format!(
        "Session is currently owned by {:?}{}",
        owner.owner,
        owner
            .session_id
            .as_deref()
            .map(|id| format!(" ({id})"))
            .unwrap_or_default()
    )
}

async fn mutation_guard(state: &WebState, session_id: &str, action: &str) -> Option<Response> {
    if state.is_streaming.load(Ordering::SeqCst) {
        return Some(
            (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: format!("A query is in progress — abort it before {action}"),
                    code: "engine_busy".into(),
                }),
            )
                .into_response(),
        );
    }
    if let Some(response) = ownership_conflict_response(state) {
        return Some(response);
    }
    if storage::load_session_info(session_id).is_err() {
        return Some(session_not_found_response());
    }
    None
}

fn load_session_messages_response(session_id: &str) -> Result<Vec<Message>, Response> {
    session_resume::resume_session(session_id).map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Session not found: {}", e),
                code: "session_not_found".into(),
            }),
        )
            .into_response()
    })
}

fn message_index(messages: &[Message], message_id: &str) -> Option<usize> {
    messages
        .iter()
        .position(|msg| msg.uuid().to_string() == message_id)
}

fn preceding_user_prompt(messages: &[Message], before_index: usize) -> Option<(usize, String)> {
    messages
        .iter()
        .enumerate()
        .take(before_index)
        .rev()
        .find_map(|(index, msg)| match msg {
            Message::User(user) => Some((index, user_message_text(user))),
            _ => None,
        })
}

fn user_message_text(user: &allthecodes_types::message::UserMessage) -> String {
    match &user.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn session_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "Session not found".into(),
            code: "session_not_found".into(),
        }),
    )
        .into_response()
}

fn message_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "Message not found".into(),
            code: "message_not_found".into(),
        }),
    )
        .into_response()
}

fn storage_error_response(prefix: &str, code: &str, error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: format!("{prefix}: {error}"),
            code: code.into(),
        }),
    )
        .into_response()
}

pub(crate) fn ownership_conflict_response(state: &WebState) -> Option<Response> {
    let owner = state.ownership_snapshot();
    if owner.owner == SessionOwner::None {
        return None;
    }
    Some(
        (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: ownership_conflict_message(&owner),
                code: "session_owned".into(),
            }),
        )
            .into_response(),
    )
}

/// Build a fresh engine that inherits the current engine's config, with an
/// optional seed message list. The new engine gets a freshly minted session id.
fn rebuild_engine(
    state: &WebState,
    seed: Option<Vec<Message>>,
    cwd: Option<String>,
) -> Arc<QueryEngine> {
    rebuild_engine_with_session_id_and_cwd(state, seed, None, cwd)
}

/// Rebuild with a caller-provided session id (used by resume so the engine's
/// auto-save keeps writing back to the resumed session file).
fn rebuild_engine_with_session_id(
    state: &WebState,
    seed: Option<Vec<Message>>,
    session_id: Option<&str>,
) -> Arc<QueryEngine> {
    rebuild_engine_with_session_id_and_cwd(state, seed, session_id, None)
}

fn rebuild_engine_with_session_id_and_cwd(
    state: &WebState,
    seed: Option<Vec<Message>>,
    session_id: Option<&str>,
    cwd: Option<String>,
) -> Arc<QueryEngine> {
    let current = state.engine();
    let mut cfg: QueryEngineConfig = current.config_ref().clone();
    cfg.initial_messages = seed;
    if let Some(cwd) = cwd {
        cfg.cwd = cwd;
    }

    let mut engine = QueryEngine::new(cfg);
    engine.set_hook_runner(current.hook_runner());
    engine.set_command_dispatcher(current.command_dispatcher());
    if let Some(id) = session_id {
        let id = SessionId::from_string(id);
        engine.session_id = id.clone();
        engine.set_current_session_id(id);
    }
    Arc::new(engine)
}

/// Convert an internal `Message` into the lightweight wire form used by the
/// session detail / resume responses.
fn stored_message_from(msg: &Message) -> StoredMessage {
    match msg {
        Message::User(u) => {
            let (text, blocks) = match &u.content {
                MessageContent::Text(t) => (t.clone(), None),
                MessageContent::Blocks(bs) => {
                    let text = bs
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    (text, Some(bs.clone()))
                }
            };
            StoredMessage {
                uuid: u.uuid.to_string(),
                timestamp: u.timestamp,
                role: "user".into(),
                content: text,
                content_blocks: blocks,
            }
        }
        Message::Assistant(a) => {
            let text = a
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            StoredMessage {
                uuid: a.uuid.to_string(),
                timestamp: a.timestamp,
                role: "assistant".into(),
                content: text,
                content_blocks: Some(a.content.clone()),
            }
        }
        Message::System(s) => StoredMessage {
            uuid: s.uuid.to_string(),
            timestamp: s.timestamp,
            role: "system".into(),
            content: s.content.clone(),
            content_blocks: None,
        },
        Message::Progress(p) => StoredMessage {
            uuid: p.uuid.to_string(),
            timestamp: p.timestamp,
            role: "progress".into(),
            content: String::new(),
            content_blocks: None,
        },
        Message::Attachment(a) => StoredMessage {
            uuid: a.uuid.to_string(),
            timestamp: a.timestamp,
            role: "attachment".into(),
            content: String::new(),
            content_blocks: None,
        },
    }
}
