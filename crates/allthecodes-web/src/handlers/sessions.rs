//! Session management handlers — list, detail, new, resume.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tracing::{info, warn};

use allthecodes_bootstrap::SessionId;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_session::{resume as session_resume, storage};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

use crate::handlers::ApiError;
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

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// GET /api/sessions -- List all sessions with workspace grouping metadata.
pub async fn sessions_list_handler(State(state): State<WebState>) -> impl IntoResponse {
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

    Json(SessionListResponse {
        current_workspace: WorkspaceInfo {
            key: ws_key,
            root: ws_root.to_string_lossy().to_string(),
            name: ws_name,
        },
        active_session_id: engine.current_session_id().to_string(),
        sessions: summaries,
    })
}

/// GET /api/sessions/:id -- Load a session's message history for preview.
pub async fn session_detail_handler(
    AxumPath(id): AxumPath<String>,
    State(_state): State<WebState>,
) -> impl IntoResponse {
    info!(session_id = %id, "GET /api/sessions/:id");

    let messages = match session_resume::resume_session(&id) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Session not found: {}", e),
                    code: "session_not_found".into(),
                }),
            )
                .into_response();
        }
    };

    // Look up disk metadata for title / cwd / timestamps.
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

    Json(SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        messages: rendered,
    })
    .into_response()
}

/// POST /api/sessions/new -- Start a fresh session in the current workspace.
pub async fn session_new_handler(State(state): State<WebState>) -> impl IntoResponse {
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

    let engine = rebuild_engine(&state, None);
    let new_id = engine.current_session_id().to_string();
    state.replace_engine(engine);

    info!(session_id = %new_id, "POST /api/sessions/new");
    Json(NewSessionResponse { session_id: new_id }).into_response()
}

/// POST /api/sessions/:id/resume -- Load an existing session into the engine.
pub async fn session_resume_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is in progress — abort it before switching sessions".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }
    if let Some(response) = ownership_conflict_response(&state) {
        return response;
    }

    info!(session_id = %id, "POST /api/sessions/:id/resume");

    let messages = match session_resume::resume_session(&id) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Session not found: {}", e),
                    code: "session_not_found".into(),
                }),
            )
                .into_response();
        }
    };

    let engine = rebuild_engine_with_session_id(&state, Some(messages.clone()), Some(&id));
    state.replace_engine(engine);

    let rendered: Vec<StoredMessage> = messages.iter().map(stored_message_from).collect();

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

    Json(SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        messages: rendered,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ownership_conflict_response(state: &WebState) -> Option<Response> {
    let owner = state.ownership_snapshot();
    if owner.owner == SessionOwner::None {
        return None;
    }
    Some(
        (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!(
                    "Session is currently owned by {:?}{}",
                    owner.owner,
                    owner
                        .session_id
                        .as_deref()
                        .map(|id| format!(" ({id})"))
                        .unwrap_or_default()
                ),
                code: "session_owned".into(),
            }),
        )
            .into_response(),
    )
}

/// Build a fresh engine that inherits the current engine's config, with an
/// optional seed message list. The new engine gets a freshly minted session id.
fn rebuild_engine(state: &WebState, seed: Option<Vec<Message>>) -> Arc<QueryEngine> {
    rebuild_engine_with_session_id(state, seed, None)
}

/// Rebuild with a caller-provided session id (used by resume so the engine's
/// auto-save keeps writing back to the resumed session file).
fn rebuild_engine_with_session_id(
    state: &WebState,
    seed: Option<Vec<Message>>,
    session_id: Option<&str>,
) -> Arc<QueryEngine> {
    let current = state.engine();
    let mut cfg: QueryEngineConfig = current.config_ref().clone();
    cfg.initial_messages = seed;

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
