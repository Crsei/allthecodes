//! Agent definition REST handlers.

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_ipc_protocol::subsystem_types::{
    AgentDefinitionEntry, AgentDefinitionSource, AgentToolInfo,
};
use allthecodes_services::agent_definitions;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Serialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentDefinitionEntry>,
    pub tools: Vec<AgentToolInfo>,
}

#[derive(Deserialize)]
pub struct AgentUpsertRequest {
    pub entry: AgentDefinitionEntry,
}

#[derive(Deserialize)]
pub struct AgentDeleteQuery {
    pub source: String,
}

#[derive(Serialize)]
pub struct AgentRestoreResponse {
    pub removed_overrides: usize,
    pub agents: Vec<AgentDefinitionEntry>,
}

/// GET /api/agents
pub async fn agents_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let cwd = engine_cwd(&state);
    Json(AgentsListResponse {
        agents: agent_definitions::list_all_agents(&cwd),
        tools: agent_definitions::available_tools(),
    })
}

/// GET /api/agents/{name}
pub async fn agents_detail_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    match find_agent(&cwd, &name) {
        Some(agent) => Json(agent).into_response(),
        None => not_found(format!("Agent '{}' not found", name)),
    }
}

/// POST /api/agents
pub async fn agents_create_handler(
    State(state): State<WebState>,
    Json(req): Json<AgentUpsertRequest>,
) -> Response {
    save_agent(&state, req.entry)
}

/// PATCH /api/agents/{name}
pub async fn agents_update_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<AgentUpsertRequest>,
) -> Response {
    if req.entry.name != name {
        return validation_error(format!(
            "Agent name '{}' does not match path '{}'",
            req.entry.name, name
        ));
    }
    save_agent(&state, req.entry)
}

/// DELETE /api/agents/{name}?source=user|project
pub async fn agents_delete_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<AgentDeleteQuery>,
) -> Response {
    let source = match parse_agent_source(&query.source) {
        Ok(source) => source,
        Err(error) => return validation_error(error),
    };
    let cwd = engine_cwd(&state);
    match agent_definitions::delete_agent(&cwd, &name, &source) {
        Ok(()) => Json(AgentsListResponse {
            agents: agent_definitions::list_all_agents(&cwd),
            tools: agent_definitions::available_tools(),
        })
        .into_response(),
        Err(error) => service_error(error),
    }
}

/// POST /api/agents/{name}/restore
pub async fn agents_restore_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    match agent_definitions::restore_agent_overrides(&cwd, &name) {
        Ok(removed_overrides) => Json(AgentRestoreResponse {
            removed_overrides,
            agents: agent_definitions::list_all_agents(&cwd),
        })
        .into_response(),
        Err(error) => validation_error(error),
    }
}

fn save_agent(state: &WebState, entry: AgentDefinitionEntry) -> Response {
    let cwd = engine_cwd(state);
    match agent_definitions::upsert_agent(&cwd, entry) {
        Ok(saved) => Json(saved).into_response(),
        Err((_name, error)) => service_error(error),
    }
}

fn engine_cwd(state: &WebState) -> PathBuf {
    PathBuf::from(state.engine().cwd())
}

fn find_agent(cwd: &Path, name: &str) -> Option<AgentDefinitionEntry> {
    agent_definitions::list_all_agents(cwd)
        .into_iter()
        .filter(|agent| agent.name == name)
        .max_by_key(|agent| source_rank(&agent.source))
}

fn source_rank(source: &AgentDefinitionSource) -> u8 {
    match source {
        AgentDefinitionSource::Project => 3,
        AgentDefinitionSource::User => 2,
        AgentDefinitionSource::Builtin => 1,
        AgentDefinitionSource::Plugin { .. } => 0,
    }
}

fn parse_agent_source(raw: &str) -> Result<AgentDefinitionSource, String> {
    match raw {
        "user" => Ok(AgentDefinitionSource::User),
        "project" => Ok(AgentDefinitionSource::Project),
        "builtin" | "built-in" => Ok(AgentDefinitionSource::Builtin),
        value if value.starts_with("plugin:") => Ok(AgentDefinitionSource::Plugin {
            id: value.trim_start_matches("plugin:").to_string(),
        }),
        _ => Err("source must be one of `user` or `project`".to_string()),
    }
}

fn service_error(error: String) -> Response {
    if error.starts_with("failed to") {
        internal_error(error)
    } else {
        validation_error(error)
    }
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
        entity: "agent",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
