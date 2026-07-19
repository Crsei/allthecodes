//! Shared provider runtime primitives.
//!
//! This module keeps provider endpoint construction, response metadata, error
//! classification, and diagnostics probes in one place while the public
//! `ApiClient` streaming API remains compatible.

use std::fmt;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use url::Url;

use crate::api::client::{
    build_anthropic_headers_for_body, build_anthropic_headers_for_body_with_beta_policy,
    is_openai_codex_provider, AnthropicAuth,
};
use crate::api::retry::RetryConfig;
use crate::api::streaming::{normalize_api_error_body, NormalizedApiError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStreamTransport {
    SseHttp,
    WebSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    ContextWindowExceeded,
    QuotaExceeded,
    RateLimited,
    PolicyBlocked,
    ServerOverloaded,
    RetryableTransport,
    AuthenticationFailed,
    InvalidRequest,
    UnknownProviderError,
}

impl ProviderErrorKind {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::ServerOverloaded | Self::RetryableTransport
        )
    }

    pub fn classify(
        status: Option<u16>,
        error_type: Option<&str>,
        message: &str,
    ) -> ProviderErrorKind {
        let mut lower = String::new();
        if let Some(error_type) = error_type {
            lower.push_str(&error_type.to_ascii_lowercase());
            lower.push(' ');
        }
        lower.push_str(&message.to_ascii_lowercase());

        if lower.contains("context_window")
            || lower.contains("context window")
            || lower.contains("prompt_too_long")
            || lower.contains("prompt is too long")
            || lower.contains("too many tokens")
            || lower.contains("maximum context")
        {
            return Self::ContextWindowExceeded;
        }
        if lower.contains("insufficient_quota")
            || lower.contains("quota")
            || lower.contains("billing")
            || lower.contains("credit")
        {
            return Self::QuotaExceeded;
        }
        if lower.contains("rate_limit")
            || lower.contains("rate limit")
            || lower.contains("too many requests")
        {
            return Self::RateLimited;
        }
        if lower.contains("policy")
            || lower.contains("safety")
            || lower.contains("blocked")
            || lower.contains("content_filter")
            || lower.contains("content filter")
        {
            return Self::PolicyBlocked;
        }
        if lower.contains("overloaded")
            || lower.contains("high demand")
            || lower.contains("capacity")
            || lower.contains("unavailable")
        {
            return Self::ServerOverloaded;
        }
        if lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("authentication")
            || lower.contains("invalid api key")
            || lower.contains("invalid_api_key")
        {
            return Self::AuthenticationFailed;
        }
        if lower.contains("invalid request") || lower.contains("bad request") {
            return Self::InvalidRequest;
        }
        if lower.contains("failed to send")
            || lower.contains("error sending")
            || lower.contains("timed out")
            || lower.contains("timeout")
            || lower.contains("connection")
            || lower.contains("connect")
            || lower.contains("dns")
            || lower.contains("eof")
            || lower.contains("network")
        {
            return Self::RetryableTransport;
        }

        match status {
            Some(400) => Self::InvalidRequest,
            Some(401 | 403) => Self::AuthenticationFailed,
            Some(408 | 502 | 503 | 504) => Self::RetryableTransport,
            Some(429) => Self::RateLimited,
            Some(529) => Self::ServerOverloaded,
            Some(500..=599) => Self::ServerOverloaded,
            _ => Self::UnknownProviderError,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub provider: String,
    pub kind: ProviderErrorKind,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub error_type: Option<String>,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl ProviderError {
    pub fn from_normalized(error: NormalizedApiError, retry_after_ms: Option<u64>) -> Self {
        let kind =
            ProviderErrorKind::classify(error.status, error.error_type.as_deref(), &error.message);
        Self {
            provider: error.provider,
            kind,
            status: error.status,
            request_id: error.request_id,
            error_type: error.error_type,
            message: error.message,
            retry_after_ms,
        }
    }

    pub fn transport(provider: impl Into<String>, error: impl fmt::Display) -> Self {
        let message = error.to_string();
        Self {
            provider: provider.into(),
            kind: ProviderErrorKind::classify(None, None, &message),
            status: None,
            request_id: None,
            error_type: None,
            message,
            retry_after_ms: None,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Provider error kind={:?}", self.kind)?;
        write!(f, " provider={}", self.provider)?;
        if let Some(status) = self.status {
            write!(f, " status={status}")?;
        }
        if let Some(request_id) = &self.request_id {
            write!(f, " request_id={request_id}")?;
        }
        if let Some(error_type) = &self.error_type {
            write!(f, " type={error_type}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStreamFailureCategory {
    Transport,
    IdleTimeout,
    IncompleteResponse,
    ProviderFailed,
    Decode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStreamFailure {
    pub category: ProviderStreamFailureCategory,
    pub provider: String,
    pub message: String,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub error_type: Option<String>,
    pub retry_after_ms: Option<u64>,
}

impl ProviderStreamFailure {
    pub fn new(
        category: ProviderStreamFailureCategory,
        provider: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            category,
            provider: provider.into(),
            message: message.into(),
            status: None,
            request_id: None,
            error_type: None,
            retry_after_ms: None,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self.category,
            ProviderStreamFailureCategory::Transport
                | ProviderStreamFailureCategory::IdleTimeout
                | ProviderStreamFailureCategory::IncompleteResponse
        ) || (self.category == ProviderStreamFailureCategory::ProviderFailed
            && ProviderErrorKind::classify(self.status, self.error_type.as_deref(), &self.message)
                .is_retryable())
    }
}

impl fmt::Display for ProviderStreamFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "provider stream failure category={:?} provider={}",
            self.category, self.provider
        )?;
        if let Some(status) = self.status {
            write!(f, " status={status}")?;
        }
        if let Some(request_id) = &self.request_id {
            write!(f, " request_id={request_id}")?;
        }
        if let Some(error_type) = &self.error_type {
            write!(f, " type={error_type}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for ProviderStreamFailure {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderResponseMetadata {
    pub provider: String,
    pub transport: ProviderStreamTransport,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ProviderEndpoint {
    pub provider_name: String,
    pub base_url: String,
    pub headers: HeaderMap,
    pub query_params: Vec<(String, String)>,
    pub retry: RetryConfig,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub transport: ProviderStreamTransport,
    pub websocket_probe_path: Option<String>,
}

impl ProviderEndpoint {
    pub fn anthropic(
        auth: &AnthropicAuth,
        base_url: impl Into<String>,
        direct_official_anthropic: bool,
        body: &serde_json::Value,
    ) -> Result<Self> {
        let headers = if direct_official_anthropic {
            build_anthropic_headers_for_body(auth, false, body)?
        } else {
            build_anthropic_headers_for_body_with_beta_policy(auth, false, body, false)?
        };
        Ok(Self::new("anthropic", base_url, headers))
    }

    pub fn openai_compat(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: &str,
    ) -> Result<Self> {
        let name = name.into();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if is_openai_codex_provider(&name) {
            headers.insert("Accept", HeaderValue::from_static("text/event-stream"));
        }
        if name.eq_ignore_ascii_case("azure") {
            headers.insert(
                "api-key",
                HeaderValue::from_str(api_key).context("provider API key is not a header value")?,
            );
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .context("provider bearer token is not a header value")?,
            );
        }
        let mut endpoint = Self::new(name.clone(), base_url, headers);
        if is_openai_codex_provider(&name) {
            endpoint.websocket_probe_path = Some("/codex/realtime".to_string());
        }
        Ok(endpoint)
    }

    pub fn google(base_url: impl Into<String>, api_key: &str) -> Self {
        let mut endpoint = Self::new("google", base_url, HeaderMap::new());
        endpoint
            .query_params
            .push(("key".to_string(), api_key.to_string()));
        endpoint
    }

    pub fn new(
        provider_name: impl Into<String>,
        base_url: impl Into<String>,
        headers: HeaderMap,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            base_url: base_url.into(),
            headers,
            query_params: Vec::new(),
            retry: RetryConfig::default(),
            request_timeout: Duration::from_secs(120),
            stream_idle_timeout: Duration::from_secs(120),
            transport: ProviderStreamTransport::SseHttp,
            websocket_probe_path: None,
        }
    }

    pub fn url_for_path(&self, path: &str) -> Result<Url> {
        let base = if path.is_empty() {
            self.base_url.trim_end_matches('/').to_string()
        } else {
            format!(
                "{}{}{}",
                self.base_url.trim_end_matches('/'),
                if path.starts_with('/') { "" } else { "/" },
                path
            )
        };
        let mut url = Url::parse(&base).context("provider base URL is invalid")?;
        if !self.query_params.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &self.query_params {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }

    pub fn websocket_url_for_path(&self, path: &str) -> Result<Url> {
        let mut url = self.url_for_path(path)?;
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            "ws" | "wss" => url.scheme(),
            other => anyhow::bail!("provider URL scheme `{other}` cannot be used for WebSocket"),
        }
        .to_string();
        url.set_scheme(&scheme)
            .map_err(|_| anyhow::anyhow!("failed to set WebSocket URL scheme"))?;
        Ok(url)
    }

    pub fn apply_headers(&self, mut builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if !self.headers.is_empty() {
            builder = builder.headers(self.headers.clone());
        }
        builder
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeStatus {
    Ok,
    Unsupported,
    AuthFailed,
    NetworkError,
    QuotaOrRateLimited,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProbeReport {
    pub provider_id: String,
    pub transport: ProviderStreamTransport,
    pub status: ProviderProbeStatus,
    pub error_kind: Option<ProviderErrorKind>,
    pub message: String,
    pub base_url_host: Option<String>,
    pub request_id: Option<String>,
    pub metadata: Option<ProviderResponseMetadata>,
}

pub fn metadata_from_response(
    provider: &str,
    transport: ProviderStreamTransport,
    response: &reqwest::Response,
) -> ProviderResponseMetadata {
    ProviderResponseMetadata {
        provider: provider.to_string(),
        transport,
        status: Some(response.status().as_u16()),
        request_id: request_id_from_headers(response.headers()),
        headers: safe_response_headers(response.headers()),
    }
}

pub async fn provider_error_from_response(
    provider: &str,
    response: reqwest::Response,
) -> ProviderError {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let request_id = request_id_from_headers(&headers);
    let retry_after_ms = retry_after_ms(&headers);
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("(failed to read error body)"));
    let normalized = normalize_api_error_body(provider, Some(status), &body, request_id);
    ProviderError::from_normalized(normalized, retry_after_ms)
}

pub async fn probe_http(endpoint: &ProviderEndpoint, provider_id: &str) -> ProviderProbeReport {
    let base_url_host = crate::api::providers::base_url_host(&endpoint.base_url);
    let Ok(url) = endpoint.url_for_path("") else {
        return ProviderProbeReport {
            provider_id: provider_id.to_string(),
            transport: ProviderStreamTransport::SseHttp,
            status: ProviderProbeStatus::Error,
            error_kind: Some(ProviderErrorKind::InvalidRequest),
            message: "provider base URL is invalid".to_string(),
            base_url_host,
            request_id: None,
            metadata: None,
        };
    };

    let client = match reqwest::Client::builder()
        .timeout(endpoint.request_timeout)
        .user_agent(allthecodes_config::user_agent::api_user_agent())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return probe_error_report(
                provider_id,
                ProviderStreamTransport::SseHttp,
                base_url_host,
                ProviderError::transport(&endpoint.provider_name, error),
            );
        }
    };

    match endpoint.apply_headers(client.get(url)).send().await {
        Ok(response) => {
            let metadata = metadata_from_response(
                &endpoint.provider_name,
                ProviderStreamTransport::SseHttp,
                &response,
            );
            if response.status().is_success() || response.status().as_u16() == 404 {
                ProviderProbeReport {
                    provider_id: provider_id.to_string(),
                    transport: ProviderStreamTransport::SseHttp,
                    status: ProviderProbeStatus::Ok,
                    error_kind: None,
                    message: "provider endpoint is reachable".to_string(),
                    base_url_host,
                    request_id: metadata.request_id.clone(),
                    metadata: Some(metadata),
                }
            } else {
                let error = provider_error_from_response(&endpoint.provider_name, response).await;
                probe_error_report(
                    provider_id,
                    ProviderStreamTransport::SseHttp,
                    base_url_host,
                    error,
                )
            }
        }
        Err(error) => probe_error_report(
            provider_id,
            ProviderStreamTransport::SseHttp,
            base_url_host,
            ProviderError::transport(&endpoint.provider_name, error),
        ),
    }
}

pub async fn probe_websocket(
    endpoint: &ProviderEndpoint,
    provider_id: &str,
) -> ProviderProbeReport {
    let base_url_host = crate::api::providers::base_url_host(&endpoint.base_url);
    let Some(path) = endpoint.websocket_probe_path.as_deref() else {
        return ProviderProbeReport {
            provider_id: provider_id.to_string(),
            transport: ProviderStreamTransport::WebSocket,
            status: ProviderProbeStatus::Unsupported,
            error_kind: None,
            message: "provider does not advertise a WebSocket probe endpoint".to_string(),
            base_url_host,
            request_id: None,
            metadata: None,
        };
    };

    match endpoint.websocket_url_for_path(path) {
        Ok(url) => {
            let request = match url.as_str().into_client_request() {
                Ok(mut request) => {
                    for (name, value) in endpoint.headers.iter() {
                        request.headers_mut().insert(name, value.clone());
                    }
                    request
                }
                Err(error) => {
                    return ProviderProbeReport {
                        provider_id: provider_id.to_string(),
                        transport: ProviderStreamTransport::WebSocket,
                        status: ProviderProbeStatus::Error,
                        error_kind: Some(ProviderErrorKind::InvalidRequest),
                        message: error.to_string(),
                        base_url_host,
                        request_id: None,
                        metadata: None,
                    };
                }
            };

            match tokio::time::timeout(
                endpoint.request_timeout,
                tokio_tungstenite::connect_async(request),
            )
            .await
            {
                Ok(Ok((mut websocket, response))) => {
                    let _ = websocket.close(None).await;
                    let metadata = ProviderResponseMetadata {
                        provider: endpoint.provider_name.clone(),
                        transport: ProviderStreamTransport::WebSocket,
                        status: Some(response.status().as_u16()),
                        request_id: request_id_from_headers(response.headers()),
                        headers: safe_response_headers(response.headers()),
                    };
                    ProviderProbeReport {
                        provider_id: provider_id.to_string(),
                        transport: ProviderStreamTransport::WebSocket,
                        status: ProviderProbeStatus::Ok,
                        error_kind: None,
                        message: format!(
                            "WebSocket endpoint accepted handshake: {}",
                            redact_url_query(&url)
                        ),
                        base_url_host,
                        request_id: metadata.request_id.clone(),
                        metadata: Some(metadata),
                    }
                }
                Ok(Err(error)) => probe_error_report(
                    provider_id,
                    ProviderStreamTransport::WebSocket,
                    base_url_host,
                    ProviderError::transport(&endpoint.provider_name, error),
                ),
                Err(error) => probe_error_report(
                    provider_id,
                    ProviderStreamTransport::WebSocket,
                    base_url_host,
                    ProviderError::transport(&endpoint.provider_name, error),
                ),
            }
        }
        Err(error) => ProviderProbeReport {
            provider_id: provider_id.to_string(),
            transport: ProviderStreamTransport::WebSocket,
            status: ProviderProbeStatus::Error,
            error_kind: Some(ProviderErrorKind::InvalidRequest),
            message: error.to_string(),
            base_url_host,
            request_id: None,
            metadata: None,
        },
    }
}

fn probe_error_report(
    provider_id: &str,
    transport: ProviderStreamTransport,
    base_url_host: Option<String>,
    error: ProviderError,
) -> ProviderProbeReport {
    let status = match error.kind {
        ProviderErrorKind::AuthenticationFailed => ProviderProbeStatus::AuthFailed,
        ProviderErrorKind::RateLimited | ProviderErrorKind::QuotaExceeded => {
            ProviderProbeStatus::QuotaOrRateLimited
        }
        ProviderErrorKind::RetryableTransport => ProviderProbeStatus::NetworkError,
        _ => ProviderProbeStatus::Error,
    };
    ProviderProbeReport {
        provider_id: provider_id.to_string(),
        transport,
        status,
        error_kind: Some(error.kind),
        message: error.message,
        base_url_host,
        request_id: error.request_id,
        metadata: None,
    }
}

pub fn request_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("request-id")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

pub fn retry_after_ms(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000))
}

fn safe_response_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    const SAFE: &[&str] = &[
        "request-id",
        "x-request-id",
        "retry-after",
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ];
    SAFE.iter()
        .filter_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .map(|value| ((*name).to_string(), value.to_string()))
        })
        .collect()
}

