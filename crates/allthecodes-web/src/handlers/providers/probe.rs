use axum::extract::Path as AxumPath;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use allthecodes_api::api::provider_runtime::{
    probe_http, probe_websocket, ProviderEndpoint, ProviderErrorKind, ProviderProbeReport,
    ProviderProbeStatus as RuntimeProbeStatus, ProviderStreamTransport,
};
use allthecodes_config::settings::{load_global_config, ProviderProfileSettings};
use allthecodes_protocol::v1::providers::{
    ProviderProbeErrorKind, ProviderProbeMetadata, ProviderProbeRequest, ProviderProbeResponse,
    ProviderProbeStatus, ProviderProbeTransport,
};

use super::helpers::normalized_non_empty;

/// POST /api/providers/{id}/probe — Probe provider connectivity.
pub async fn providers_probe_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProviderProbeRequest>,
) -> Response {
    let settings = load_global_config().unwrap_or_default();
    let profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&id));

    let endpoint = match provider_endpoint_for_probe(&id, profile) {
        Ok(endpoint) => endpoint,
        Err(response) => return Json(*response).into_response(),
    };

    let report = match req.transport {
        ProviderProbeTransport::Http => probe_http(&endpoint, &id).await,
        ProviderProbeTransport::WebSocket => probe_websocket(&endpoint, &id).await,
    };
    Json(provider_probe_response(report)).into_response()
}

pub(super) fn provider_endpoint_for_probe(
    id: &str,
    profile: Option<&ProviderProfileSettings>,
) -> Result<ProviderEndpoint, Box<ProviderProbeResponse>> {
    let kind = profile
        .and_then(|profile| profile.api_provider.as_deref())
        .unwrap_or(id);
    let info = allthecodes_api::api::providers::get_provider(kind);
    let Some(protocol) = info.map(|info| info.protocol).or_else(|| {
        profile.map(|_| allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat)
    }) else {
        return Err(Box::new(provider_probe_local_error(
            id,
            ProviderProbeStatus::Error,
            Some(ProviderProbeErrorKind::InvalidRequest),
            format!("Provider '{id}' is not configured"),
            None,
        )));
    };
    let base_url = profile
        .and_then(|profile| normalized_non_empty(profile.base_url.as_deref()))
        .or_else(|| info.map(|info| info.base_url.to_string()))
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let env_key = info.map(|info| info.env_key).unwrap_or("OPENAI_API_KEY");
    let Some(api_key) = provider_probe_api_key(profile, env_key) else {
        return Err(Box::new(provider_probe_local_error(
            id,
            ProviderProbeStatus::AuthFailed,
            Some(ProviderProbeErrorKind::AuthenticationFailed),
            format!("Provider '{id}' has no configured credential"),
            allthecodes_api::api::providers::base_url_host(&base_url),
        )));
    };

    match protocol {
        allthecodes_api::api::providers::ProviderProtocol::Anthropic => {
            let auth = allthecodes_api::api::client::AnthropicAuth::ApiKey(api_key);
            let direct_official_anthropic =
                allthecodes_api::api::providers::base_url_host(&base_url).as_deref()
                    == Some("api.anthropic.com");
            ProviderEndpoint::anthropic(&auth, base_url, direct_official_anthropic, &json!({}))
                .map_err(|error| {
                    Box::new(provider_probe_local_error(
                        id,
                        ProviderProbeStatus::Error,
                        Some(ProviderProbeErrorKind::InvalidRequest),
                        error.to_string(),
                        None,
                    ))
                })
        }
        allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat => {
            ProviderEndpoint::openai_compat(kind, base_url, &api_key).map_err(|error| {
                Box::new(provider_probe_local_error(
                    id,
                    ProviderProbeStatus::Error,
                    Some(ProviderProbeErrorKind::InvalidRequest),
                    error.to_string(),
                    None,
                ))
            })
        }
        allthecodes_api::api::providers::ProviderProtocol::Google => {
            Ok(ProviderEndpoint::google(base_url, &api_key))
        }
    }
}

