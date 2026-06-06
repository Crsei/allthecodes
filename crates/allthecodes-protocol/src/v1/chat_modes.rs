//! V1 API request/response types for the chat_modes domain.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeBundle {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub plugin_ids: Vec<String>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub mcp_server_names: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub built_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResolved {
    pub bundle: ChatModeBundle,
    pub status: String,
    pub missing_plugins: Vec<String>,
    pub missing_skills: Vec<String>,
    pub missing_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModesResponse {
    pub modes: Vec<ChatModeResolved>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResourcesResponse {
    pub plugins: Vec<ModePluginResource>,
    pub skills: Vec<ModeSkillResource>,
    pub mcp_servers: Vec<ModeMcpServerResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModePluginResource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeSkillResource {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub source: String,
    pub description: String,
    pub model_invocable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeMcpServerResource {
    pub name: String,
    pub scope: String,
    pub transport: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeBundleUpsertRequest {
    pub bundle: ChatModeBundle,
}

fn default_enabled() -> bool {
    true
}
