use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayActionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayStatusResponse {
    pub status: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    pub id: String,

    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,

    pub diagnostics: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_ref: Option<String>,

    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayListResponse {
    pub gateways: Vec<GatewayStatusResponse>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_gateway_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayActionResponse {
    pub ok: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
