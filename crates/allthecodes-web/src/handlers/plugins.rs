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

use crate::handlers::ApiError;
use crate::state::WebState;

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
pub async fn plugins_list_handler() -> impl IntoResponse {
    Json(plugins_list_response())
}

/// GET /api/plugins/marketplace
pub async fn plugins_marketplace_handler() -> impl IntoResponse {
    Json(PluginsMarketplaceResponse {
        plugins: GLOBAL_MARKETPLACE_INDEX.list_all_entries(),
        sources: GLOBAL_MARKETPLACE_INDEX.list_sources(),
    })
}

/// POST /api/plugins/install
pub async fn plugins_install_handler(
    State(state): State<WebState>,
    Json(req): Json<PluginInstallRequest>,
) -> Response {
    let scope = match parse_install_scope(req.scope.as_deref()) {
        Ok(scope) => scope,
        Err(error) => return validation_error(error).into_response(),
    };
    let source = resolve_source_against_cwd(&req.source, Path::new(&state.engine().cwd()));
    let available_plugins = HashMap::<String, String>::new();
    let manifests = HashMap::<String, PluginManifest>::new();

    match install_plugin(
        &source,
        Some(scope),
        Some(env!("CARGO_PKG_VERSION")),
        None,
        &available_plugins,
        &manifests,
    )
    .await
    {
        Ok(result) => Json(PluginInstallResponse {
            plugin: result.plugin,
            install_path: result.install_path,
            fresh_install: result.fresh_install,
        })
        .into_response(),
        Err(error) => install_error(error).into_response(),
    }
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
        Ok(None) => not_found(format!("Plugin '{}' not found", id)).into_response(),
        Err(error) => internal_error(error.to_string()).into_response(),
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

fn install_error(error: InstallError) -> (StatusCode, Json<ApiError>) {
    match error {
        InstallError::AlreadyInstalled(plugin) => (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!("Plugin '{}' is already installed", plugin),
                code: "plugin_already_installed".into(),

                details: serde_json::json!({}),
            }),
        ),
        InstallError::SourceNotFound(message)
        | InstallError::ValidationFailed(message)
        | InstallError::MissingDependency(message)
        | InstallError::PolicyBlocked(message)
        | InstallError::DownloadFailed(message) => validation_error(message),
        InstallError::EngineIncompatible { required, current } => validation_error(format!(
            "Plugin engine version incompatible: required {}, running {}",
            required, current
        )),
        InstallError::MaxPluginsReached { max } => {
            validation_error(format!("Max plugins limit reached ({})", max))
        }
        InstallError::Other(message) => internal_error(message),
    }
}

fn validation_error(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error,
            code: "validation_error".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn not_found(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error,
            code: "not_found".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn internal_error(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error,
            code: "internal_error".into(),

            details: serde_json::json!({}),
        }),
    )
}