fn redact_url_query(url: &Url) -> String {
    let mut redacted = url.clone();
    if redacted.query().is_some() {
        redacted.set_query(Some("redacted=1"));
    }
    redacted.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failed_stream_retryability_uses_status_and_safe_error_type() {
        let mut server_error = ProviderStreamFailure::new(
            ProviderStreamFailureCategory::ProviderFailed,
            "openai-codex",
            "temporary provider failure",
        );
        server_error.error_type = Some("server_error".to_string());
        assert!(server_error.is_retryable());

        let mut invalid = ProviderStreamFailure::new(
            ProviderStreamFailureCategory::ProviderFailed,
            "openai-codex",
            "invalid request",
        );
        invalid.status = Some(400);
        assert!(!invalid.is_retryable());
    }

    #[test]
    fn classifies_provider_error_fixtures() {
        assert_eq!(
            ProviderErrorKind::classify(
                Some(400),
                Some("invalid_request_error"),
                "prompt is too long"
            ),
            ProviderErrorKind::ContextWindowExceeded
        );
        assert_eq!(
            ProviderErrorKind::classify(Some(429), Some("rate_limit_error"), "too many requests"),
            ProviderErrorKind::RateLimited
        );
        assert_eq!(
            ProviderErrorKind::classify(
                Some(402),
                Some("insufficient_quota"),
                "billing quota exceeded"
            ),
            ProviderErrorKind::QuotaExceeded
        );
        assert_eq!(
            ProviderErrorKind::classify(
                Some(400),
                Some("content_filter"),
                "blocked by safety policy"
            ),
            ProviderErrorKind::PolicyBlocked
        );
        assert_eq!(
            ProviderErrorKind::classify(Some(529), Some("overloaded_error"), "overloaded"),
            ProviderErrorKind::ServerOverloaded
        );
        assert_eq!(
            ProviderErrorKind::classify(None, None, "failed to send HTTP request: dns error"),
            ProviderErrorKind::RetryableTransport
        );
        assert_eq!(
            ProviderErrorKind::classify(Some(401), None, "unauthorized"),
            ProviderErrorKind::AuthenticationFailed
        );
    }

    #[test]
    fn endpoint_builds_http_and_websocket_urls_with_query_params() {
        let endpoint =
            ProviderEndpoint::google("https://generativelanguage.googleapis.com/v1beta", "secret");
        let url = endpoint
            .url_for_path("/models/gemini:streamGenerateContent")
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini:streamGenerateContent?key=secret"
        );

        let mut endpoint =
            ProviderEndpoint::new("demo", "https://example.test/api", HeaderMap::new());
        endpoint.websocket_probe_path = Some("/realtime".to_string());
        let ws = endpoint.websocket_url_for_path("/realtime").unwrap();
        assert_eq!(ws.as_str(), "wss://example.test/api/realtime");
    }
}
