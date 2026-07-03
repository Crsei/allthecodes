//! MCP server settings REST handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use allthecodes_config::settings::{user_settings_path, write_settings_file, RawSettings};
use allthecodes_ipc_protocol::subsystem_types::{
    ConfigScope, McpServerConfigEntry, McpServerInfoBrief, McpServerStatusInfo,
};
use allthecodes_mcp::discovery::{discover_mcp_servers_scoped, DiscoveryScope};
use allthecodes_mcp::probe::probe_mcp_server;
use allthecodes_mcp::McpServerConfig;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Clone)]
struct McpOAuthFlowSnapshot {
    state: String,
    error: Option<String>,
}

static MCP_OAUTH_FLOW_STATUS: OnceLock<Mutex<HashMap<String, McpOAuthFlowSnapshot>>> =
    OnceLock::new();

#[derive(Serialize)]
pub struct McpServersListResponse {
    pub servers: Vec<McpServerConfigEntry>,
}

#[derive(Serialize)]
pub struct McpServersHealthResponse {
    pub servers: Vec<McpServerStatusInfo>,
}

#[derive(Deserialize)]
pub struct McpServerProbeRequest {
    pub name: Option<String>,
    pub config: Option<McpServerConfig>,
    pub entry: Option<McpServerConfigEntry>,
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

/// GET /api/mcp-servers/health
pub async fn mcp_servers_health_handler(State(state): State<WebState>) -> Response {
    let cwd = engine_cwd(&state);
    let servers = build_mcp_server_health_response(&cwd).await;
    Json(McpServersHealthResponse { servers }).into_response()
}

/// POST /api/mcp-servers/probe
pub async fn mcp_servers_probe_handler(
    State(state): State<WebState>,
    Json(req): Json<McpServerProbeRequest>,
) -> Response {
    let cwd = engine_cwd(&state);
    let config = match probe_config_from_request(&cwd, req) {
        Ok(config) => config,
        Err(error) => return crate::api_errors::protocol_error_response(error).into_response(),
    };
    let probe = probe_mcp_server(config).await;
    Json(probe).into_response()
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

/// POST /api/mcp-servers/{name}/oauth/start
///
/// Starts an OAuth authorization flow for the named MCP server using auto
/// loopback callback discovery. Returns the authorization URL the user must
/// open in a browser.
pub async fn mcp_servers_auth_start_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    let config = match find_mcp_config(&cwd, &name) {
        Ok(cfg) => cfg,
        Err(error) => return not_found(error),
    };
    match allthecodes_mcp::oauth_login::start_auto_authorization(&config).await {
        Ok((handle, start)) => {
            record_oauth_flow_status(config.name.clone(), "pending", None);
            spawn_web_oauth_wait_task(config.name.clone(), handle);
            Json(serde_json::json!({
                "authorization_url": start.authorization_url,
                "state": start.state,
                "redirect_uri": start.redirect_uri,
                "token_store_path": start.token_store_path.to_string_lossy(),
            }))
            .into_response()
        }
        Err(error) => {
            let message = format!("Failed to start OAuth authorization: {error}");
            record_oauth_flow_status(config.name.clone(), "failed", Some(message.clone()));
            oauth_problem_response(message)
        }
    }
}

/// POST /api/mcp-servers/{name}/oauth/complete
///
/// Completes an OAuth authorization manually with the code (and optional
/// state) returned by the authorization server. Used when the auto loopback
/// callback could not receive the code.
///
/// Request body (JSON):
/// - `code` (required): The authorization code.
/// - `state` (optional): The state to validate.
#[derive(Deserialize)]
pub struct McpAuthCompleteRequest {
    pub code: String,
    pub state: Option<String>,
}

pub async fn mcp_servers_auth_complete_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
    Json(req): Json<McpAuthCompleteRequest>,
) -> Response {
    let cwd = engine_cwd(&state);
    let config = match find_mcp_config(&cwd, &name) {
        Ok(cfg) => cfg,
        Err(error) => return not_found(error),
    };
    match allthecodes_mcp::auth::complete_authorization(&config, &req.code, req.state.as_deref())
        .await
    {
        Ok(token) => {
            record_oauth_flow_status(config.name.clone(), "completed", None);
            Json(serde_json::json!({
                "authorized": true,
                "token_type": token.token_type,
                "expires_at": token.expires_at,
                "scopes": token.scopes,
            }))
            .into_response()
        }
        Err(error) => {
            let message = format!("Failed to complete OAuth authorization: {error}");
            record_oauth_flow_status(config.name.clone(), "failed", Some(message.clone()));
            oauth_problem_response(message)
        }
    }
}

