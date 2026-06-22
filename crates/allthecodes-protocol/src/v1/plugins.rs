use std::path::PathBuf;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Response to GET /api/plugins
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginsListResponse {
    pub plugins: Vec<Value>,
    pub diagnostics: Vec<Value>,
}

/// Response to GET /api/plugins/marketplace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginsMarketplaceResponse {
    pub plugins: Vec<Value>,
    pub sources: Vec<Value>,
}

/// Request body for POST /api/plugins/install
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PluginInstallRequest {
    Legacy(PluginLegacyInstallRequest),
    Official(PluginOfficialInstallRequest),
}

/// Legacy request body for POST /api/plugins/install
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginLegacyInstallRequest {
    pub source: String,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Official marketplace install/update request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginOfficialInstallRequest {
    pub id: String,
    pub version: String,
    pub download_url: String,
    #[serde(default, alias = "checksum")]
    pub sha256: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

/// Response body for POST /api/plugins/install
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginInstallResponse {
    pub plugin: Value,
    pub install_path: PathBuf,
    pub fresh_install: bool,
    #[serde(default)]
    pub status: String,
}

/// Request body for POST /api/plugins/{id}/uninstall
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginUninstallRequest {
    #[serde(default)]
    pub purge: bool,
}

/// Response body for POST /api/plugins/{id}/uninstall
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginUninstallResponse {
    pub plugin: Value,
    pub purged: bool,
}

/// Request body for plugin lifecycle operations that target one plugin ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginIdRequest {
    pub id: String,
}

/// Request body for POST /api/plugins/uninstall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginUninstallByIdRequest {
    pub id: String,
    #[serde(default)]
    pub purge: bool,
}

/// Request body for POST /api/plugins/update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PluginUpdateRequest {
    Official(PluginOfficialInstallRequest),
    Id(PluginIdRequest),
}

/// Generic response for plugin lifecycle operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginLifecycleResponse {
    pub plugin: Value,
    pub status: String,
}

/// Response body for POST /api/plugins/{id}/test-connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginTestConnectionResponse {
    pub plugin: Value,
    pub status: String,
    pub servers: Vec<PluginMcpConnectionTestResult>,
}

/// Per-MCP-server connection diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginMcpConnectionTestResult {
    pub server: String,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub tools: Option<usize>,
    #[serde(default)]
    pub resources: Option<usize>,
    #[serde(default)]
    pub checks: Vec<PluginMcpConnectionCheck>,
}

/// One diagnostic check within an MCP connection test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginMcpConnectionCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}
