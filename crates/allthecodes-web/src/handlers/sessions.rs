//! Session management handlers — list, detail, new, resume.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use allthecodes_protocol::v1::{
    SessionArchiveParams, SessionCreateParams, SessionDetailParams, SessionMessageActionParams,
    SessionModePatchParams, SessionResumeParams,
};
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::{ApiError as ProtocolApiError, NoParams, SerializationScope};
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use allthecodes_bootstrap::SessionId;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_session::{fork, resume as session_resume, storage};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::handlers::workspaces::resolve_workspace_root_protocol;
use crate::handlers::{
    chat_mode_preference_for_cwd_session, chat_mode_preference_for_session_info,
    normalize_optional_mode,
};
use crate::processors::Processor;
use crate::state::WebState;

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::SessionList, get(sessions_list_handler))
        .handle(ApiMethod::SessionCreate, post(session_new_handler))
        .handle(ApiMethod::SessionDetail, get(session_detail_handler))
        .handle(ApiMethod::SessionResume, post(session_resume_handler))
        .handle(ApiMethod::SessionArchive, post(session_archive_handler))
        .handle(
            ApiMethod::SessionModePatch,
            patch(session_mode_patch_handler),
        )
        .handle(
            ApiMethod::SessionMessageBranch,
            post(session_message_branch_handler),
        )
        .handle(
            ApiMethod::SessionMessageFeedback,
            post(session_message_feedback_handler),
        )
        .handle(
            ApiMethod::SessionMessageDelete,
            post(session_message_delete_handler),
        )
        .handle(
            ApiMethod::SessionMessageRegeneratePrepare,
            post(session_message_regenerate_prepare_handler),
        )
        .handle(
            ApiMethod::SessionMessageEditPrepare,
            post(session_message_edit_prepare_handler),
        )
        .handle(
            ApiMethod::SessionMessageRollbackPreview,
            post(session_message_rollback_preview_handler),
        )
        .handle(
            ApiMethod::SessionMessageRollback,
            post(session_message_rollback_handler),
        )
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Lightweight description of a workspace used to group sessions.
#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub key: String,
    pub root: String,
    pub name: String,
    pub default_chat_mode: String,
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
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
}