/// GET /api/mcp-servers/{name}/oauth/status
///
/// Returns redacted OAuth credential status for the named MCP server.
pub async fn mcp_servers_auth_status_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    let config = match find_mcp_config(&cwd, &name) {
        Ok(cfg) => cfg,
        Err(error) => return not_found(error),
    };
    match allthecodes_mcp::auth::credential_status(&config) {
        Ok(status) => {
            let flow = oauth_flow_json(oauth_flow_status(&config.name));
            Json(serde_json::json!({
                "status": status.status.as_str(),
                "configured": status.configured,
                "authorized": status.authorized,
                "expired": status.expired,
                "can_refresh": status.can_refresh,
                "message": status.message,
                "oauth_flow": flow,
                "token_store_path": status.token_store_path.to_string_lossy(),
            }))
            .into_response()
        }
        Err(error) => {
            let body = ProtocolApiError::Internal {
                message: format!("Failed to query OAuth status: {error}"),
            }
            .into_body();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

/// DELETE /api/mcp-servers/{name}/oauth
///
/// Clears stored OAuth credentials for the named MCP server.
pub async fn mcp_servers_auth_clear_handler(
    State(state): State<WebState>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let cwd = engine_cwd(&state);
    let config = match find_mcp_config(&cwd, &name) {
        Ok(cfg) => cfg,
        Err(error) => return not_found(error),
    };
    match allthecodes_mcp::auth::clear_stored_token(&config) {
        Ok(true) => {
            clear_oauth_flow_status(&config.name);
            Json(serde_json::json!({"cleared": true})).into_response()
        }
        Ok(false) => {
            clear_oauth_flow_status(&config.name);
            Json(serde_json::json!({"cleared": false, "message": "No stored OAuth credentials found"})).into_response()
        }
        Err(error) => {
            let body = ProtocolApiError::Internal {
                message: format!("Failed to clear OAuth credentials: {error}"),
            }
            .into_body();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

fn spawn_web_oauth_wait_task(
    server_name: String,
    handle: allthecodes_mcp::oauth_login::McpOAuthLoginHandle,
) {
    tokio::spawn(async move {
        match handle.wait().await {
            Ok(_) => record_oauth_flow_status(server_name, "completed", None),
            Err(error) => {
                record_oauth_flow_status(server_name, "failed", Some(error.to_string()));
            }
        }
    });
}

fn oauth_flow_store() -> &'static Mutex<HashMap<String, McpOAuthFlowSnapshot>> {
    MCP_OAUTH_FLOW_STATUS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn record_oauth_flow_status(server_name: String, state: &str, error: Option<String>) {
    if let Ok(mut status) = oauth_flow_store().lock() {
        status.insert(
            server_name,
            McpOAuthFlowSnapshot {
                state: state.to_string(),
                error,
            },
        );
    }
}

fn clear_oauth_flow_status(server_name: &str) {
    if let Ok(mut status) = oauth_flow_store().lock() {
        status.remove(server_name);
    }
}

fn oauth_flow_status(server_name: &str) -> Option<McpOAuthFlowSnapshot> {
    oauth_flow_store()
        .lock()
        .ok()
        .and_then(|status| status.get(server_name).cloned())
}

fn oauth_flow_json(flow: Option<McpOAuthFlowSnapshot>) -> serde_json::Value {
    flow.map(|flow| {
        let error_code = flow.error.as_deref().map(oauth_error_code);
        serde_json::json!({
            "state": flow.state,
            "error": flow.error,
            "error_code": error_code,
        })
    })
    .unwrap_or(serde_json::Value::Null)
}

fn oauth_problem_response(message: String) -> Response {
    let code = oauth_error_code(&message);
    let status = oauth_error_status(code);
    let body = ProtocolApiError::BadRequest { code, message }.into_body();
    (status, Json(body)).into_response()
}

fn oauth_error_code(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "oauth_timeout"
    } else if lower.contains("has no oauth configuration")
        || lower.contains("no oauth configuration")
        || lower.contains("unsupported")
        || lower.contains("no authorization support")
    {
        "auth_unsupported"
    } else {
        "oauth_error"
    }
}

fn oauth_error_status(code: &str) -> StatusCode {
    match code {
        "oauth_timeout" => StatusCode::GATEWAY_TIMEOUT,
        "auth_unsupported" => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_REQUEST,
    }
}

fn find_mcp_config(cwd: &Path, name: &str) -> Result<McpServerConfig, String> {
    discover_mcp_servers_scoped(cwd)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|server| server.config.name == name)
        .map(|server| server.config)
        .ok_or_else(|| format!("MCP server '{}' not found", name))
}

fn upsert_entry(
    state: &WebState,
    entry: McpServerConfigEntry,
) -> Result<McpServerConfigEntry, ProtocolApiError> {
    validate_entry(&entry).map_err(validation_api)?;
    let cwd = engine_cwd(state);
    let path = settings_path_for_scope(&cwd, &entry.scope).map_err(validation_api)?;
    let mut raw = read_raw_settings(&path).map_err(internal_api)?;
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

async fn build_mcp_server_health_response(cwd: &Path) -> Vec<McpServerStatusInfo> {
    let scoped = match discover_mcp_servers_scoped(cwd) {
        Ok(scoped) => scoped,
        Err(error) => return vec![mcp_discovery_error_status(error.to_string())],
    };
    let mut configs: Vec<McpServerConfig> = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in scoped {
        if let Some(error) = entry.error {
            diagnostics.push(McpServerStatusInfo {
                name: entry.config.name,
                state: "error".to_string(),
                transport: entry.config.transport,
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: Some(format!(
                    "{} scope: {}",
                    scope_from_discovery(&entry.scope).label(),
                    error
                )),
                ..Default::default()
            });
            continue;
        }
        if let Some(existing) = configs
            .iter_mut()
            .find(|config| config.name == entry.config.name)
        {
            *existing = entry.config;
        } else {
            configs.push(entry.config);
        }
    }

    let mut rows = if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        let manager = manager.lock().await;
        build_mcp_server_health_from_configs(configs, Some(&manager))
    } else {
        build_mcp_server_health_from_configs(configs, None)
    };
    rows.append(&mut diagnostics);
    rows
}

fn build_mcp_server_health_from_configs(
    configs: Vec<McpServerConfig>,
    manager: Option<&allthecodes_mcp::manager::McpManager>,
) -> Vec<McpServerStatusInfo> {
    let health_by_name = manager.map(|manager| {
        manager
            .health_snapshots()
            .into_iter()
            .map(|snapshot| (snapshot.server_name.clone(), snapshot))
            .collect::<HashMap<_, _>>()
    });
    configs
        .into_iter()
        .map(|cfg| {
            let health = health_by_name
                .as_ref()
                .and_then(|health| health.get(&cfg.name));
            build_mcp_server_health_row(cfg, manager, health)
        })
        .collect()
}

fn build_mcp_server_health_row(
    cfg: McpServerConfig,
    manager: Option<&allthecodes_mcp::manager::McpManager>,
    health: Option<&allthecodes_mcp::McpServerHealthSnapshot>,
) -> McpServerStatusInfo {
    if cfg.disabled.unwrap_or(false) {
        return McpServerStatusInfo {
            name: cfg.name,
            state: "disabled".to_string(),
            transport: cfg.transport,
            tools_count: 0,
            resources_count: 0,
            server_info: None,
            instructions: None,
            error: None,
            ..mcp_health_defaults(health)
        };
    }

    if let Some(client) = manager.and_then(|manager| manager.clients.get(&cfg.name)) {
        let (state, error) = match &client.state {
            allthecodes_mcp::McpConnectionState::Pending => ("pending".to_string(), None),
            allthecodes_mcp::McpConnectionState::Connected => ("connected".to_string(), None),
            allthecodes_mcp::McpConnectionState::Disconnected => ("disconnected".to_string(), None),
            allthecodes_mcp::McpConnectionState::Error(error) => {
                ("error".to_string(), Some(error.clone()))
            }
        };
        let server_info = (!client.server_info.name.is_empty()).then(|| McpServerInfoBrief {
            name: client.server_info.name.clone(),
            version: client.server_info.version.clone(),
        });
        return McpServerStatusInfo {
            name: cfg.name,
            state,
            transport: cfg.transport,
            tools_count: client.tools.len(),
            resources_count: client.resources.len(),
            server_info,
            instructions: client.instructions.clone(),
            error: error.or_else(|| health.and_then(|health| health.last_error.clone())),
            ..mcp_health_defaults(health)
        };
    }

    let remembered = allthecodes_mcp::runtime::server_state(&cfg.name);
    let remembered_state = remembered.as_ref().map(|state| state.state.clone());
    let remembered_error = remembered.and_then(|state| state.error);
    McpServerStatusInfo {
        name: cfg.name,
        state: health
            .map(|health| health.state.clone())
            .or(remembered_state)
            .unwrap_or_else(|| "pending".to_string()),
        transport: cfg.transport,
        tools_count: health.and_then(|health| health.tools_count).unwrap_or(0),
        resources_count: health
            .and_then(|health| health.resources_count)
            .unwrap_or(0),
        server_info: None,
        instructions: None,
        error: health
            .and_then(|health| health.last_error.clone())
            .or(remembered_error),
        ..mcp_health_defaults(health)
    }
}

fn mcp_discovery_error_status(error: String) -> McpServerStatusInfo {
    McpServerStatusInfo {
        name: "discovery".to_string(),
        state: "error".to_string(),
        transport: "settings".to_string(),
        tools_count: 0,
        resources_count: 0,
        server_info: None,
        instructions: None,
        error: Some(format!("Failed to discover MCP servers: {error}")),
        ..Default::default()
    }
}

fn mcp_health_defaults(
    health: Option<&allthecodes_mcp::McpServerHealthSnapshot>,
) -> McpServerStatusInfo {
    let Some(health) = health else {
        return McpServerStatusInfo::default();
    };

    McpServerStatusInfo {
        last_success_at: health.last_success_at,
        last_attempt_at: health.last_attempt_at,
        last_error_kind: health.last_error_kind.clone(),
        failure_count: (health.failure_count > 0).then_some(health.failure_count),
        connect_attempt_count: Some(health.connect_attempt_count),
        retry_scheduled_count: Some(health.retry_scheduled_count),
        retry_exhausted_count: Some(health.retry_exhausted_count),
        recovered_count: Some(health.recovered_count),
        next_retry_at: health.next_retry_at,
        stderr_tail: health.stderr_tail.clone(),
        stderr_tail_dropped_line_count: Some(health.stderr_tail_dropped_line_count),
        ..Default::default()
    }
}

fn probe_config_from_request(
    cwd: &Path,
    req: McpServerProbeRequest,
) -> Result<McpServerConfig, ProtocolApiError> {
    let selector_count = usize::from(req.name.is_some())
        + usize::from(req.config.is_some())
        + usize::from(req.entry.is_some());
    if selector_count != 1 {
        return Err(validation_api(
            "probe request must include exactly one of `name`, `config`, or `entry`".to_string(),
        ));
    }

    if let Some(mut config) = req.config {
        if config.name.trim().is_empty() {
            config.name = "probe".to_string();
        }
        return Ok(config);
    }
    if let Some(entry) = req.entry {
        return Ok(entry_to_config(&entry));
    }
    let name = req.name.unwrap_or_default();
    if name.trim().is_empty() {
        return Err(validation_api("MCP server name is required".to_string()));
    }
    find_mcp_config(cwd, &name).map_err(|error| ProtocolApiError::NotFound {
        entity: "mcp_server",
        id: error,
    })
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
                    headers: redact_server_headers(server.config.headers),
                    oauth: server.config.oauth,
                    env: redact_server_env(server.config.env),
                    browser_mcp: server.config.browser_mcp,
                    disabled: server.config.disabled,
                    bearer_token_env_var: server.config.bearer_token_env_var,
                    env_http_headers: server.config.env_http_headers,
                    auth: server.config.auth,
                })
                .collect()
        })
}

