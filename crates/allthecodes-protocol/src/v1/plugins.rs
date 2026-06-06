use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Response to GET /api/plugins
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginsListResponse {
    pub plugins: Vec<Value>,
    pub diagnostics: Vec<Value>,
}

/// Response to GET /api/plugins/marketplace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginsMarketplaceResponse {
    pub plugins: Vec<Value>,
    pub sources: Vec<Value>,
}

/// Request body for POST /api/plugins/install
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInstallRequest {
    pub source: String,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Response body for POST /api/plugins/install
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInstallResponse {
    pub plugin: Value,
    pub install_path: PathBuf,
    pub fresh_install: bool,
}

/// Request body for POST /api/plugins/{id}/uninstall
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginUninstallRequest {
    #[serde(default)]
    pub purge: bool,
}

/// Response body for POST /api/plugins/{id}/uninstall
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginUninstallResponse {
    pub plugin: Value,
    pub purged: bool,
}
