//! Plugin REST handlers.

use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_mcp::client::McpClient;
use allthecodes_mcp::runtime::current_manager;
use allthecodes_mcp::McpServerConfig;
use allthecodes_plugins::installation::{
    install_official_plugin, install_plugin, uninstall_plugin_ext, InstallError, InstallScope,
    OfficialPluginInstallRequest,
};
use allthecodes_plugins::loader::{load_installed_plugins_report, PluginDiagnostic};
use allthecodes_plugins::manifest::PluginManifest;
use allthecodes_plugins::marketplace::{
    refresh_official_marketplace_if_stale, MarketplacePluginEntry, MarketplaceSource,
    GLOBAL_MARKETPLACE_INDEX, OFFICIAL_MARKETPLACE_SOURCE_NAME,
};
use allthecodes_plugins::{PluginEntry, PluginStatus};
use tracing::warn;

use allthecodes_protocol::v1::plugins::PluginsListResponse as ProtocolPluginsListResponse;
use allthecodes_protocol::v1::plugins::PluginsMarketplaceResponse as ProtocolPluginsMarketplaceResponse;
use allthecodes_protocol::v1::plugins::{
    PluginIdRequest, PluginInstallRequest as ProtocolPluginInstallRequest,
    PluginInstallResponse as ProtocolPluginInstallResponse,
    PluginLifecycleResponse as ProtocolPluginLifecycleResponse,
    PluginOfficialInstallRequest as ProtocolPluginOfficialInstallRequest,
    PluginUninstallByIdRequest, PluginUninstallResponse as ProtocolPluginUninstallResponse,
    PluginUpdateRequest,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::NoParams;
use async_trait::async_trait;
use axum::routing::{get, post};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PluginsListProcessor {
    state: WebState,
}

impl From<WebState> for PluginsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsListProcessor {
    type Request = NoParams;
    type Response = ProtocolPluginsListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let handler_resp = plugins_list_response();
        serde_json::from_value(serde_json::to_value(&handler_resp).map_err(|e| {
            ProtocolApiError::Internal {
                message: e.to_string(),
            }
        })?)
        .map_err(|e| ProtocolApiError::Internal {
            message: e.to_string(),
        })
    }
}

#[derive(Clone)]
pub struct PluginsMarketplaceProcessor {
    state: WebState,
}

impl From<WebState> for PluginsMarketplaceProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsMarketplaceProcessor {
    type Request = NoParams;
    type Response = ProtocolPluginsMarketplaceResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.marketplace"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let _ = refresh_official_marketplace_if_stale().await;
        let handler_resp = PluginsMarketplaceResponse {
            plugins: GLOBAL_MARKETPLACE_INDEX.list_all_entries(),
            sources: GLOBAL_MARKETPLACE_INDEX.list_sources(),
        };
        serde_json::from_value(serde_json::to_value(&handler_resp).map_err(|e| {
            ProtocolApiError::Internal {
                message: e.to_string(),
            }
        })?)
        .map_err(|e| ProtocolApiError::Internal {
            message: e.to_string(),
        })
    }
}

#[derive(Clone)]
pub struct PluginsInstallProcessor {
    state: WebState,
}

impl From<WebState> for PluginsInstallProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsInstallProcessor {
    type Request = ProtocolPluginInstallRequest;
    type Response = ProtocolPluginInstallResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.install"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        install_request(request, &self.state).await
    }
}

#[derive(Clone)]
pub struct PluginsUpdateProcessor {
    state: WebState,
}

impl From<WebState> for PluginsUpdateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsUpdateProcessor {
    type Request = PluginUpdateRequest;
    type Response = ProtocolPluginInstallResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.update"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        let request = match request {
            PluginUpdateRequest::Official(req) => ProtocolPluginInstallRequest::Official(req),
            PluginUpdateRequest::Id(req) => {
                return Err(ProtocolApiError::BadRequest {
                    code: "not_implemented",
                    message: format!(
                        "Marketplace fields are required to update plugin '{}'",
                        req.id
                    ),
                });
            }
        };
        install_request(request, &self.state).await
    }
}

#[derive(Clone)]
pub struct PluginsUninstallByIdProcessor {
    state: WebState,
}

