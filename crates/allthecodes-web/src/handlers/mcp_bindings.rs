//! MCP binding REST handlers.

use std::path::PathBuf;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_mcp::bindings::{self, BindingSelector};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_types::mcp::{McpBinding, McpPermission, McpToolScope};

use crate::state::WebState;

#[derive(Serialize)]
pub struct McpBindingsListResponse {
    pub bindings: Vec<McpBinding>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum McpBindingUpsertRequest {
    Wrapped { binding: McpBinding },
    Binding(McpBinding),
}

impl McpBindingUpsertRequest {
    fn into_binding(self) -> McpBinding {
        match self {
            Self::Wrapped { binding } | Self::Binding(binding) => binding,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpBindingSelectorQuery {
    pub scope: McpToolScope,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpBindingPermissionsRequest {
    pub scope: McpToolScope,
    pub permissions: Vec<McpPermission>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

/// GET /api/mcp-bindings
pub async fn mcp_bindings_list_handler(State(state): State<WebState>) -> Response {
    let cwd = engine_cwd(&state);
    let session_id = current_session_id(&state);
    match binding_snapshot(&cwd, Some(&session_id)) {
        Ok(bindings) => Json(McpBindingsListResponse { bindings }).into_response(),
        Err(error) => internal_error(error),
    }
}

/// POST /api/mcp-bindings
pub async fn mcp_bindings_create_handler(
    State(state): State<WebState>,
    Json(req): Json<McpBindingUpsertRequest>,
) -> Response {
    let cwd = engine_cwd(&state);
    let session_id = current_session_id(&state);
    let binding = fill_binding_defaults(req.into_binding(), &session_id);
    match bindings::upsert_binding(&cwd, binding) {
        Ok(()) => binding_snapshot_response(&cwd, Some(&session_id)),
        Err(error) => validation_error(error.to_string()),
    }
}

/// PATCH /api/mcp-bindings/{server_id}
pub async fn mcp_bindings_update_handler(
    State(state): State<WebState>,
    AxumPath(server_id): AxumPath<String>,
    Json(req): Json<McpBindingPermissionsRequest>,
) -> Response {
    let cwd = engine_cwd(&state);
    let session_id = current_session_id(&state);
    let selector = selector_from_parts(
        server_id.clone(),
        req.scope,
        req.session_id,
        req.thread_id,
        &session_id,
    );
    match bindings::set_binding_permissions(&cwd, selector, req.permissions) {
        Ok(true) => binding_snapshot_response(&cwd, Some(&session_id)),
        Ok(false) => not_found(format!("MCP binding `{server_id}` not found")),
        Err(error) => validation_error(error.to_string()),
    }
}

/// DELETE /api/mcp-bindings/{server_id}?scope=session
pub async fn mcp_bindings_delete_handler(
    State(state): State<WebState>,
    AxumPath(server_id): AxumPath<String>,
    Query(query): Query<McpBindingSelectorQuery>,
) -> Response {
    let cwd = engine_cwd(&state);
    let session_id = current_session_id(&state);
    let selector = selector_from_parts(
        server_id.clone(),
        query.scope,
        query.session_id,
        query.thread_id,
        &session_id,
    );
    match bindings::remove_binding(&cwd, selector) {
        Ok(true) => binding_snapshot_response(&cwd, Some(&session_id)),
        Ok(false) => not_found(format!("MCP binding `{server_id}` not found")),
        Err(error) => validation_error(error.to_string()),
    }
}

fn fill_binding_defaults(mut binding: McpBinding, current_session_id: &str) -> McpBinding {
    match binding.scope {
        McpToolScope::Global | McpToolScope::Project => {}
        McpToolScope::Session => {
            if binding
                .session_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                binding.session_id = Some(current_session_id.to_string());
            }
            binding.thread_id = None;
        }
        McpToolScope::Thread => {
            if binding
                .session_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                binding.session_id = Some(current_session_id.to_string());
            }
        }
    }
    binding
}

fn selector_from_parts(
    server_id: String,
    scope: McpToolScope,
    session_id: Option<String>,
    thread_id: Option<String>,
    current_session_id: &str,
) -> BindingSelector {
    BindingSelector::new(
        server_id,
        scope,
        matches!(scope, McpToolScope::Session | McpToolScope::Thread)
            .then(|| session_id.unwrap_or_else(|| current_session_id.to_string())),
        thread_id,
    )
}

fn binding_snapshot_response(cwd: &std::path::Path, session_id: Option<&str>) -> Response {
    match binding_snapshot(cwd, session_id) {
        Ok(bindings) => Json(McpBindingsListResponse { bindings }).into_response(),
        Err(error) => internal_error(error),
    }
}

fn binding_snapshot(
    cwd: &std::path::Path,
    session_id: Option<&str>,
) -> Result<Vec<McpBinding>, String> {
    let discovered = allthecodes_mcp::discovery::discover_bound_mcp_servers(cwd, session_id)
        .map_err(|error| error.to_string())?;
    let bindings = discovered.bindings;
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        if let Ok(mut manager) = manager.try_lock() {
            manager.set_bindings(bindings.clone());
        }
    }
    Ok(bindings)
}

fn engine_cwd(state: &WebState) -> PathBuf {
    PathBuf::from(state.engine().cwd())
}

fn current_session_id(state: &WebState) -> String {
    state.engine().current_session_id().to_string()
}

fn validation_error(error: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "validation_error",
        message: error,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "mcp_binding",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
