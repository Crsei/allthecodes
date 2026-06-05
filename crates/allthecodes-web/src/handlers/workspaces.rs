//! Workspace project handlers for the web sidebar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{info, warn};

use allthecodes_session::storage::{self, SessionInfo};

use crate::handlers::sessions::ownership_conflict_response;
use crate::handlers::ApiError;
use crate::state::WebState;
use crate::workspace_metadata::{self, WorkspaceUiMetadata, WorkspaceUiMetadataPatch};

#[derive(Serialize)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Serialize)]
pub struct WorkspaceSummary {
    pub key: String,
    pub root: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub pinned: bool,
    pub hidden: bool,
    pub session_count: usize,
    pub last_modified: i64,
}

#[derive(Deserialize)]
pub struct WorkspacePatchRequest {
    #[serde(default)]
    pub display_name: Option<Option<String>>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

#[derive(Deserialize)]
pub struct WorkspaceOpenRequest {
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceArchiveRequest {
    #[serde(default)]
    pub include_active: bool,
}

#[derive(Serialize)]
pub struct WorkspaceMutationResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct WorkspaceArchiveResponse {
    pub ok: bool,
    pub archived: Vec<String>,
    pub skipped: Vec<WorkspaceArchiveSkip>,
    pub failed: Vec<WorkspaceArchiveFailure>,
}

#[derive(Serialize)]
pub struct WorkspaceArchiveSkip {
    pub session_id: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct WorkspaceArchiveFailure {
    pub session_id: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub(crate) struct KnownWorkspace {
    pub key: String,
    pub root: PathBuf,
    pub name: String,
    pub session_count: usize,
    pub last_modified: i64,
}

/// GET /api/workspaces -- List known workspaces with persisted UI metadata.
pub async fn workspaces_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let known = known_workspaces(&state);
    let metadata = workspace_metadata::load_metadata().unwrap_or_else(|error| {
        warn!(%error, "failed to load workspace metadata");
        HashMap::new()
    });

    let mut workspaces: Vec<WorkspaceSummary> = known
        .values()
        .map(|workspace| workspace_summary(workspace, metadata.get(&workspace.key)))
        .collect();

    workspaces.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.last_modified.cmp(&a.last_modified))
            .then_with(|| a.name.cmp(&b.name))
    });

    Json(WorkspacesResponse { workspaces }).into_response()
}

/// PATCH /api/workspaces/:workspace_key -- Persist sidebar metadata.
pub async fn workspace_patch_handler(
    AxumPath(workspace_key): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<WorkspacePatchRequest>,
) -> impl IntoResponse {
    let known = known_workspaces(&state);
    let Some(workspace) = known.get(&workspace_key) else {
        return workspace_not_found();
    };

    let patch = WorkspaceUiMetadataPatch {
        display_name: req.display_name,
        pinned: req.pinned,
        hidden: req.hidden,
    };
    let metadata = match workspace_metadata::update_metadata(&workspace_key, patch) {
        Ok(metadata) => metadata,
        Err(error) => {
            warn!(%error, workspace_key = %workspace_key, "failed to update workspace metadata");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("Failed to update workspace metadata: {error}"),
                    code: "workspace_metadata_failed".into(),
                }),
            )
                .into_response();
        }
    };

    Json(workspace_summary(workspace, Some(&metadata))).into_response()
}

/// POST /api/workspaces/:workspace_key/open -- Open a workspace directory.
pub async fn workspace_open_handler(
    AxumPath(workspace_key): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<WorkspaceOpenRequest>,
) -> impl IntoResponse {
    let root = match resolve_workspace_root(&state, &workspace_key, req.root.as_deref()) {
        Ok(root) => root,
        Err(response) => return response,
    };

    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(&root);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(&root);
        command
    } else {
        let mut command = Command::new("code");
        command.arg(&root);
        command
    };

    match command.spawn() {
        Ok(_) => {
            info!(workspace_key = %workspace_key, root = %root.display(), "opened workspace");
            Json(WorkspaceMutationResponse {
                ok: true,
                message: "Workspace opened".into(),
            })
            .into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: if cfg!(target_os = "linux") {
                    format!("Failed to open workspace in VS Code with `code`: {error}")
                } else {
                    format!("Failed to open workspace: {error}")
                },
                code: "workspace_open_failed".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/workspaces/:workspace_key/sessions/archive -- Archive sessions in one workspace.
pub async fn workspace_sessions_archive_handler(
    AxumPath(workspace_key): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<WorkspaceArchiveRequest>,
) -> impl IntoResponse {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is in progress — abort it before archiving sessions".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }
    if let Some(response) = ownership_conflict_response(&state) {
        return response;
    }

    let sessions = match storage::list_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            warn!(%error, "failed to list sessions for workspace archive");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("Failed to list sessions: {error}"),
                    code: "session_list_failed".into(),
                }),
            )
                .into_response();
        }
    };

    let matching: Vec<SessionInfo> = sessions
        .into_iter()
        .filter(|session| session.workspace_key == workspace_key)
        .collect();
    if matching.is_empty() && !known_workspaces(&state).contains_key(&workspace_key) {
        return workspace_not_found();
    }

    let active_id = state.engine().current_session_id().to_string();
    let mut archived = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();

    for session in matching {
        if session.session_id == active_id && !req.include_active {
            skipped.push(WorkspaceArchiveSkip {
                session_id: session.session_id,
                reason: "active_session".into(),
            });
            continue;
        }
        if session.session_id == active_id && req.include_active {
            skipped.push(WorkspaceArchiveSkip {
                session_id: session.session_id,
                reason: "active_session".into(),
            });
            continue;
        }

        match storage::archive_session(&session.session_id) {
            Ok(()) => archived.push(session.session_id),
            Err(error) => failed.push(WorkspaceArchiveFailure {
                session_id: session.session_id,
                error: error.to_string(),
            }),
        }
    }

    Json(WorkspaceArchiveResponse {
        ok: failed.is_empty(),
        archived,
        skipped,
        failed,
    })
    .into_response()
}

