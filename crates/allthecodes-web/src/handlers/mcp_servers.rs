//! MCP server settings REST handlers.

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use allthecodes_config::settings::{user_settings_path, write_settings_file, RawSettings};
use allthecodes_ipc_protocol::subsystem_types::{ConfigScope, McpServerConfigEntry};
use allthecodes_mcp::discovery::{discover_mcp_servers_scoped, DiscoveryScope};
use allthecodes_mcp::McpServerConfig;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Serialize)]
pub struct McpServersListResponse {
    pub servers: Vec<McpServerConfigEntry>,
}

#[derive(Serialize)]
pub struct McpServersMarketplaceResponse {
    pub servers: Vec<Value>,
    pub capabilities: McpServersMarketplaceCapabilities,
}

#[derive(Serialize)]
pub struct McpServersMarketplaceCapabilities {
    pub refresh: bool,
    pub install: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum McpServerUpsertRequest {
    Wrapped { entry: McpServerConfigEntry },
    Entry(McpServerConfigEntry),
}

impl McpServerUpsertRequest {
    fn into_entry(self) -> McpServerConfigEntry {
        match self {
            Self::Wrapped { entry } | Self::Entry(entry) => entry,
        }
    }
}

#[derive(Deserialize)]
pub struct McpServerDeleteQuery {
    pub scope: String,
}

/// GET /api/mcp-servers
pub async fn mcp_servers_list_handler(State(state): State<WebState>) -> Response {
    let cwd = engine_cwd(&state);
    match list_entries(&cwd) {
        Ok(servers) => Json(McpServersListResponse { servers }).into_response(),
        Err(error) => internal_error(error),
    }
}

/// GET /api/mcp-servers/{name}
pub async fn mcp_servers_detail_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    match list_entries(&cwd) {
        Ok(servers) => servers
            .into_iter()
            .rev()
            .find(|server| server.name == name)
            .map(Json)
            .map(IntoResponse::into_response)
            .unwrap_or_else(|| not_found(format!("MCP server '{}' not found", name))),
        Err(error) => internal_error(error),
    }
}

/// POST /api/mcp-servers
pub async fn mcp_servers_create_handler(
    State(state): State<WebState>,
    Json(req): Json<McpServerUpsertRequest>,
) -> Response {
    let entry = req.into_entry();
    match upsert_entry(&state, entry) {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => crate::api_errors::protocol_error_response(error).into_response(),
    }
}

/// PATCH /api/mcp-servers/{name}
pub async fn mcp_servers_update_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<McpServerUpsertRequest>,
) -> Response {
    let entry = req.into_entry();
    if entry.name != name {
        return validation_error(format!(
            "MCP server name '{}' does not match path '{}'",
            entry.name, name
        ));
    }
    match upsert_entry(&state, entry) {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => crate::api_errors::protocol_error_response(error).into_response(),
    }
}

/// DELETE /api/mcp-servers/{name}?scope=user|project
pub async fn mcp_servers_delete_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<McpServerDeleteQuery>,
) -> Response {
    let scope = match parse_scope(&query.scope) {
        Ok(scope) => scope,
        Err(error) => return validation_error(error),
    };
    if !scope.is_editable() {
        return validation_error(format!(
            "scope `{}` is read-only; only user and project MCP servers can be deleted",
            scope.label()
        ));
    }

    let cwd = engine_cwd(&state);
    match remove_entry(&cwd, &name, &scope) {
        Ok(()) => match list_entries(&cwd) {
            Ok(servers) => Json(McpServersListResponse { servers }).into_response(),
            Err(error) => internal_error(error),
        },
        Err(RemoveError::NotFound(error)) => not_found(error),
        Err(RemoveError::Invalid(error)) => validation_error(error),
        Err(RemoveError::Internal(error)) => internal_error(error),
    }
}

/// GET /api/mcp-servers/marketplace
pub async fn mcp_servers_marketplace_handler() -> impl IntoResponse {
    Json(McpServersMarketplaceResponse {
        servers: Vec::new(),
        capabilities: McpServersMarketplaceCapabilities {
            refresh: false,
            install: false,
        },
    })
}

fn upsert_entry(
    state: &WebState,
    entry: McpServerConfigEntry,
) -> Result<McpServerConfigEntry, ProtocolApiError> {
    validate_entry(&entry).map_err(|error| validation_api(error))?;
    let cwd = engine_cwd(state);
    let path =
        settings_path_for_scope(&cwd, &entry.scope).map_err(|error| validation_api(error))?;
    let mut raw = read_raw_settings(&path).map_err(|error| internal_api(error))?;
    let servers = raw
        .extra
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(servers) = servers.as_object_mut() else {
        return Err(validation_api(format!(
            "{} has a non-object `mcpServers` field",
            path.display()
        )));
    };
    servers.insert(entry.name.clone(), entry_to_settings_value(&entry));
    write_settings_file(&path, &raw).map_err(|error| internal_api(error.to_string()))?;
    Ok(entry)
}

