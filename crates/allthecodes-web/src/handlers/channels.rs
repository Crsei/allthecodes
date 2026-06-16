//! Remote channel / gateway REST handlers.

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_daemon::gateway_client::{
    GatewayCapabilitiesSnapshot, LocalGatewayClient, LocalGatewayDaemonStatus,
};
use allthecodes_gateway::{AdapterProvider, AdapterStatus, GatewayDiagnostic};

use allthecodes_protocol::{ApiError as ProtocolApiError, ApiErrorBody};

type BoxResponse = Box<Response>;

#[derive(Serialize)]
pub struct ChannelsResponse {
    pub daemon: ChannelDaemonInfo,
    pub adapters: Vec<AdapterStatus>,
}

#[derive(Serialize)]
pub struct ChannelsCapabilitiesResponse {
    pub daemon: ChannelDaemonInfo,
    pub gateway: ChannelGatewayCapabilities,
}

#[derive(Serialize)]
pub struct ChannelDaemonInfo {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Serialize)]
pub struct ChannelGatewayCapabilities {
    pub version: String,
    pub auth_mode: Value,
    pub supports_steer: bool,
    pub max_running: usize,
    pub max_queued: usize,
    pub endpoints: Vec<String>,
}

#[derive(Deserialize)]
pub struct ChannelTestRequest {
    pub target: String,
    pub text: String,
}

/// GET /api/channels
pub async fn channels_list_handler() -> Response {
    let (daemon, client) = match running_gateway_client() {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    match client.adapters().await {
        Ok(adapters) => Json(ChannelsResponse { daemon, adapters }).into_response(),
        Err(diagnostic) => diagnostic_response(diagnostic),
    }
}

/// GET /api/channels/capabilities
pub async fn channels_capabilities_handler() -> Response {
    let (daemon, client) = match running_gateway_client() {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    match client.capabilities().await {
        Ok(snapshot) => Json(ChannelsCapabilitiesResponse {
            daemon,
            gateway: capabilities_from_snapshot(snapshot),
        })
        .into_response(),
        Err(diagnostic) => diagnostic_response(diagnostic),
    }
}

/// POST /api/channels/{provider}/connect
pub async fn channels_connect_handler(AxumPath(provider): AxumPath<String>) -> Response {
    let provider = match parse_provider(&provider) {
        Ok(provider) => provider,
        Err(diagnostic) => return diagnostic_response(diagnostic),
    };
    let (_daemon, client) = match running_gateway_client() {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    match client.connect_adapter(provider).await {
        Ok(status) => Json(status).into_response(),
        Err(diagnostic) => diagnostic_response(diagnostic),
    }
}

/// POST /api/channels/{provider}/test
pub async fn channels_test_handler(
    AxumPath(provider): AxumPath<String>,
    Json(req): Json<ChannelTestRequest>,
) -> Response {
    let provider = match parse_provider(&provider) {
        Ok(provider) => provider,
        Err(diagnostic) => return diagnostic_response(diagnostic),
    };
    let (_daemon, client) = match running_gateway_client() {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    match client
        .test_adapter_message(provider, req.target, req.text)
        .await
    {
        Ok(status) => Json(status).into_response(),
        Err(diagnostic) => diagnostic_response(diagnostic),
    }
}

fn running_gateway_client() -> Result<(ChannelDaemonInfo, LocalGatewayClient), BoxResponse> {
    let status = LocalGatewayClient::daemon_status().map_err(|error| {
        Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_state_unavailable",
            format!("The daemon state could not be read: {}", error),
        ))
    })?;

    match status {
        LocalGatewayDaemonStatus::Running { pid, base_url, .. } => {
            let daemon = ChannelDaemonInfo {
                status: "running".to_string(),
                base_url: Some(base_url),
                pid: Some(pid),
            };
            let client = LocalGatewayClient::from_running_daemon()
                .map_err(|diagnostic| Box::new(diagnostic_response(diagnostic)))?;
            Ok((daemon, client))
        }
        LocalGatewayDaemonStatus::Stale { pid } => Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_stale",
            format!("The daemon state is stale for pid {}", pid),
        ))),
        LocalGatewayDaemonStatus::Stopped => Err(Box::new(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_stopped",
            "The daemon is not running".to_string(),
        ))),
    }
}

fn parse_provider(provider: &str) -> Result<AdapterProvider, GatewayDiagnostic> {
    AdapterProvider::parse(provider).map_err(|error| error.into_diagnostic())
}

fn capabilities_from_snapshot(snapshot: GatewayCapabilitiesSnapshot) -> ChannelGatewayCapabilities {
    ChannelGatewayCapabilities {
        version: snapshot.version,
        auth_mode: snapshot.auth_mode,
        supports_steer: snapshot.supports_steer,
        max_running: snapshot.max_running,
        max_queued: snapshot.max_queued,
        endpoints: snapshot.endpoints,
    }
}

fn diagnostic_response(diagnostic: GatewayDiagnostic) -> Response {
    let status = status_for_diagnostic(&diagnostic.code);
    (
        status,
        Json(ApiErrorBody {
            error: diagnostic.message,
            code: diagnostic.code,
            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn status_for_diagnostic(code: &str) -> StatusCode {
    match code {
        "adapter_unsupported" | "invalid_adapter_provider" => StatusCode::BAD_REQUEST,
        "daemon_stopped"
        | "daemon_stale"
        | "control_token_missing"
        | "control_token_unavailable"
        | "gateway_unreachable"
        | "daemon_state_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

fn error_response(status: StatusCode, code: &'static str, error: impl Into<String>) -> Response {
    let body = ProtocolApiError::BadRequest {
        code,
        message: error.into(),
    }
    .into_body();
    (status, Json(body)).into_response()
}

#[cfg(test)]
#[path = "channels_tests.rs"]
mod tests;