pub(crate) fn known_workspaces(state: &WebState) -> HashMap<String, KnownWorkspace> {
    let mut by_key: HashMap<String, KnownWorkspace> = HashMap::new();

    let engine = state.engine();
    let current_root = storage::workspace_root(Path::new(engine.cwd()));
    let current_key = storage::workspace_key(Path::new(engine.cwd()));
    by_key.insert(
        current_key.clone(),
        KnownWorkspace {
            key: current_key,
            root: current_root.clone(),
            name: storage::workspace_name(&current_root),
            session_count: 0,
            last_modified: 0,
        },
    );

    if let Ok(sessions) = storage::list_sessions() {
        for session in sessions {
            let key = session.workspace_key.clone();
            let root = PathBuf::from(&session.workspace_root);
            let entry = by_key.entry(key.clone()).or_insert_with(|| KnownWorkspace {
                key,
                root: root.clone(),
                name: session.workspace_name.clone(),
                session_count: 0,
                last_modified: 0,
            });
            entry.session_count += 1;
            entry.last_modified = entry.last_modified.max(session.last_modified);
            if entry.root.as_os_str().is_empty() {
                entry.root = root;
            }
            if entry.name.is_empty() {
                entry.name = session.workspace_name;
            }
        }
    }

    by_key
}

pub(crate) fn resolve_workspace_root(
    state: &WebState,
    workspace_key: &str,
    requested_root: Option<&str>,
) -> Result<PathBuf, Response> {
    let known = known_workspaces(state);
    let Some(workspace) = known.get(workspace_key) else {
        return Err(workspace_not_found());
    };

    let root = match requested_root {
        Some(root) => {
            let candidate = validate_local_dir(root)?;
            let candidate_key = storage::workspace_key(&candidate);
            if candidate_key != workspace_key {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error: "Workspace root does not match workspace key".into(),
                        code: "workspace_root_mismatch".into(),
                    }),
                )
                    .into_response());
            }
            candidate
        }
        None => workspace.root.clone(),
    };

    validate_local_dir_path(&root)?;
    Ok(root)
}

fn validate_local_dir(raw: &str) -> Result<PathBuf, Response> {
    if raw.contains("://") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Workspace root must be a local directory path".into(),
                code: "workspace_root_invalid".into(),
            }),
        )
            .into_response());
    }
    validate_local_dir_path(Path::new(raw))
}

fn validate_local_dir_path(path: &Path) -> Result<PathBuf, Response> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!("Workspace root is not accessible: {error}"),
                code: "workspace_root_invalid".into(),
            }),
        )
            .into_response()
    })?;

    if !canonical.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Workspace root must be a directory".into(),
                code: "workspace_root_invalid".into(),
            }),
        )
            .into_response());
    }

    Ok(canonical)
}

fn workspace_summary(
    workspace: &KnownWorkspace,
    metadata: Option<&WorkspaceUiMetadata>,
) -> WorkspaceSummary {
    WorkspaceSummary {
        key: workspace.key.clone(),
        root: workspace.root.to_string_lossy().to_string(),
        name: workspace.name.clone(),
        display_name: metadata.and_then(|m| m.display_name.clone()),
        pinned: metadata.map(|m| m.pinned).unwrap_or(false),
        hidden: metadata.map(|m| m.hidden).unwrap_or(false),
        session_count: workspace.session_count,
        last_modified: workspace.last_modified,
    }
}

fn workspace_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "Workspace not found".into(),
            code: "workspace_not_found".into(),
        }),
    )
        .into_response()
}
