use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub auth_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub providers: Vec<ProviderSummary>,
    pub presets: Vec<ProviderPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCreateRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
    #[serde(default)]
    pub provider_options: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
    #[serde(default)]
    pub provider_options: Option<Value>,
}
