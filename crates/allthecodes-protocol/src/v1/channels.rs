#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ChannelProvider {
    Telegram,
    Lark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfigSnapshot {
    pub provider: ChannelProvider,
    pub enabled: bool,
    pub configured: bool,
    pub has_secret: bool,
    pub api_base_url: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_chat_allowlist: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_target_allowlist: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_updates: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ChannelsConfigResponse {
    pub providers: Vec<ChannelConfigSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfigPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_allowlist: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_chat_allowlist: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_target_allowlist: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_updates: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbound_webhook_url: Option<String>,
}