fn provider_probe_api_key(
    profile: Option<&ProviderProfileSettings>,
    env_key: &str,
) -> Option<String> {
    profile
        .and_then(|profile| normalized_non_empty(profile.api_key.as_deref()))
        .or_else(|| {
            profile
                .and_then(|profile| profile.env.as_ref())
                .and_then(|env| env.get(env_key))
                .and_then(|value| normalized_non_empty(Some(value.as_str())))
        })
        .or_else(|| {
            std::env::var(env_key)
                .ok()
                .and_then(|value| normalized_non_empty(Some(value.as_str())))
        })
}

fn provider_probe_local_error(
    provider_id: &str,
    status: ProviderProbeStatus,
    error_kind: Option<ProviderProbeErrorKind>,
    message: String,
    base_url_host: Option<String>,
) -> ProviderProbeResponse {
    ProviderProbeResponse {
        provider_id: provider_id.to_string(),
        transport: ProviderProbeTransport::WebSocket,
        status,
        error_kind,
        message,
        base_url_host,
        request_id: None,
        metadata: None,
    }
}

fn provider_probe_response(report: ProviderProbeReport) -> ProviderProbeResponse {
    ProviderProbeResponse {
        provider_id: report.provider_id,
        transport: protocol_probe_transport(report.transport),
        status: protocol_probe_status(report.status),
        error_kind: report.error_kind.map(protocol_probe_error_kind),
        message: report.message,
        base_url_host: report.base_url_host,
        request_id: report.request_id,
        metadata: report.metadata.map(|metadata| ProviderProbeMetadata {
            provider: metadata.provider,
            transport: protocol_probe_transport(metadata.transport),
            status: metadata.status,
            request_id: metadata.request_id,
            headers: metadata.headers,
        }),
    }
}

fn protocol_probe_transport(transport: ProviderStreamTransport) -> ProviderProbeTransport {
    match transport {
        ProviderStreamTransport::SseHttp => ProviderProbeTransport::Http,
        ProviderStreamTransport::WebSocket => ProviderProbeTransport::WebSocket,
    }
}

fn protocol_probe_status(status: RuntimeProbeStatus) -> ProviderProbeStatus {
    match status {
        RuntimeProbeStatus::Ok => ProviderProbeStatus::Ok,
        RuntimeProbeStatus::Unsupported => ProviderProbeStatus::Unsupported,
        RuntimeProbeStatus::AuthFailed => ProviderProbeStatus::AuthFailed,
        RuntimeProbeStatus::NetworkError => ProviderProbeStatus::NetworkError,
        RuntimeProbeStatus::QuotaOrRateLimited => ProviderProbeStatus::QuotaOrRateLimited,
        RuntimeProbeStatus::Error => ProviderProbeStatus::Error,
    }
}

fn protocol_probe_error_kind(kind: ProviderErrorKind) -> ProviderProbeErrorKind {
    match kind {
        ProviderErrorKind::ContextWindowExceeded => ProviderProbeErrorKind::ContextWindowExceeded,
        ProviderErrorKind::QuotaExceeded => ProviderProbeErrorKind::QuotaExceeded,
        ProviderErrorKind::RateLimited => ProviderProbeErrorKind::RateLimited,
        ProviderErrorKind::PolicyBlocked => ProviderProbeErrorKind::PolicyBlocked,
        ProviderErrorKind::ServerOverloaded => ProviderProbeErrorKind::ServerOverloaded,
        ProviderErrorKind::RetryableTransport => ProviderProbeErrorKind::RetryableTransport,
        ProviderErrorKind::AuthenticationFailed => ProviderProbeErrorKind::AuthenticationFailed,
        ProviderErrorKind::InvalidRequest => ProviderProbeErrorKind::InvalidRequest,
        ProviderErrorKind::UnknownProviderError => ProviderProbeErrorKind::UnknownProviderError,
    }
}