impl From<WebState> for PluginsUninstallByIdProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsUninstallByIdProcessor {
    type Request = PluginUninstallByIdRequest;
    type Response = ProtocolPluginUninstallResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.uninstall"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        uninstall_plugin_by_id(request, &self.state).await
    }
}

#[derive(Clone)]
pub struct PluginsEnableProcessor {
    state: WebState,
}

impl From<WebState> for PluginsEnableProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsEnableProcessor {
    type Request = PluginIdRequest;
    type Response = ProtocolPluginLifecycleResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.enable"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        set_plugin_enabled(request, true, &self.state).await
    }
}

#[derive(Clone)]
pub struct PluginsDisableProcessor {
    state: WebState,
}

impl From<WebState> for PluginsDisableProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsDisableProcessor {
    type Request = PluginIdRequest;
    type Response = ProtocolPluginLifecycleResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.disable"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        set_plugin_enabled(request, false, &self.state).await
    }
}

#[derive(Clone)]
pub struct PluginsRestartProcessor {
    state: WebState,
}

impl From<WebState> for PluginsRestartProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PluginsRestartProcessor {
    type Request = PluginIdRequest;
    type Response = ProtocolPluginLifecycleResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.restart"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        restart_plugin(request, &self.state).await
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::PluginsList, get(plugins_list_handler))
        .handle(ApiMethod::PluginsInstalled, get(plugins_installed_handler))
        .handle(
            ApiMethod::PluginsMarketplace,
            get(plugins_marketplace_handler),
        )
        .handle(ApiMethod::PluginsInstall, post(plugins_install_handler))
        .handle(ApiMethod::PluginsUpdate, post(plugins_update_handler))
        .handle(
            ApiMethod::PluginsUninstallById,
            post(plugins_uninstall_by_id_handler),
        )
        .handle(ApiMethod::PluginsEnable, post(plugins_enable_handler))
        .handle(ApiMethod::PluginsDisable, post(plugins_disable_handler))
        .handle(ApiMethod::PluginsRestart, post(plugins_restart_handler))
        .handle(
            ApiMethod::PluginsTestConnection,
            post(plugins_test_connection_handler),
        )
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PluginsListResponse {
    pub plugins: Vec<PluginEntry>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Serialize)]
pub struct PluginsMarketplaceResponse {
    pub plugins: Vec<MarketplacePluginEntry>,
    pub sources: Vec<MarketplaceSource>,
}

#[derive(Serialize)]
pub struct PluginInstallResponse {
    pub plugin: PluginEntry,
    pub install_path: PathBuf,
    pub fresh_install: bool,
    pub status: String,
}

#[derive(Deserialize)]
pub struct PluginUninstallRequest {
    #[serde(default)]
    pub purge: bool,
}

#[derive(Serialize)]
pub struct PluginUninstallResponse {
    pub plugin: PluginEntry,
    pub purged: bool,
}

#[derive(Serialize)]
pub struct PluginLifecycleResponse {
    pub plugin: PluginEntry,
    pub status: String,
}

#[derive(Serialize)]
pub struct PluginTestConnectionResponse {
    pub plugin: PluginEntry,
    pub status: String,
    pub servers: Vec<PluginMcpConnectionTestResult>,
}

#[derive(Serialize)]
pub struct PluginMcpConnectionTestResult {
    pub server: String,
    pub status: String,
    pub message: String,
    pub command: Option<String>,
    pub tools: Option<usize>,
    pub resources: Option<usize>,
    pub checks: Vec<PluginMcpConnectionCheck>,
}

#[derive(Serialize)]
pub struct PluginMcpConnectionCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

/// GET /api/plugins
pub async fn plugins_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PluginsListProcessor>(state, ApiMethod::PluginsList, NoParams {})
        .await
}

/// GET /api/plugins/installed
pub async fn plugins_installed_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PluginsListProcessor>(state, ApiMethod::PluginsInstalled, NoParams {})
        .await
}

/// GET /api/plugins/marketplace
pub async fn plugins_marketplace_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PluginsMarketplaceProcessor>(
        state,
        ApiMethod::PluginsMarketplace,
        NoParams {},
    )
    .await
}

