//! Plugin REST handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_plugins::installation::{
    install_plugin, uninstall_plugin_ext, InstallError, InstallScope,
};
use allthecodes_plugins::loader::{load_installed_plugins_report, PluginDiagnostic};
use allthecodes_plugins::manifest::PluginManifest;
use allthecodes_plugins::marketplace::{
    MarketplacePluginEntry, MarketplaceSource, GLOBAL_MARKETPLACE_INDEX,
};
use allthecodes_plugins::PluginEntry;

use allthecodes_protocol::v1::plugins::PluginInstallResponse as ProtocolPluginInstallResponse;
use allthecodes_protocol::v1::plugins::PluginsListResponse as ProtocolPluginsListResponse;
use allthecodes_protocol::v1::plugins::PluginsMarketplaceResponse as ProtocolPluginsMarketplaceResponse;
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
    type Request = allthecodes_protocol::v1::plugins::PluginInstallRequest;
    type Response = ProtocolPluginInstallResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "plugins.install"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        let scope = match parse_install_scope(request.scope.as_deref()) {
            Ok(scope) => scope,
            Err(error) => {
                return Err(ProtocolApiError::BadRequest {
                    code: "validation_error",
                    message: error,
                })
            }
        };
        let source =
            resolve_source_against_cwd(&request.source, Path::new(&self.state.engine().cwd()));
        let available_plugins = HashMap::<String, String>::new();
        let manifests = HashMap::<String, PluginManifest>::new();

        match install_plugin(
            &source,
            Some(scope),
            Some(self.state.app_version()),
            None,
            &available_plugins,
            &manifests,
        )
        .await
        {
            Ok(result) => {
                let handler_resp = PluginInstallResponse {
                    plugin: result.plugin,
                    install_path: result.install_path,
                    fresh_install: result.fresh_install,
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
            Err(error) => {
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
                Err(match error {
                    InstallError::AlreadyInstalled(_) => {
                        ProtocolApiError::Conflict { reason: err_str }
                    }
                    _ => ProtocolApiError::BadRequest {
                        code: "validation_error",
                        message: err_str,
                    },
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::PluginsList, get(plugins_list_handler))
        .handle(
            ApiMethod::PluginsMarketplace,
            get(plugins_marketplace_handler),
        )
        .handle(ApiMethod::PluginsInstall, post(plugins_install_handler))
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

#[derive(Deserialize)]
pub struct PluginInstallRequest {
    pub source: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Serialize)]
pub struct PluginInstallResponse {
    pub plugin: PluginEntry,
    pub install_path: PathBuf,
    pub fresh_install: bool,
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

/// GET /api/plugins
pub async fn plugins_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PluginsListProcessor>(state, ApiMethod::PluginsList, NoParams {})
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
    Json(req): Json<allthecodes_protocol::v1::plugins::PluginInstallRequest>,
) -> Response {
    rest_processor_response::<PluginsInstallProcessor>(state, ApiMethod::PluginsInstall, req).await
}

/// POST /api/plugins/{id}/uninstall
pub async fn plugins_uninstall_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PluginUninstallRequest>,
) -> Response {
    match uninstall_plugin_ext(&id, req.purge) {
        Ok(Some(plugin)) => Json(PluginUninstallResponse {
            plugin,
            purged: req.purge,
        })
        .into_response(),
        Ok(None) => not_found(format!("Plugin '{}' not found", id)),
        Err(error) => internal_error(error.to_string()),
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
