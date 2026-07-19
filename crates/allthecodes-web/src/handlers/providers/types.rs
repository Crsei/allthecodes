use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::handlers::models::ModelSummary;

pub use allthecodes_protocol::v1::providers::{
    ProviderDetailResponse, ProviderReplaceRequest, ProviderSecretUpdate,
};

#[derive(Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_proxy: Option<ProviderApiProxy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderApiProxy {
    pub available: bool,
    pub status: ProviderApiProxyStatus,
    pub base_url: String,
    pub endpoints: ProviderApiProxyEndpoints,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApiProxyStatus {
    Ready,
    ProviderDisabled,
    MissingCredential,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderApiProxyEndpoints {
    pub anthropic_messages: String,
    pub anthropic_count_tokens: String,
    pub openai_responses: String,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
pub struct ProviderListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub providers: Vec<ProviderSummary>,
    pub presets: Vec<ProviderPreset>,
}

#[derive(Deserialize)]
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
    #[serde(default, alias = "requestMaxRetries")]
    pub request_max_retries: Option<u8>,
    #[serde(default, alias = "streamMaxRetries")]
    pub stream_max_retries: Option<u8>,
    #[serde(default, alias = "streamIdleTimeoutMs")]
    pub stream_idle_timeout_ms: Option<u64>,
    #[serde(default, alias = "requestTimeoutMs")]
    pub request_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
pub struct ProviderUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub base_url: PatchField<String>,
    #[serde(default)]
    pub api_key: PatchField<String>,
    #[serde(default)]
    pub command: PatchField<String>,
    #[serde(default)]
    pub arguments: PatchField<Vec<String>>,
    #[serde(default)]
    pub env: PatchField<HashMap<String, String>>,
    #[serde(default)]
    pub models: PatchField<Vec<String>>,
    #[serde(default)]
    pub provider_options: PatchField<Value>,
    #[serde(default, alias = "requestMaxRetries")]
    pub request_max_retries: PatchField<u8>,
    #[serde(default, alias = "streamMaxRetries")]
    pub stream_max_retries: PatchField<u8>,
    #[serde(default, alias = "streamIdleTimeoutMs")]
    pub stream_idle_timeout_ms: PatchField<u64>,
    #[serde(default, alias = "requestTimeoutMs")]
    pub request_timeout_ms: PatchField<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchField<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn patch_distinguishes_missing_null_and_value() {
        let missing: ProviderUpdateRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.base_url, PatchField::Missing);
        let null: ProviderUpdateRequest = serde_json::from_str(r#"{"base_url":null}"#).unwrap();
        assert_eq!(null.base_url, PatchField::Null);
        let value: ProviderUpdateRequest =
            serde_json::from_str(r#"{"base_url":"https://example.test"}"#).unwrap();
        assert_eq!(
            value.base_url,
            PatchField::Value("https://example.test".to_string())
        );
    }
}

impl<'de, T> Deserialize<'de> for PatchField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

#[derive(Serialize)]
pub struct ModelDiscoveryResponse {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexLocalStatusResponse {
    pub cli_installed: bool,
    pub auth_present: bool,
    pub auth_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub settings_path: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexApplyLocalResponse {
    pub cli_installed: bool,
    pub auth_present: bool,
    pub auth_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub settings_path: String,
    pub active: bool,
    pub model: String,
    pub message: String,
}