impl From<storage::SessionInfo> for SessionSummary {
    fn from(s: storage::SessionInfo) -> Self {
        let preference = chat_mode_preference_for_session_info(&s);
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
            default_chat_mode: preference.default_chat_mode,
            chat_mode_override: preference.chat_mode_override,
            effective_chat_mode: preference.effective_chat_mode,
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
    pub workspace_key: String,
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
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

#[derive(Deserialize)]
pub struct SessionModePatchRequest {
    #[serde(default)]
    pub chat_mode_override: Option<String>,
}

#[derive(Serialize)]
pub struct SessionModeResponse {
    pub session_id: String,
    pub workspace_key: String,
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
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
pub struct SessionCreateProcessor {
    state: WebState,
}

impl From<WebState> for SessionCreateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionCreateProcessor {
    type Request = SessionCreateParams;
    type Response = NewSessionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.create"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        if self.state.is_streaming.load(Ordering::SeqCst) {
            return Err(ProtocolApiError::EngineBusy);
        }

        let target_cwd = session_create_target_cwd(&self.state, &params)?;
        let engine = rebuild_engine(&self.state, None, target_cwd);
        let new_id = engine.current_session_id().to_string();
        self.state.replace_engine(engine);

        info!(session_id = %new_id, "POST /api/sessions/new");
        Ok(NewSessionResponse { session_id: new_id })
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
        // Check no conflicts in the same session
        if self.state.is_streaming.load(Ordering::SeqCst) {
            return Err(ProtocolApiError::EngineBusy);
        }

        info!(session_id = %params.id, "POST /api/sessions/:id/resume");

        let messages = load_session_messages(&params.id)?;
        let response = session_detail_from_messages(params.id.clone(), &messages);
        let engine = self
            .state
            .engine_for_session(&params.id)
            .unwrap_or_else(|| {
                rebuild_engine_with_session_id(&self.state, Some(messages), Some(&params.id))
            });
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
        // Check no conflicts in the same session
        if self.state.is_streaming.load(Ordering::SeqCst) {
            return Err(ProtocolApiError::EngineBusy);
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

#[derive(Clone)]
pub struct SessionModePatchProcessor {
    state: WebState,
}

impl From<WebState> for SessionModePatchProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionModePatchProcessor {
    type Request = SessionModePatchParams;
    type Response = SessionModeResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.mode_patch"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        update_session_mode_preference(&self.state, params.id, params.chat_mode_override)
    }
}

#[derive(Clone)]
pub struct SessionMessageBranchProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageBranchProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageBranchProcessor {
    type Request = SessionMessageActionParams;
    type Response = SessionBranchResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_branch"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = params.profile_id.as_deref();
        if let Some(error) = mutation_guard_error(&self.state, &params.id, "branching a message") {
            return Err(error);
        }

        let messages = load_session_messages(&params.id)?;
        if message_index(&messages, &params.message_id).is_none() {
            return Err(message_not_found_error(params.message_id));
        }
        let cwd = storage::load_session_info(&params.id)
            .map(|info| info.cwd)
            .unwrap_or_else(|_| self.state.engine().cwd().to_string());
        let new_id = SessionId::new().to_string();

        fork::fork_session(
            &params.id,
            &new_id,
            &messages,
            &cwd,
            Some(&params.message_id),
        )
        .map(|outcome| SessionBranchResponse {
            session_id: outcome.new_session_id,
            title: Some(outcome.title),
        })
        .map_err(|error| ProtocolApiError::Internal {
            message: format!("Failed to branch session: {error}"),
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageFeedbackProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageFeedbackProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageFeedbackProcessor {
    type Request = SessionMessageActionParams;
    type Response = SessionMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_feedback"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let rating = params.rating.as_deref().unwrap_or("none");
        if storage::load_session_info(&params.id).is_err() {
            return Err(session_not_found_error(params.id));
        }
        let messages = load_session_messages(&params.id)?;
        if message_index(&messages, &params.message_id).is_none() {
            return Err(message_not_found_error(params.message_id));
        }
        info!(session_id = %params.id, message_id = %params.message_id, rating = %rating, "message feedback recorded");
        Ok(SessionMutationResponse {
            ok: true,
            message: "Feedback recorded".into(),
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageDeleteProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageDeleteProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageDeleteProcessor {
    type Request = SessionMessageActionParams;
    type Response = SessionMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_delete"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = params.profile_id.as_deref();
        if let Some(error) = mutation_guard_error(&self.state, &params.id, "deleting a message") {
            return Err(error);
        }
        let messages = load_session_messages(&params.id)?;
        let Some(index) = message_index(&messages, &params.message_id) else {
            return Err(message_not_found_error(params.message_id));
        };
        storage::truncate_session(&params.id, index).map_err(|error| {
            ProtocolApiError::Internal {
                message: format!("Failed to delete message: {error}"),
            }
        })?;
        Ok(SessionMutationResponse {
            ok: true,
            message: "Message deleted".into(),
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageRegeneratePrepareProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageRegeneratePrepareProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageRegeneratePrepareProcessor {
    type Request = SessionMessageActionParams;
    type Response = MessageRegeneratePrepareResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_regenerate_prepare"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = params.profile_id.as_deref();
        if let Some(error) = mutation_guard_error(&self.state, &params.id, "regenerating a message")
        {
            return Err(error);
        }
        let messages = load_session_messages(&params.id)?;
        let Some(assistant_index) = message_index(&messages, &params.message_id) else {
            return Err(message_not_found_error(params.message_id));
        };
        let Some((user_index, prompt)) = preceding_user_prompt(&messages, assistant_index) else {
            return Err(ProtocolApiError::BadRequest {
                code: "message_regenerate_unavailable",
                message: "No preceding user message found for regeneration".to_string(),
            });
        };
        storage::truncate_session(&params.id, user_index).map_err(|error| {
            ProtocolApiError::Internal {
                message: format!("Failed to prepare regeneration: {error}"),
            }
        })?;
        Ok(MessageRegeneratePrepareResponse {
            session_id: params.id,
            prompt,
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageEditPrepareProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageEditPrepareProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageEditPrepareProcessor {
    type Request = SessionMessageActionParams;
    type Response = MessageRegeneratePrepareResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_edit_prepare"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        if let Some(error) = mutation_guard_error(&self.state, &params.id, "editing a message") {
            return Err(error);
        }
        let edited_text = params.text.as_deref().unwrap_or("").trim().to_string();
        if edited_text.is_empty() {
            return Err(ProtocolApiError::BadRequest {
                code: "message_edit_empty",
                message: "Edited message text is required".to_string(),
            });
        }
        let messages = load_session_messages(&params.id)?;
        let Some(index) = message_index(&messages, &params.message_id) else {
            return Err(message_not_found_error(params.message_id));
        };
        if !matches!(messages.get(index), Some(Message::User(_))) {
            return Err(ProtocolApiError::BadRequest {
                code: "message_edit_role_invalid",
                message: "Only user messages can be edited".to_string(),
            });
        }
        storage::truncate_session(&params.id, index).map_err(|error| {
            ProtocolApiError::Internal {
                message: format!("Failed to prepare edit: {error}"),
            }
        })?;
        Ok(MessageRegeneratePrepareResponse {
            session_id: params.id,
            prompt: edited_text,
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageRollbackPreviewProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageRollbackPreviewProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageRollbackPreviewProcessor {
    type Request = SessionMessageActionParams;
    type Response = RollbackPreviewResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_rollback_preview"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = params.profile_id.as_deref();
        let messages = load_session_messages(&params.id)?;
        if message_index(&messages, &params.message_id).is_none() {
            return Err(message_not_found_error(params.message_id));
        }
        Ok(RollbackPreviewResponse {
            available: false,
            message: Some("Rollback checkpoint is not available for this session yet".into()),
            files: Vec::new(),
        })
    }
}

#[derive(Clone)]
pub struct SessionMessageRollbackProcessor {
    state: WebState,
}

impl From<WebState> for SessionMessageRollbackProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SessionMessageRollbackProcessor {
    type Request = SessionMessageActionParams;
    type Response = serde_json::Value;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "session.message_rollback"
    }

    fn serialization_layer(&self) -> Option<crate::serialization::SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _files = params.files;
        Err(ProtocolApiError::NotImplemented {
            capability: "rollback_checkpoint_unavailable".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// GET /api/sessions -- List all sessions with workspace grouping metadata.
pub async fn sessions_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<SessionListProcessor>(state, ApiMethod::SessionList, NoParams {})
        .await
}

/// GET /api/sessions/:id -- Load a session's message history for preview.
pub async fn session_detail_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    rest_processor_response::<SessionDetailProcessor>(
        state,
        ApiMethod::SessionDetail,
        SessionDetailParams { id },
    )
    .await
}

/// POST /api/sessions/new -- Start a fresh session in the current workspace.
pub async fn session_new_handler(
    State(state): State<WebState>,
    body: Option<Json<NewSessionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionCreateProcessor>(
        state,
        ApiMethod::SessionCreate,
        session_create_params_from_body(body),
    )
    .await
}

/// POST /api/sessions/:id/resume -- Load an existing session into the engine.
pub async fn session_resume_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    rest_processor_response::<SessionResumeProcessor>(
        state,
        ApiMethod::SessionResume,
        SessionResumeParams { id },
    )
    .await
}

/// POST /api/sessions/:id/archive -- Hide a saved session from the default list.
pub async fn session_archive_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    rest_processor_response::<SessionArchiveProcessor>(
        state,
        ApiMethod::SessionArchive,
        SessionArchiveParams { id },
    )
    .await
}

/// PATCH /api/sessions/:id/mode -- Set or clear the session chat mode override.
pub async fn session_mode_patch_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<SessionModePatchRequest>,
) -> Response {
    rest_processor_response::<SessionModePatchProcessor>(
        state,
        ApiMethod::SessionModePatch,
        SessionModePatchParams {
            id,
            chat_mode_override: req.chat_mode_override,
        },
    )
    .await
}

fn update_session_mode_preference(
    state: &WebState,
    id: String,
    chat_mode_override: Option<String>,
) -> Result<SessionModeResponse, ProtocolApiError> {
    let cwd = match storage::load_session_info(&id) {
        Ok(info) => info.cwd,
        Err(_) if state.engine().current_session_id().to_string() == id => {
            state.engine().cwd().to_string()
        }
        Err(_) => {
            return Err(ProtocolApiError::NotFound {
                entity: "session",
                id,
            });
        }
    };
    let normalized = normalize_optional_mode(chat_mode_override.as_deref());

    storage::set_session_chat_mode_override(&id, normalized.as_deref(), &cwd).map_err(|error| {
        ProtocolApiError::Internal {
            message: format!("Failed to update session mode: {error}"),
        }
    })?;
    let preference = chat_mode_preference_for_cwd_session(&cwd, &id);
    Ok(SessionModeResponse {
        session_id: id,
        workspace_key: preference.workspace_key,
        default_chat_mode: preference.default_chat_mode,
        chat_mode_override: preference.chat_mode_override,
        effective_chat_mode: preference.effective_chat_mode,
    })
}

fn session_create_params_from_body(body: Option<Json<NewSessionRequest>>) -> SessionCreateParams {
    match body {
        Some(Json(req)) => SessionCreateParams {
            title: None,
            workspace_key: req.workspace_key,
            cwd: req.cwd,
        },
        None => SessionCreateParams {
            title: None,
            workspace_key: None,
            cwd: None,
        },
    }
}

fn session_create_target_cwd(
    state: &WebState,
    params: &SessionCreateParams,
) -> Result<Option<String>, ProtocolApiError> {
    match (params.workspace_key.as_deref(), params.cwd.as_deref()) {
        (Some(workspace_key), cwd) => {
            let root = resolve_workspace_root_protocol(state, workspace_key, cwd)?;
            Ok(Some(root.to_string_lossy().to_string()))
        }
        (None, Some(cwd)) => {
            let cwd_path = Path::new(cwd);
            match cwd_path.canonicalize() {
                Ok(root) if root.is_dir() => Ok(Some(root.to_string_lossy().to_string())),
                _ => Err(ProtocolApiError::BadRequest {
                    code: "cwd_invalid",
                    message: "cwd must be an existing directory".to_string(),
                }),
            }
        }
        (None, None) => Ok(None),
    }
}

fn session_message_action_params(
    id: String,
    message_id: String,
    body: Option<Json<MessageActionRequest>>,
) -> SessionMessageActionParams {
    let body = body.map(|Json(req)| req);
    SessionMessageActionParams {
        id,
        message_id,
        profile_id: body.as_ref().and_then(|req| req.profile_id.clone()),
        rating: body.as_ref().and_then(|req| req.rating.clone()),
        text: body.as_ref().and_then(|req| req.text.clone()),
        files: body.map(|req| req.files).unwrap_or_default(),
    }
}

/// POST /api/sessions/:id/messages/:message_id/branch
pub async fn session_message_branch_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageBranchProcessor>(
        state,
        ApiMethod::SessionMessageBranch,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/feedback
pub async fn session_message_feedback_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageFeedbackProcessor>(
        state,
        ApiMethod::SessionMessageFeedback,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/delete
pub async fn session_message_delete_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageDeleteProcessor>(
        state,
        ApiMethod::SessionMessageDelete,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/regenerate/prepare
pub async fn session_message_regenerate_prepare_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageRegeneratePrepareProcessor>(
        state,
        ApiMethod::SessionMessageRegeneratePrepare,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/edit/prepare
pub async fn session_message_edit_prepare_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageEditPrepareProcessor>(
        state,
        ApiMethod::SessionMessageEditPrepare,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/rollback/preview
pub async fn session_message_rollback_preview_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageRollbackPreviewProcessor>(
        state,
        ApiMethod::SessionMessageRollbackPreview,
        session_message_action_params(id, message_id, body),
    )
    .await
}

/// POST /api/sessions/:id/messages/:message_id/rollback
pub async fn session_message_rollback_handler(
    AxumPath((id, message_id)): AxumPath<(String, String)>,
    State(state): State<WebState>,
    body: Option<Json<MessageActionRequest>>,
) -> impl IntoResponse {
    rest_processor_response::<SessionMessageRollbackProcessor>(
        state,
        ApiMethod::SessionMessageRollback,
        session_message_action_params(id, message_id, body),
    )
    .await
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
            default_chat_mode: crate::handlers::workspace_default_chat_mode(&ws_key),
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
    let info = storage::load_session_info(&id).ok();

    let (title, cwd, created_at, last_modified, workspace_name, workspace_key, preference) =
        match info {
            Some(i) => {
                let preference = chat_mode_preference_for_session_info(&i);
                (
                    i.title,
                    i.cwd,
                    i.created_at,
                    i.last_modified,
                    i.workspace_name,
                    i.workspace_key,
                    preference,
                )
            }
            None => {
                let preference = chat_mode_preference_for_cwd_session("", &id);
                (
                    String::new(),
                    String::new(),
                    0,
                    0,
                    String::new(),
                    preference.workspace_key.clone(),
                    preference,
                )
            }
        };

    let rendered: Vec<StoredMessage> = messages.iter().map(stored_message_from).collect();

    SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        workspace_key,
        default_chat_mode: preference.default_chat_mode,
        chat_mode_override: preference.chat_mode_override,
        effective_chat_mode: preference.effective_chat_mode,
        messages: rendered,
    }
}

fn mutation_guard_error(
    state: &WebState,
    session_id: &str,
    _action: &str,
) -> Option<ProtocolApiError> {
    if state.is_streaming.load(Ordering::SeqCst) {
        return Some(ProtocolApiError::EngineBusy);
    }
    if storage::load_session_info(session_id).is_err() {
        return Some(session_not_found_error(session_id.to_string()));
    }
    None
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

fn session_not_found_error(id: String) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "session",
        id,
    }
}

fn message_not_found_error(id: String) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "message",
        id,
    }
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

pub(crate) fn build_engine_for_session(
    state: &WebState,
    session_id: &str,
) -> Result<Arc<QueryEngine>, ProtocolApiError> {
    let messages = load_session_messages(session_id)?;
    let cwd = storage::load_session_info(session_id)
        .ok()
        .map(|info| info.cwd);
    Ok(rebuild_engine_with_session_id_and_cwd(
        state,
        Some(messages),
        Some(session_id),
        cwd,
    ))
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
    let mut app_state = current.app_state();
    app_state.tool_permission_context.clear_session_grants();
    engine.update_app_state(|state| *state = app_state);
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