fn redact_server_headers(
    headers: Option<std::collections::HashMap<String, String>>,
) -> Option<std::collections::HashMap<String, String>> {
    headers.map(|mut headers| {
        for (name, value) in headers.iter_mut() {
            if is_sensitive_header_name(name) {
                *value = "[redacted]".to_string();
            }
        }
        headers
    })
}

fn redact_server_env(
    env: Option<std::collections::HashMap<String, String>>,
) -> Option<std::collections::HashMap<String, String>> {
    env.map(|mut env| {
        if let Some(value) = env.get_mut("ALLTHECODES_COM_ACCESS_TOKEN") {
            *value = "[redacted]".to_string();
        }
        env
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
    if let Some(headers) = &entry.headers {
        for name in headers.keys() {
            if is_sensitive_header_name(name) {
                return Err(format!(
                    "refusing static sensitive HTTP header `{}`; use bearerTokenEnvVar or envHttpHeaders instead",
                    name
                ));
            }
        }
    }
    if matches!(entry.auth.as_deref(), Some("chatgpt")) {
        return Err("auth=chatgpt is reserved but not implemented yet".to_string());
    }
    match entry.transport.as_str() {
        "stdio" => {
            if entry.command.as_deref().unwrap_or("").trim().is_empty() {
                return Err("stdio MCP servers require `command`".to_string());
            }
            if entry
                .headers
                .as_ref()
                .map(|h| !h.is_empty())
                .unwrap_or(false)
                || entry.bearer_token_env_var.is_some()
                || entry
                    .env_http_headers
                    .as_ref()
                    .map(|h| !h.is_empty())
                    .unwrap_or(false)
                || entry.auth.is_some()
                || entry.oauth.is_some()
            {
                return Err(
                    "stdio MCP servers do not support HTTP headers, bearer env vars, env HTTP headers, auth, or OAuth config"
                        .to_string(),
                );
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

fn is_sensitive_header_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "authorization"
        || lower == "proxy-authorization"
        || lower == "x-api-key"
        || lower == "api-key"
        || lower == "x-auth-token"
        || lower == "x-access-token"
        || lower.contains("token")
        || lower.contains("secret")
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

fn entry_to_config(entry: &McpServerConfigEntry) -> McpServerConfig {
    McpServerConfig {
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
        bearer_token_env_var: entry.bearer_token_env_var.clone(),
        env_http_headers: entry.env_http_headers.clone(),
        auth: entry.auth.clone(),
    }
}

fn entry_to_settings_value(entry: &McpServerConfigEntry) -> Value {
    let cfg = entry_to_config(entry);
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