fn remove_entry(cwd: &Path, name: &str, scope: &ConfigScope) -> Result<(), RemoveError> {
    let path = settings_path_for_scope(cwd, scope).map_err(RemoveError::Invalid)?;
    if !path.exists() {
        return Err(RemoveError::NotFound(format!(
            "no settings file at {}",
            path.display()
        )));
    }
    let mut raw = read_raw_settings(&path).map_err(RemoveError::Internal)?;
    let Some(servers) = raw.extra.get_mut("mcpServers") else {
        return Err(RemoveError::NotFound(format!(
            "{} has no `mcpServers` section",
            path.display()
        )));
    };
    let Some(servers) = servers.as_object_mut() else {
        return Err(RemoveError::Invalid(format!(
            "{} has a non-object `mcpServers` field",
            path.display()
        )));
    };
    if servers.remove(name).is_none() {
        return Err(RemoveError::NotFound(format!(
            "{} has no MCP server named `{}`",
            path.display(),
            name
        )));
    }
    write_settings_file(&path, &raw).map_err(|error| RemoveError::Internal(error.to_string()))?;
    Ok(())
}

enum RemoveError {
    NotFound(String),
    Invalid(String),
    Internal(String),
}

fn list_entries(cwd: &Path) -> Result<Vec<McpServerConfigEntry>, String> {
    discover_mcp_servers_scoped(cwd)
        .map_err(|error| error.to_string())
        .map(|servers| {
            servers
                .into_iter()
                .map(|server| McpServerConfigEntry {
                    name: server.config.name,
                    scope: scope_from_discovery(&server.scope),
                    transport: server.config.transport,
                    command: server.config.command,
                    args: server.config.args,
                    url: server.config.url,
                    headers: server.config.headers,
                    oauth: server.config.oauth,
                    env: server.config.env,
                    browser_mcp: server.config.browser_mcp,
                    disabled: server.config.disabled,
                })
                .collect()
        })
}

fn validate_entry(entry: &McpServerConfigEntry) -> Result<(), String> {
    if entry.name.trim().is_empty() {
        return Err("MCP server name is required".to_string());
    }
    if !entry.scope.is_editable() {
        return Err(format!(
            "scope `{}` is read-only; only user and project MCP servers can be written",
            entry.scope.label()
        ));
    }
    match entry.transport.as_str() {
        "stdio" => {
            if entry.command.as_deref().unwrap_or("").trim().is_empty() {
                return Err("stdio MCP servers require `command`".to_string());
            }
        }
        "sse" | "streamable-http" => {
            if entry.url.as_deref().unwrap_or("").trim().is_empty() {
                return Err(format!("{} MCP servers require `url`", entry.transport));
            }
        }
        _ => {
            return Err(
                "transport must be one of `stdio`, `sse`, or `streamable-http`".to_string(),
            );
        }
    }
    Ok(())
}

fn settings_path_for_scope(cwd: &Path, scope: &ConfigScope) -> Result<PathBuf, String> {
    match scope {
        ConfigScope::User => Ok(user_settings_path()),
        ConfigScope::Project => Ok(cwd.join(".allthecodes").join("settings.json")),
        ConfigScope::Plugin { id } => Err(format!(
            "scope `plugin:{}` is read-only; edit the owning plugin instead",
            id
        )),
        ConfigScope::Ide { id } => Err(format!(
            "scope `ide:{}` is read-only; edit the IDE bridge config instead",
            id
        )),
    }
}

fn read_raw_settings(path: &Path) -> Result<RawSettings, String> {
    if !path.exists() {
        return Ok(RawSettings::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    if content.trim().is_empty() {
        return Ok(RawSettings::default());
    }
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))
}

fn entry_to_settings_value(entry: &McpServerConfigEntry) -> Value {
    let cfg = McpServerConfig {
        name: entry.name.clone(),
        transport: entry.transport.clone(),
        command: entry.command.clone(),
        args: entry.args.clone(),
        url: entry.url.clone(),
        headers: entry.headers.clone(),
        oauth: entry.oauth.clone(),
        env: entry.env.clone(),
        browser_mcp: entry.browser_mcp,
        disabled: entry.disabled,
    };
    let mut value = serde_json::to_value(cfg).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.remove("name");
    }
    value
}

fn parse_scope(raw: &str) -> Result<ConfigScope, String> {
    match raw {
        "user" => Ok(ConfigScope::User),
        "project" => Ok(ConfigScope::Project),
        "plugin" => Ok(ConfigScope::Plugin { id: String::new() }),
        "ide" => Ok(ConfigScope::Ide { id: String::new() }),
        value if value.starts_with("plugin:") => Ok(ConfigScope::Plugin {
            id: value.trim_start_matches("plugin:").to_string(),
        }),
        value if value.starts_with("ide:") => Ok(ConfigScope::Ide {
            id: value.trim_start_matches("ide:").to_string(),
        }),
        _ => Err("scope must be one of `user` or `project`".to_string()),
    }
}

fn scope_from_discovery(scope: &DiscoveryScope) -> ConfigScope {
    match scope {
        DiscoveryScope::User => ConfigScope::User,
        DiscoveryScope::Project => ConfigScope::Project,
        DiscoveryScope::Plugin(id) => ConfigScope::Plugin { id: id.clone() },
        DiscoveryScope::Ide(id) => ConfigScope::Ide { id: id.clone() },
    }
}

fn engine_cwd(state: &WebState) -> PathBuf {
    PathBuf::from(state.engine().cwd())
}

fn validation_api(error: String) -> ProtocolApiError {
    ProtocolApiError::BadRequest {
        code: "validation_error",
        message: error,
    }
}

fn internal_api(error: String) -> ProtocolApiError {
    ProtocolApiError::Internal { message: error }
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
        entity: "mcp_server",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
#[path = "mcp_servers_tests.rs"]
mod tests;