/// POST /api/plugins/install
pub async fn plugins_install_handler(
    State(state): State<WebState>,
    Json(req): Json<ProtocolPluginInstallRequest>,
) -> Response {
    rest_processor_response::<PluginsInstallProcessor>(state, ApiMethod::PluginsInstall, req).await
}

/// POST /api/plugins/update
pub async fn plugins_update_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginUpdateRequest>,
) -> Response {
    rest_processor_response::<PluginsUpdateProcessor>(state, ApiMethod::PluginsUpdate, req).await
}

/// POST /api/plugins/{id}/uninstall
pub async fn plugins_uninstall_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PluginUninstallRequest>,
) -> Response {
    let runtime_configs = plugin_runtime_configs(&id);
    match uninstall_plugin_ext(&id, req.purge) {
        Ok(Some(plugin)) => {
            let _ = disconnect_plugin_runtime_configs(runtime_configs).await;
            refresh_plugin_contributed_skills(&state);
            Json(PluginUninstallResponse {
                plugin,
                purged: req.purge,
            })
            .into_response()
        }
        Ok(None) => not_found(format!("Plugin '{}' not found", id)),
        Err(error) => internal_error(error.to_string()),
    }
}

/// POST /api/plugins/uninstall
pub async fn plugins_uninstall_by_id_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginUninstallByIdRequest>,
) -> Response {
    match uninstall_plugin_by_id(req, &state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

/// POST /api/plugins/enable
pub async fn plugins_enable_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginIdRequest>,
) -> Response {
    match set_plugin_enabled(req, true, &state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

/// POST /api/plugins/disable
pub async fn plugins_disable_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginIdRequest>,
) -> Response {
    match set_plugin_enabled(req, false, &state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

/// POST /api/plugins/restart
pub async fn plugins_restart_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginIdRequest>,
) -> Response {
    match restart_plugin(req, &state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

/// POST /api/plugins/{id}/test-connection
pub async fn plugins_test_connection_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match test_plugin_connection(&id, &state).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn install_request(
    request: ProtocolPluginInstallRequest,
    state: &WebState,
) -> Result<ProtocolPluginInstallResponse, ProtocolApiError> {
    let available_plugins = HashMap::<String, String>::new();
    let manifests = HashMap::<String, PluginManifest>::new();
    let result = match request {
        ProtocolPluginInstallRequest::Legacy(request) => {
            let scope = parse_install_scope(request.scope.as_deref()).map_err(|error| {
                ProtocolApiError::BadRequest {
                    code: "validation_error",
                    message: error,
                }
            })?;
            let source =
                resolve_source_against_cwd(&request.source, Path::new(&state.engine().cwd()));
            install_plugin(
                &source,
                Some(scope),
                Some(state.app_version()),
                None,
                &available_plugins,
                &manifests,
            )
            .await
        }
        ProtocolPluginInstallRequest::Official(request) => {
            install_official_plugin(
                official_install_request(request),
                Some(state.app_version()),
                &available_plugins,
                &manifests,
            )
            .await
        }
    };

    match result {
        Ok(result) => {
            refresh_plugin_contributed_skills(state);
            let runtime_report =
                refresh_plugin_runtime(state, &result.plugin.id, PluginRuntimeRefreshMode::Connect)
                    .await;
            let status = lifecycle_status_with_runtime("installed", &runtime_report);
            let handler_resp = PluginInstallResponse {
                plugin: result.plugin,
                install_path: result.install_path,
                fresh_install: result.fresh_install,
                status,
            };
            serde_json::from_value(serde_json::to_value(&handler_resp).map_err(|e| {
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
            })?)
            .map_err(|e| ProtocolApiError::Internal {
                message: e.to_string(),
            })
        }
        Err(error) => Err(install_error_to_api(error)),
    }
}

fn plugins_list_response() -> PluginsListResponse {
    let report = load_installed_plugins_report();
    PluginsListResponse {
        plugins: report.plugins,
        diagnostics: report.diagnostics,
    }
}

fn parse_install_scope(scope: Option<&str>) -> Result<InstallScope, String> {
    match scope.unwrap_or("user") {
        "user" => Ok(InstallScope::User),
        "project" => Ok(InstallScope::Project),
        value => Err(format!(
            "scope must be one of `user` or `project`, got `{}`",
            value
        )),
    }
}

fn resolve_source_against_cwd(source: &str, cwd: &Path) -> String {
    let path = Path::new(source);
    if path.is_absolute() || path.exists() {
        return source.to_string();
    }
    let candidate = cwd.join(path);
    if candidate.exists() {
        return candidate.to_string_lossy().to_string();
    }
    source.to_string()
}

fn official_install_request(
    request: ProtocolPluginOfficialInstallRequest,
) -> OfficialPluginInstallRequest {
    OfficialPluginInstallRequest {
        id: request.id,
        version: request.version,
        download_url: request.download_url,
        sha256: request.sha256,
        homepage: request.homepage,
    }
}

fn install_error_to_api(error: InstallError) -> ProtocolApiError {
    let err_str = match &error {
        InstallError::AlreadyInstalled(plugin) => {
            format!("Plugin '{}' is already installed", plugin)
        }
        InstallError::SourceNotFound(msg)
        | InstallError::ValidationFailed(msg)
        | InstallError::MissingDependency(msg)
        | InstallError::PolicyBlocked(msg)
        | InstallError::DownloadFailed(msg) => msg.clone(),
        InstallError::EngineIncompatible { required, current } => {
            format!(
                "Plugin engine version incompatible: required {}, running {}",
                required, current
            )
        }
        InstallError::MaxPluginsReached { max } => {
            format!("Max plugins limit reached ({})", max)
        }
        InstallError::Other(msg) => msg.clone(),
    };
    match error {
        InstallError::AlreadyInstalled(_) => ProtocolApiError::Conflict { reason: err_str },
        _ => ProtocolApiError::BadRequest {
            code: "validation_error",
            message: err_str,
        },
    }
}

fn api_error_response(error: ProtocolApiError) -> Response {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body())).into_response()
}

async fn uninstall_plugin_by_id(
    req: PluginUninstallByIdRequest,
    state: &WebState,
) -> Result<ProtocolPluginUninstallResponse, ProtocolApiError> {
    if req.id.trim().is_empty() {
        return Err(ProtocolApiError::BadRequest {
            code: "validation_error",
            message: "id is required".to_string(),
        });
    }
    let runtime_configs = plugin_runtime_configs(&req.id);
    match uninstall_plugin_ext(&req.id, req.purge) {
        Ok(Some(plugin)) => {
            let _ = disconnect_plugin_runtime_configs(runtime_configs).await;
            refresh_plugin_contributed_skills(state);
            Ok(ProtocolPluginUninstallResponse {
                plugin: plugin_value(plugin)?,
                purged: req.purge,
            })
        }
        Ok(None) => Err(plugin_not_found(req.id)),
        Err(error) => Err(ProtocolApiError::Internal {
            message: error.to_string(),
        }),
    }
}

fn find_installed_plugin(id: &str) -> Option<PluginEntry> {
    allthecodes_plugins::loader::load_installed_plugins()
        .into_iter()
        .find(|plugin| plugin.id == id)
}

async fn set_plugin_enabled(
    req: PluginIdRequest,
    enabled: bool,
    state: &WebState,
) -> Result<ProtocolPluginLifecycleResponse, ProtocolApiError> {
    if req.id.trim().is_empty() {
        return Err(ProtocolApiError::BadRequest {
            code: "validation_error",
            message: "id is required".to_string(),
        });
    }
    let disconnect_configs = if enabled {
        Vec::new()
    } else {
        plugin_runtime_configs(&req.id)
    };
    let status = if enabled {
        PluginStatus::Installed
    } else {
        PluginStatus::Disabled
    };
    match set_installed_plugin_status(&req.id, status) {
        Ok(Some(plugin)) => {
            refresh_plugin_contributed_skills(state);
            let runtime_report = if enabled {
                refresh_plugin_runtime(state, &req.id, PluginRuntimeRefreshMode::Connect).await
            } else {
                disconnect_plugin_runtime_configs(disconnect_configs).await
            };
            let status = lifecycle_status_with_runtime(
                if enabled { "enabled" } else { "disabled" },
                &runtime_report,
            );
            Ok(ProtocolPluginLifecycleResponse {
                plugin: plugin_value(plugin)?,
                status,
            })
        }
        Ok(None) => Err(plugin_not_found(req.id)),
        Err(error) => Err(ProtocolApiError::Internal {
            message: error.to_string(),
        }),
    }
}

async fn restart_plugin(
    req: PluginIdRequest,
    state: &WebState,
) -> Result<ProtocolPluginLifecycleResponse, ProtocolApiError> {
    if req.id.trim().is_empty() {
        return Err(ProtocolApiError::BadRequest {
            code: "validation_error",
            message: "id is required".to_string(),
        });
    }
    match find_installed_plugin(&req.id) {
        Some(plugin) => {
            refresh_plugin_contributed_skills(state);
            let runtime_report =
                refresh_plugin_runtime(state, &req.id, PluginRuntimeRefreshMode::Reconnect).await;
            if !runtime_report.failures.is_empty() {
                return Err(ProtocolApiError::Internal {
                    message: runtime_report.failure_summary(),
                });
            }
            Ok(ProtocolPluginLifecycleResponse {
                plugin: plugin_value(plugin)?,
                status: lifecycle_status_with_runtime("restarted", &runtime_report),
            })
        }
        None => Err(plugin_not_found(req.id)),
    }
}

#[derive(Debug, Clone, Copy)]
enum PluginRuntimeRefreshMode {
    Connect,
    Reconnect,
    Disconnect,
}

#[derive(Default)]
struct PluginRuntimeRefreshReport {
    attempted: usize,
    skipped: Option<String>,
    failures: Vec<PluginRuntimeFailure>,
}

struct PluginRuntimeFailure {
    server: String,
    error: String,
}

impl PluginRuntimeRefreshReport {
    fn failure_summary(&self) -> String {
        if self.failures.is_empty() {
            return self
                .skipped
                .clone()
                .unwrap_or_else(|| "Plugin MCP runtime refresh completed".to_string());
        }
        self.failures
            .iter()
            .map(|failure| format!("{}: {}", failure.server, failure.error))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

async fn refresh_plugin_runtime(
    _state: &WebState,
    plugin_id: &str,
    mode: PluginRuntimeRefreshMode,
) -> PluginRuntimeRefreshReport {
    let configs = plugin_runtime_configs(plugin_id);
    refresh_plugin_runtime_configs(configs, mode).await
}

fn plugin_runtime_configs(plugin_id: &str) -> Vec<McpServerConfig> {
    allthecodes_plugins::discover_plugin_mcp_servers_scoped()
        .into_iter()
        .filter_map(|(id, config)| (id == plugin_id).then_some(config))
        .collect()
}

async fn disconnect_plugin_runtime_configs(
    configs: Vec<McpServerConfig>,
) -> PluginRuntimeRefreshReport {
    refresh_plugin_runtime_configs(configs, PluginRuntimeRefreshMode::Disconnect).await
}

async fn refresh_plugin_runtime_configs(
    configs: Vec<McpServerConfig>,
    mode: PluginRuntimeRefreshMode,
) -> PluginRuntimeRefreshReport {
    let mut report = PluginRuntimeRefreshReport::default();
    if configs.is_empty() {
        return report;
    }

    let Some(manager) = current_manager() else {
        report.skipped = Some("MCP runtime manager is not available".to_string());
        return report;
    };

    let mut manager = manager.lock().await;
    for config in configs {
        report.attempted += 1;
        let server = config.name.clone();
        let result = match mode {
            PluginRuntimeRefreshMode::Connect => manager.connect_server(config).await,
            PluginRuntimeRefreshMode::Reconnect => manager.reconnect_server(config).await,
            PluginRuntimeRefreshMode::Disconnect => {
                manager.disconnect_server(&server).await;
                Ok(())
            }
        };

        if let Err(error) = result {
            report.failures.push(PluginRuntimeFailure {
                server,
                error: error.to_string(),
            });
        }
    }

    report
}

fn lifecycle_status_with_runtime(base: &str, report: &PluginRuntimeRefreshReport) -> String {
    if !report.failures.is_empty() {
        return format!("{}_with_mcp_errors: {}", base, report.failure_summary());
    }
    if let Some(skipped) = &report.skipped {
        return format!("{}_mcp_skipped: {}", base, skipped);
    }
    if report.attempted > 0 {
        return format!("{}_mcp_connected", base);
    }
    base.to_string()
}

async fn test_plugin_connection(
    id: &str,
    _state: &WebState,
) -> Result<PluginTestConnectionResponse, ProtocolApiError> {
    if id.trim().is_empty() {
        return Err(ProtocolApiError::BadRequest {
            code: "validation_error",
            message: "id is required".to_string(),
        });
    }
    let Some(plugin) = find_installed_plugin(id) else {
        return Err(plugin_not_found(id.to_string()));
    };
    let Some(cache_path) = plugin.cache_path.clone() else {
        return Ok(PluginTestConnectionResponse {
            plugin,
            status: "failed".to_string(),
            servers: vec![manifest_failure_result(
                "Plugin has no installed cache path".to_string(),
            )],
        });
    };
    let manifest = match allthecodes_plugins::manifest::load_manifest(&cache_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Ok(PluginTestConnectionResponse {
                plugin,
                status: "failed".to_string(),
                servers: vec![manifest_failure_result(format!(
                    "Failed to load plugin manifest: {error:#}"
                ))],
            });
        }
    };

    let configs = plugin_mcp_test_configs(&plugin, &cache_path, &manifest);
    let mut servers = Vec::new();
    for config in configs {
        servers.push(test_mcp_server_connection(&plugin, config).await);
    }
    let status = aggregate_connection_status(&servers);
    Ok(PluginTestConnectionResponse {
        plugin,
        status,
        servers,
    })
}

fn manifest_failure_result(message: String) -> PluginMcpConnectionTestResult {
    PluginMcpConnectionTestResult {
        server: "manifest".to_string(),
        status: "failed".to_string(),
        message: message.clone(),
        command: None,
        tools: None,
        resources: None,
        checks: vec![PluginMcpConnectionCheck {
            name: "manifest".to_string(),
            status: "failed".to_string(),
            message,
        }],
    }
}

fn plugin_mcp_test_configs(
    plugin: &PluginEntry,
    cache_path: &Path,
    manifest: &PluginManifest,
) -> Vec<McpServerConfig> {
    let mut discovered = plugin_runtime_configs(&plugin.id)
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect::<HashMap<_, _>>();

    manifest
        .mcp_servers
        .iter()
        .map(|mcp| {
            if let Some(config) = discovered.remove(&mcp.name) {
                return config;
            }
            let mut env = mcp.env.clone();
            if is_official_plugin(plugin) {
                env.insert(
                    "ALLTHECODES_COM_BASE_URL".to_string(),
                    "https://allthecodes.cc".to_string(),
                );
            }
            McpServerConfig {
                name: mcp.name.clone(),
                transport: "stdio".to_string(),
                command: Some(resolve_plugin_mcp_command(cache_path, &mcp.command)),
                args: Some(mcp.args.clone()),
                url: None,
                headers: None,
                oauth: None,
                env: Some(env),
                browser_mcp: None,
                disabled: None,
                bearer_token_env_var: None,
                env_http_headers: None,
                auth: None,
            }
        })
        .collect()
}

async fn test_mcp_server_connection(
    plugin: &PluginEntry,
    config: McpServerConfig,
) -> PluginMcpConnectionTestResult {
    let mut checks = Vec::new();
    checks.push(check("manifest", "ready", "MCP server is declared"));

    let command = config.command.clone();
    let mut hard_failure = None;
    if config.transport == "stdio" {
        match command.as_deref() {
            Some(command) if !command.trim().is_empty() => {
                checks.push(check("command", "ready", format!("Command: {command}")));
                if command_has_path_components(command) {
                    match std::fs::metadata(command) {
                        Ok(metadata) if metadata.is_file() => {
                            checks.push(check("command_file", "ready", "Command file exists"));
                            if command_is_executable(&metadata) {
                                checks.push(check(
                                    "executable",
                                    "ready",
                                    "Command file is executable",
                                ));
                            } else {
                                hard_failure = Some("Command file is not executable".to_string());
                                checks.push(check(
                                    "executable",
                                    "failed",
                                    "Command file is not executable",
                                ));
                            }
                        }
                        Ok(_) => {
                            hard_failure = Some("Command path is not a file".to_string());
                            checks.push(check(
                                "command_file",
                                "failed",
                                "Command path is not a file",
                            ));
                        }
                        Err(error) => {
                            hard_failure = Some(format!("Command file is missing: {error}"));
                            checks.push(check(
                                "command_file",
                                "failed",
                                format!("Command file is missing: {error}"),
                            ));
                        }
                    }
                } else {
                    checks.push(check(
                        "command_file",
                        "warning",
                        "Command will be resolved from PATH",
                    ));
                }
            }
            _ => {
                hard_failure = Some("stdio MCP server is missing a command".to_string());
                checks.push(check(
                    "command",
                    "failed",
                    "stdio MCP server is missing a command",
                ));
            }
        }
    }

    if is_official_plugin(plugin) {
        let token_present = config
            .env
            .as_ref()
            .and_then(|env| env.get("ALLTHECODES_COM_ACCESS_TOKEN"))
            .is_some_and(|token| !token.trim().is_empty());
        if token_present {
            checks.push(check(
                "account_token",
                "ready",
                "Account access token is present",
            ));
        } else {
            hard_failure = Some("Official plugin MCP server is missing account token".to_string());
            checks.push(check(
                "account_token",
                "failed",
                "Official plugin MCP server is missing account token",
            ));
        }
    }

    if let Some(message) = hard_failure {
        return PluginMcpConnectionTestResult {
            server: config.name,
            status: "failed".to_string(),
            message,
            command,
            tools: None,
            resources: None,
            checks,
        };
    }

    let mut client = McpClient::new(config.clone());
    if let Err(error) = client.connect().await {
        checks.push(check("connect", "failed", error.to_string()));
        return PluginMcpConnectionTestResult {
            server: config.name,
            status: "failed".to_string(),
            message: format!("MCP connect failed: {error}"),
            command,
            tools: None,
            resources: None,
            checks,
        };
    }
    checks.push(check("connect", "ready", "MCP process connected"));

    if let Err(error) = client.initialize().await {
        checks.push(check("initialize", "failed", error.to_string()));
        client.disconnect().await;
        return PluginMcpConnectionTestResult {
            server: config.name,
            status: "failed".to_string(),
            message: format!("MCP initialize failed: {error}"),
            command,
            tools: None,
            resources: None,
            checks,
        };
    }
    checks.push(check("initialize", "ready", "MCP initialize completed"));

    let mut status = "ready".to_string();
    let mut message = "MCP server is ready".to_string();
    let mut tools = None;
    let mut resources = None;

    if client.supports_tools() {
        match client.list_tools().await {
            Ok(list) => {
                tools = Some(list.len());
                checks.push(check(
                    "tools_list",
                    "ready",
                    format!("{} tools", list.len()),
                ));
            }
            Err(error) => {
                status = "warning".to_string();
                message = format!("tools/list failed: {error}");
                checks.push(check("tools_list", "warning", error.to_string()));
            }
        }
    } else {
        tools = Some(0);
        checks.push(check(
            "tools_list",
            "warning",
            "Server does not advertise tools",
        ));
    }

    if client.supports_resources() {
        match client.list_resources().await {
            Ok(list) => {
                resources = Some(list.len());
                checks.push(check(
                    "resources_list",
                    "ready",
                    format!("{} resources", list.len()),
                ));
            }
            Err(error) => {
                status = "warning".to_string();
                if message == "MCP server is ready" {
                    message = format!("resources/list failed: {error}");
                }
                checks.push(check("resources_list", "warning", error.to_string()));
            }
        }
    } else {
        resources = Some(0);
        checks.push(check(
            "resources_list",
            "warning",
            "Server does not advertise resources",
        ));
    }

    if tools == Some(0) && resources == Some(0) && status == "ready" {
        status = "warning".to_string();
        message = "MCP initialized but returned 0 tools and 0 resources".to_string();
    }

    client.disconnect().await;
    PluginMcpConnectionTestResult {
        server: config.name,
        status,
        message,
        command,
        tools,
        resources,
        checks,
    }
}

fn check(
    name: impl Into<String>,
    status: impl Into<String>,
    message: impl Into<String>,
) -> PluginMcpConnectionCheck {
    PluginMcpConnectionCheck {
        name: name.into(),
        status: status.into(),
        message: message.into(),
    }
}

fn aggregate_connection_status(servers: &[PluginMcpConnectionTestResult]) -> String {
    if servers.is_empty() {
        return "warning".to_string();
    }
    if servers.iter().any(|server| server.status == "failed") {
        return "failed".to_string();
    }
    if servers.iter().any(|server| server.status == "warning") {
        return "warning".to_string();
    }
    "ready".to_string()
}

fn is_official_plugin(plugin: &PluginEntry) -> bool {
    plugin.official || plugin.marketplace.as_deref() == Some(OFFICIAL_MARKETPLACE_SOURCE_NAME)
}

fn resolve_plugin_mcp_command(plugin_root: &Path, command: &str) -> String {
    let path = Path::new(command);
    if path.is_absolute() || !command_has_path_components(command) {
        command.to_string()
    } else {
        plugin_root.join(path).to_string_lossy().to_string()
    }
}

fn command_has_path_components(command: &str) -> bool {
    command.contains('/') || command.contains('\\') || Path::new(command).is_absolute()
}

#[cfg(unix)]
fn command_is_executable(metadata: &std::fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn command_is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

pub(crate) fn refresh_plugin_contributed_skills(state: &WebState) {
    let engine = state.engine();
    let cwd = engine.cwd();
    let plugin_skills = load_plugin_contributed_skills();
    let report = allthecodes_skills::reload_skills_with_extra(
        &allthecodes_config::paths::skills_dir_global(),
        Some(Path::new(&cwd)),
        plugin_skills,
        allthecodes_skills::SkillLoadOptions::for_app_version(state.app_version()),
    );

    if report.error_count() > 0 || report.warning_count() > 0 {
        warn!(
            loaded = report.loaded,
            skipped = report.skipped,
            revision = report.revision,
            warnings = report.warning_count(),
            errors = report.error_count(),
            "Plugin: refreshed contributed skills with diagnostics"
        );
    }
}

fn load_plugin_contributed_skills() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();

    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                warn!(
                    plugin = %contributed.plugin_id,
                    path = %contributed.path.display(),
                    "Plugin: failed to load contributed skill file"
                );
                continue;
            }
        };

        skill.name = contributed.name;
        if let Some(desc) = contributed.description {
            if !desc.trim().is_empty() {
                skill.frontmatter.description = desc;
            }
        }
        out.push(skill);
    }

    out
}

fn set_installed_plugin_status(
    id: &str,
    status: PluginStatus,
) -> anyhow::Result<Option<PluginEntry>> {
    let mut installed = allthecodes_plugins::loader::load_installed_plugins();
    let Some(plugin) = installed.iter_mut().find(|plugin| plugin.id == id) else {
        return Ok(None);
    };
    plugin.status = status.clone();
    let updated = plugin.clone();
    allthecodes_plugins::loader::save_installed_plugins(&installed)?;
    allthecodes_plugins::set_plugin_status(id, status);
    Ok(Some(updated))
}

fn plugin_value(plugin: PluginEntry) -> Result<serde_json::Value, ProtocolApiError> {
    serde_json::to_value(plugin).map_err(|error| ProtocolApiError::Internal {
        message: error.to_string(),
    })
}

fn plugin_not_found(id: String) -> ProtocolApiError {
    ProtocolApiError::NotFound {
        entity: "plugin",
        id,
    }
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "plugin",
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
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use axum::response::IntoResponse;
    use serde_json::json;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn marketplace_handler_lists_builtin_superpowers() {
        let (_home, _guard) = temp_home();
        let response = plugins_marketplace_handler(State(make_web_state()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let plugins = body["plugins"].as_array().expect("plugins");
        assert!(plugins.iter().any(|plugin| {
            plugin["id"] == json!("superpowers")
                && plugin["name"] == json!("Superpowers")
                && plugin["source_name"] == json!("default-marketplace-source")
                && plugin["homepage"] == json!("https://github.com/obra/superpowers")
        }));
    }
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod api_tests;
