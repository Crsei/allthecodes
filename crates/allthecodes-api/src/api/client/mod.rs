//! API client 鈥?creates provider-specific HTTP clients and drives the
//! Anthropic Messages API (streaming + non-streaming).
use std::future::Future;
use std::pin::Pin;
use std::sync::Once;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::Stream;
use serde_json::Value;

use crate::api::retry::{categorize_stream_start_error, retry_delay, RetryConfig};
use allthecodes_types::message::{AssistantMessage, StreamEvent};

// Re-export siblings for convenience within this module's tests.
use crate::api::providers::{AnthropicEndpointKind, ProviderInfo, ProviderProtocol};
use crate::api::streaming::StreamAccumulator;

mod stream;
#[cfg(test)]
mod tests;

pub(crate) use stream::parse_sse_byte_stream;
#[cfg(test)]
use stream::parse_sse_text;

pub const OPENAI_CODEX_PROVIDER_NAME: &str = "openai-codex";
pub const OPENAI_PROVIDER_NAME: &str = "openai";
pub const OPENAI_CODEX_TOKEN_ENV: &str = "OPENAI_CODEX_AUTH_TOKEN";
pub const OPENAI_CODEX_BASE_URL_ENV: &str = "OPENAI_CODEX_BASE_URL";
pub const OPENAI_CODEX_MODEL_ENV: &str = "OPENAI_CODEX_MODEL";
pub const ANTHROPIC_DEFAULT_SOTA_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_SOTA_MODEL";
pub const ANTHROPIC_DEFAULT_MOTA_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_MOTA_MODEL";
pub const ANTHROPIC_DEFAULT_FOTA_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_FOTA_MODEL";
pub const ANTHROPIC_DEFAULT_OPUS_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_OPUS_MODEL";
pub const ANTHROPIC_DEFAULT_SONNET_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_SONNET_MODEL";
pub const ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV: &str = "ANTHROPIC_DEFAULT_HAIKU_MODEL";

const ANTHROPIC_OFFICIAL_SOTA_MODEL: &str = "claude-opus-4-7";
const ANTHROPIC_OFFICIAL_MOTA_MODEL: &str = "claude-sonnet-4-6";
const ANTHROPIC_OFFICIAL_FOTA_MODEL: &str = "claude-haiku-4-5-20251001";
const ANTHROPIC_DEFAULT_MODEL_ALIAS: &str = "MOTA";

static ANTHROPIC_LEGACY_MODEL_ENV_WARNING: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptCacheTtl {
    #[serde(rename = "1h")]
    OneHour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptCacheScope {
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<PromptCacheTtl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<PromptCacheScope>,
}

impl CacheControl {
    pub fn ephemeral() -> Self {
        Self {
            kind: "ephemeral".to_string(),
            ttl: None,
            scope: None,
        }
    }

    pub fn with_ttl(mut self, ttl: PromptCacheTtl) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn with_scope(mut self, scope: PromptCacheScope) -> Self {
        self.scope = Some(scope);
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptCacheCapability {
    pub explicit_markers: bool,
    pub ttl_1h: bool,
    pub global_scope: bool,
    pub direct_official_anthropic: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptCachePolicy {
    pub enabled: bool,
    pub ttl_1h: bool,
    pub global_scope: bool,
}

impl PromptCachePolicy {
    pub fn from_env(capability: PromptCacheCapability) -> Self {
        if !capability.explicit_markers {
            return Self::default();
        }
        Self {
            enabled: true,
            ttl_1h: capability.ttl_1h && is_env_value("ALLTHECODES_PROMPT_CACHE_TTL", "1h"),
            global_scope: capability.global_scope
                && capability.direct_official_anthropic
                && is_env_truthy("ALLTHECODES_PROMPT_CACHE_GLOBAL"),
        }
    }

    pub fn cache_control(self) -> CacheControl {
        let mut cache = CacheControl::ephemeral();
        if self.ttl_1h {
            cache = cache.with_ttl(PromptCacheTtl::OneHour);
        }
        if self.global_scope {
            cache = cache.with_scope(PromptCacheScope::Global);
        }
        cache
    }
}

pub fn prompt_cache_marker_value(policy: PromptCachePolicy) -> Value {
    serde_json::to_value(policy.cache_control()).unwrap_or_else(|_| {
        serde_json::json!({
            "type": "ephemeral"
        })
    })
}

fn is_env_value(name: &str, expected: &str) -> bool {
    std::env::var(name)
        .map(|value| value.trim().eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

pub(crate) fn build_openai_compat_url(base_url: &str, provider_name: &str) -> String {
    if provider_name.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_NAME) {
        let base_url = base_url.trim_end_matches('/');
        let endpoint = if base_url.ends_with("/responses") {
            ""
        } else if base_url.ends_with("/codex") {
            "/responses"
        } else {
            "/codex/responses"
        };
        return format!("{base_url}{endpoint}");
    }

    let endpoint = "/chat/completions";
    format!("{}{}", base_url.trim_end_matches('/'), endpoint)
}

pub(crate) fn is_openai_codex_provider(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_NAME)
}

/// API provider enum 鈥?determines wire protocol and auth method.
#[derive(Debug, Clone)]
pub enum ApiProvider {
    /// Direct Anthropic API (native Messages API)
    Anthropic {
        auth: AnthropicAuth,
        base_url: Option<String>,
        endpoint_kind: AnthropicEndpointKind,
    },
    /// Azure Foundry (Anthropic-compatible)
    Azure { endpoint: String, api_key: String },
    /// OpenAI-compatible provider (OpenAI, DeepSeek, Groq, Qwen, etc.)
    OpenAiCompat {
        name: String,
        api_key: String,
        base_url: String,
        default_model: String,
    },
    /// Google Gemini (streamGenerateContent API)
    Google { api_key: String, base_url: String },
    /// AWS Bedrock 鈥?Claude via AWS-managed endpoints.
    ///
    /// `base_url_override` is read from `ANTHROPIC_BEDROCK_BASE_URL` when set.
    Bedrock {
        region: String,
        auth: crate::api::bedrock::BedrockAuth,
        base_url_override: Option<String>,
    },
    /// GCP Vertex AI 鈥?Claude via Google-managed endpoints.
    Vertex {
        project_id: String,
        region: String,
        access_token: crate::api::vertex::VertexAccessToken,
    },
}

/// Authentication method for Anthropic-format native requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicAuth {
    ApiKey(String),
    BearerToken(String),
}

impl AnthropicAuth {
    fn secret(&self) -> &str {
        match self {
            Self::ApiKey(value) | Self::BearerToken(value) => value,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::ApiKey(_) => "Anthropic API key",
            Self::BearerToken(_) => "Anthropic bearer token",
        }
    }

    fn insert_auth_header(&self, headers: &mut reqwest::header::HeaderMap) -> Result<()> {
        use reqwest::header::HeaderValue;

        match self {
            Self::ApiKey(value) => {
                let value = HeaderValue::from_str(value).with_context(|| {
                    format!("{} is not a valid HTTP header value", self.label())
                })?;
                headers.insert("x-api-key", value);
            }
            Self::BearerToken(value) => {
                let bearer = format!("Bearer {value}");
                let value = HeaderValue::from_str(&bearer).with_context(|| {
                    format!("{} is not a valid HTTP header value", self.label())
                })?;
                headers.insert("Authorization", value);
            }
        }
        Ok(())
    }
}

pub(crate) fn build_anthropic_headers(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
) -> Result<reqwest::header::HeaderMap> {
    build_anthropic_headers_with_cache_betas(
        auth,
        include_token_counting_beta,
        true,
        false,
        false,
        false,
        true,
    )
}

pub(crate) fn build_anthropic_headers_for_body(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    body: &Value,
) -> Result<reqwest::header::HeaderMap> {
    build_anthropic_headers_for_body_with_beta_policy(auth, include_token_counting_beta, body, true)
}

pub(crate) fn build_anthropic_headers_for_body_with_beta_policy(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    body: &Value,
    include_anthropic_beta_header: bool,
) -> Result<reqwest::header::HeaderMap> {
    build_anthropic_headers_with_cache_betas(
        auth,
        include_token_counting_beta,
        body_contains_key(body, "cache_control"),
        body_contains_cache_attr(body, "ttl"),
        body_contains_cache_attr(body, "scope"),
        body_contains_output_effort(body),
        include_anthropic_beta_header,
    )
}

fn build_anthropic_headers_with_cache_betas(
    auth: &AnthropicAuth,
    include_token_counting_beta: bool,
    include_prompt_cache_beta: bool,
    include_ttl_beta: bool,
    include_global_scope_beta: bool,
    include_effort_beta: bool,
    include_anthropic_beta_header: bool,
) -> Result<reqwest::header::HeaderMap> {
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&allthecodes_config::user_agent::api_user_agent())
            .context("failed to build User-Agent header")?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    if include_anthropic_beta_header {
        let mut betas = vec!["interleaved-thinking-2025-05-14"];
        if include_prompt_cache_beta {
            betas.push("prompt-caching-2024-07-16");
        }
        if include_ttl_beta {
            betas.push("extended-cache-ttl-2025-04-11");
        }
        if include_global_scope_beta {
            betas.push("prompt-caching-scope-2026-01-05");
        }
        if include_effort_beta {
            betas.push(allthecodes_config::constants::api::EFFORT_BETA);
        }
        if include_token_counting_beta {
            betas.push("token-counting-2024-11-01");
        }
        let beta = HeaderValue::from_str(&betas.join(","))
            .context("failed to build anthropic-beta header")?;
        headers.insert("anthropic-beta", beta);
    }
    auth.insert_auth_header(&mut headers)?;
    Ok(headers)
}

fn body_contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.contains_key(key) || map.values().any(|v| body_contains_key(v, key))
        }
        Value::Array(values) => values.iter().any(|v| body_contains_key(v, key)),
        _ => false,
    }
}

fn body_contains_output_effort(value: &Value) -> bool {
    value
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .is_some()
}

fn body_contains_cache_attr(value: &Value, attr: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("cache_control")
                .and_then(Value::as_object)
                .and_then(|cache| cache.get(attr))
                .is_some()
                || map.values().any(|v| body_contains_cache_attr(v, attr))
        }
        Value::Array(values) => values.iter().any(|v| body_contains_cache_attr(v, attr)),
        _ => false,
    }
}

pub(crate) fn strip_anthropic_cache_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("cache_control");
            map.remove("cache_reference");
            map.remove("cache_edits");
            for value in map.values_mut() {
                strip_anthropic_cache_fields(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_anthropic_cache_fields(value);
            }
        }
        _ => {}
    }
}

pub(crate) fn strip_anthropic_compatible_only_fields(value: &mut Value) {
    strip_anthropic_cache_fields(value);
    if let Value::Object(map) = value {
        map.remove("thinking");
        map.remove("output_config");
        map.remove("context_management");
    }
    strip_anthropic_thinking_blocks(value);
}

fn strip_anthropic_thinking_blocks(value: &mut Value) {
    match value {
        Value::Array(values) => {
            values.retain(|item| {
                item.get("type")
                    .and_then(Value::as_str)
                    .map(|kind| kind != "thinking" && kind != "redacted_thinking")
                    .unwrap_or(true)
            });
            for value in values {
                strip_anthropic_thinking_blocks(value);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                strip_anthropic_thinking_blocks(value);
            }
        }
        _ => {}
    }
}

pub(crate) fn apply_prompt_cache_policy_to_body(
    body: &mut Value,
    capability: PromptCacheCapability,
) {
    let policy = PromptCachePolicy::from_env(capability);
    if !policy.enabled {
        return;
    }
    let cache_control = prompt_cache_marker_value(policy);
    replace_cache_control_markers(body, &cache_control);
}

fn replace_cache_control_markers(value: &mut Value, cache_control: &Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("cache_control") {
                map.insert("cache_control".to_string(), cache_control.clone());
            }
            for value in map.values_mut() {
                replace_cache_control_markers(value, cache_control);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_cache_control_markers(value, cache_control);
            }
        }
        _ => {}
    }
}

pub(crate) fn is_official_anthropic_base_url(base_url: &str) -> bool {
    crate::api::providers::base_url_host(base_url)
        .map(|host| host.eq_ignore_ascii_case("api.anthropic.com"))
        .unwrap_or(false)
}

fn extend_header_string_map(
    map: &mut std::collections::HashMap<String, String>,
    headers: &reqwest::header::HeaderMap,
) {
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            let name = if name == reqwest::header::AUTHORIZATION {
                "Authorization"
            } else {
                name.as_str()
            };
            map.insert(name.to_string(), value.to_string());
        }
    }
}

impl ApiProvider {
    pub fn endpoint_kind(&self) -> Option<AnthropicEndpointKind> {
        match self {
            ApiProvider::Anthropic { endpoint_kind, .. } => Some(*endpoint_kind),
            ApiProvider::Azure { .. }
            | ApiProvider::Bedrock { .. }
            | ApiProvider::Vertex { .. } => Some(AnthropicEndpointKind::DirectAnthropic),
            ApiProvider::OpenAiCompat { .. } | ApiProvider::Google { .. } => None,
        }
    }

    pub fn base_url_host(&self) -> Option<String> {
        match self {
            ApiProvider::Anthropic { base_url, .. } => {
                let base_url = base_url.as_deref().unwrap_or("https://api.anthropic.com");
                crate::api::providers::base_url_host(base_url)
            }
            ApiProvider::Azure { endpoint, .. } => crate::api::providers::base_url_host(endpoint),
            ApiProvider::OpenAiCompat { base_url, .. } => {
                crate::api::providers::base_url_host(base_url)
            }
            ApiProvider::Google { base_url, .. } => crate::api::providers::base_url_host(base_url),
            ApiProvider::Bedrock {
                base_url_override, ..
            } => {
                let base_url = base_url_override
                    .as_deref()
                    .unwrap_or("https://bedrock-runtime.amazonaws.com");
                crate::api::providers::base_url_host(base_url)
            }
            ApiProvider::Vertex { region, .. } => {
                Some(format!("{region}-aiplatform.googleapis.com"))
            }
        }
    }

    pub fn langfuse_provider_name(&self) -> &str {
        match self {
            ApiProvider::Anthropic { .. } => "anthropic",
            ApiProvider::Azure { .. } => "azure",
            ApiProvider::OpenAiCompat { name, .. } => {
                if name.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_NAME)
                    || name.eq_ignore_ascii_case("openai")
                {
                    "openai"
                } else {
                    name.as_str()
                }
            }
            ApiProvider::Google { .. } => "google",
            ApiProvider::Bedrock { .. } => "bedrock",
            ApiProvider::Vertex { .. } => "vertex",
        }
    }

    pub fn capabilities(&self) -> crate::api::providers::ProviderCapabilities {
        match self {
            ApiProvider::Anthropic { endpoint_kind, .. } => {
                if *endpoint_kind == AnthropicEndpointKind::CompatibleAnthropic {
                    crate::api::providers::compatible_anthropic_capabilities()
                } else {
                    crate::api::providers::capabilities_for_provider_name("anthropic")
                        .expect("anthropic capability matrix entry must exist")
                }
            }
            ApiProvider::Azure { .. } => {
                crate::api::providers::capabilities_for_provider_name("azure")
                    .expect("azure capability matrix entry must exist")
            }
            ApiProvider::OpenAiCompat { name, .. } => {
                crate::api::providers::capabilities_for_provider_name(name).unwrap_or_else(|| {
                    let info = crate::api::providers::get_provider("openai")
                        .expect("openai capability matrix entry must exist");
                    crate::api::providers::capabilities_for_provider_info(info)
                })
            }
            ApiProvider::Google { .. } => {
                crate::api::providers::capabilities_for_provider_name("google")
                    .expect("google capability matrix entry must exist")
            }
            ApiProvider::Bedrock { .. } => {
                crate::api::providers::capabilities_for_provider_name("bedrock")
                    .expect("bedrock capability matrix entry must exist")
            }
            ApiProvider::Vertex { .. } => {
                crate::api::providers::capabilities_for_provider_name("vertex")
                    .expect("vertex capability matrix entry must exist")
            }
        }
    }
}

/// Request body for the Messages API
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub system: Option<Vec<Value>>,
    pub max_tokens: usize,
    pub tools: Option<Vec<Value>>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    /// Anthropic `output_config` for controlling reasoning effort.
    /// Used as an alternative to `thinking` on newer Anthropic models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Optional Responses API reasoning effort. Honored by the openai-codex
    /// provider and omitted for Anthropic-compatible serialization.
    #[serde(skip_serializing)]
    pub reasoning_effort: Option<String>,
    /// Optional advisor model id (issue #33). Carried through the request
    /// pipeline only for providers that advertise advisor support
    /// (see [`provider_supports_advisor`]). Serialized as `advisor_model`;
    /// omitted when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisor_model: Option<String>,
}

/// Provider-level token count returned by an exact count endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactTokenCount {
    pub input_tokens: u64,
    pub provider: String,
}

pub(crate) fn build_anthropic_count_tokens_body(request: &MessagesRequest) -> Value {
    let mut body = serde_json::to_value(request).unwrap_or_else(|_| serde_json::json!({}));
    if let Value::Object(map) = &mut body {
        map.remove("stream");
        map.remove("max_tokens");
        map.remove("advisor_model");
    }
    body
}

/// Return `true` when the given provider supports the advisor-model field.
///
/// Only the Anthropic Messages API currently recognizes `advisor_model`.
/// For Bedrock/Vertex (which ultimately reach the same Anthropic shape) we
/// also pass it through; OpenAI-compatible and Google providers don't have
/// the field in their native schema, so we drop it there and the `/advisor`
/// command surfaces an "inactive" message.
pub fn provider_supports_advisor(provider: &ApiProvider) -> bool {
    matches!(
        provider,
        ApiProvider::Anthropic {
            endpoint_kind: AnthropicEndpointKind::DirectAnthropic,
            ..
        } | ApiProvider::Azure { .. }
            | ApiProvider::Bedrock { .. }
            | ApiProvider::Vertex { .. }
    )
}

fn anthropic_base_url_from_env() -> Option<String> {
    std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy)]
struct AnthropicModelAlias {
    name: &'static str,
    default_env: &'static str,
    legacy_env: &'static str,
    official_default: &'static str,
}

impl AnthropicModelAlias {
    fn for_name(model: &str) -> Option<Self> {
        let trimmed = model.trim();
        if trimmed.eq_ignore_ascii_case("SOTA") {
            Some(Self {
                name: "SOTA",
                default_env: ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_SOTA_MODEL,
            })
        } else if trimmed.eq_ignore_ascii_case("MOTA") {
            Some(Self {
                name: "MOTA",
                default_env: ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_MOTA_MODEL,
            })
        } else if trimmed.eq_ignore_ascii_case("FOTA") {
            Some(Self {
                name: "FOTA",
                default_env: ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_FOTA_MODEL,
            })
        } else {
            None
        }
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn selected_api_provider_from_settings() -> Result<Option<String>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let loaded = allthecodes_config::settings::load_effective(&cwd)?;
    let Some(raw) = loaded.effective.api_provider else {
        return Ok(None);
    };
    let Some(provider) = allthecodes_config::settings::normalize_api_provider(&raw) else {
        bail!(
            "Unknown apiProvider `{}`. Known providers: {}.",
            raw,
            allthecodes_config::settings::VALID_API_PROVIDERS.join(", ")
        );
    };
    Ok(Some(provider.to_string()))
}

fn warn_legacy_anthropic_model_env_once(legacy_env: &str, default_env: &str) {
    ANTHROPIC_LEGACY_MODEL_ENV_WARNING.call_once(|| {
        tracing::warn!(
            legacy_env,
            default_env,
            "legacy Anthropic model fallback env var is deprecated; use the SOTA/MOTA/FOTA env var instead"
        );
    });
}

fn anthropic_model_env_for_alias(alias: AnthropicModelAlias) -> Option<String> {
    if let Some(model) = non_empty_env(alias.default_env) {
        return Some(model);
    }
    let model = non_empty_env(alias.legacy_env)?;
    warn_legacy_anthropic_model_env_once(alias.legacy_env, alias.default_env);
    Some(model)
}

fn resolve_anthropic_model_alias(
    model: &str,
    endpoint_kind: AnthropicEndpointKind,
) -> Result<String> {
    let trimmed = model.trim();
    let Some(alias) = AnthropicModelAlias::for_name(trimmed) else {
        return Ok(trimmed.to_string());
    };

    if let Some(model) = anthropic_model_env_for_alias(alias) {
        return Ok(model);
    }

    match endpoint_kind {
        AnthropicEndpointKind::DirectAnthropic => Ok(alias.official_default.to_string()),
        AnthropicEndpointKind::CompatibleAnthropic => bail!(
            "Anthropic-compatible provider cannot resolve model alias `{}` without {}. Set ANTHROPIC_MODEL to an explicit provider model ID, or set {} for this compatible endpoint.",
            alias.name,
            alias.default_env,
            alias.default_env
        ),
    }
}

fn resolve_anthropic_default_model(endpoint_kind: AnthropicEndpointKind) -> Result<String> {
    let selected = non_empty_env("ANTHROPIC_MODEL")
        .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL_ALIAS.to_string());
    resolve_anthropic_model_alias(&selected, endpoint_kind)
}

fn resolve_model_for_request(provider: &ApiProvider, model: &str) -> Result<String> {
    match provider {
        ApiProvider::Anthropic { endpoint_kind, .. } => {
            resolve_anthropic_model_alias(model, *endpoint_kind)
        }
        ApiProvider::Bedrock { .. } | ApiProvider::Vertex { .. } => {
            resolve_anthropic_model_alias(model, AnthropicEndpointKind::DirectAnthropic)
        }
        _ => Ok(model.trim().to_string()),
    }
}

fn resolve_request_model_for_provider(
    provider: &ApiProvider,
    mut request: MessagesRequest,
) -> Result<MessagesRequest> {
    request.model = resolve_model_for_request(provider, &request.model)?;
    Ok(request)
}

/// API client configuration
#[derive(Debug, Clone)]
pub struct ApiClientConfig {
    pub provider: ApiProvider,
    pub default_model: String,
    pub max_retries: usize,
    pub timeout_secs: u64,
}

/// The API client 鈥?uses reqwest under the hood.
pub struct ApiClient {
    config: ApiClientConfig,
    http: reqwest::Client,
    stream_provider: Box<dyn crate::api::stream_provider::StreamProvider>,
}

/// Build the appropriate `StreamProvider` from an `ApiProvider`.
fn make_stream_provider(
    provider: &ApiProvider,
) -> Box<dyn crate::api::stream_provider::StreamProvider> {
    use crate::api::stream_provider::*;
    match provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            ..
        } => Box::new(OpenAiCompatStreamProvider {
            name: name.clone(),
            api_key: api_key.clone(),
            base_url: base_url.clone(),
        }),
        ApiProvider::Google { api_key, base_url } => Box::new(GoogleStreamProvider {
            api_key: api_key.clone(),
            base_url: base_url.clone(),
        }),
        ApiProvider::Anthropic { auth, base_url, .. } => Box::new(AnthropicStreamProvider {
            auth: auth.clone(),
            base_url: base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        }),
        ApiProvider::Azure { api_key, endpoint } => Box::new(AnthropicStreamProvider {
            auth: AnthropicAuth::ApiKey(api_key.clone()),
            base_url: endpoint.clone(),
        }),
        ApiProvider::Bedrock {
            region,
            auth,
            base_url_override,
        } => Box::new(crate::api::bedrock::BedrockStreamProvider {
            region: region.clone(),
            auth: auth.clone(),
            base_url_override: base_url_override.clone(),
        }),
        ApiProvider::Vertex {
            project_id,
            region,
            access_token,
        } => Box::new(crate::api::vertex::VertexStreamProvider {
            region: region.clone(),
            project_id: project_id.clone(),
            access_token: access_token.clone(),
        }),
    }
}

fn require_non_empty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn validate_base_url(value: &str, label: &str) -> Result<()> {
    require_non_empty(value, label)?;
    let parsed = url::Url::parse(value).with_context(|| format!("{label} must be a URL"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => bail!("{label} must use http or https, got `{scheme}`"),
    }
}

fn validate_provider_config(provider: &ApiProvider) -> Result<()> {
    let capabilities = provider.capabilities();
    if !capabilities.is_usable() {
        let reason = capabilities
            .status
            .reason()
            .unwrap_or("provider is unsupported");
        bail!(
            "API provider `{}` is not usable: {reason}",
            capabilities.name
        );
    }

    match provider {
        ApiProvider::Anthropic { auth, base_url, .. } => {
            require_non_empty(auth.secret(), auth.label())?;
            let mut headers = reqwest::header::HeaderMap::new();
            auth.insert_auth_header(&mut headers)?;
            if let Some(base_url) = base_url {
                validate_base_url(base_url, "ANTHROPIC_BASE_URL")?;
            }
        }
        ApiProvider::Azure { endpoint, api_key } => {
            require_non_empty(api_key, "AZURE_API_KEY")?;
            validate_base_url(endpoint, "AZURE_BASE_URL")?;
        }
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            require_non_empty(name, "provider name")?;
            require_non_empty(api_key, "provider API key")?;
            validate_base_url(base_url, "provider base URL")?;
            require_non_empty(default_model, "provider default model")?;
        }
        ApiProvider::Google { api_key, base_url } => {
            require_non_empty(api_key, "GOOGLE_API_KEY")?;
            validate_base_url(base_url, "Google base URL")?;
        }
        ApiProvider::Bedrock {
            region,
            auth,
            base_url_override,
        } => {
            require_non_empty(region, "AWS_REGION or AWS_DEFAULT_REGION")?;
            match auth {
                crate::api::bedrock::BedrockAuth::BearerToken(token) => {
                    require_non_empty(token, "AWS_BEARER_TOKEN_BEDROCK")?;
                }
                crate::api::bedrock::BedrockAuth::AwsCredentials(creds) => {
                    require_non_empty(&creds.access_key_id, "AWS_ACCESS_KEY_ID")?;
                    require_non_empty(&creds.secret_access_key, "AWS_SECRET_ACCESS_KEY")?;
                }
            }
            if let Some(base_url) = base_url_override {
                validate_base_url(base_url, "ANTHROPIC_BEDROCK_BASE_URL")?;
            }
        }
        ApiProvider::Vertex {
            project_id,
            region,
            access_token,
        } => {
            require_non_empty(
                project_id,
                "ANTHROPIC_VERTEX_PROJECT_ID or GOOGLE_CLOUD_PROJECT",
            )?;
            require_non_empty(region, "CLOUD_ML_REGION")?;
            require_non_empty(&access_token.0, "Vertex OAuth access token")?;
        }
    }

    Ok(())
}

impl ApiClient {
    pub fn try_new(config: ApiClientConfig) -> Result<Self> {
        validate_provider_config(&config.provider)?;
        require_non_empty(&config.default_model, "default model")?;

        let stream_provider = make_stream_provider(&config.provider);
        Ok(Self {
            http: {
                let mut builder = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(config.timeout_secs))
                    .user_agent(allthecodes_config::user_agent::api_user_agent());

                // Honor HTTPS_PROXY/HTTP_PROXY/ALL_PROXY explicitly so the
                // client works under TUN/fake-ip DNS hijacking (e.g. Clash TUN)
                // where the system DNS resolves API hosts to private IPs and
                // direct TLS handshakes fail with "unexpected EOF".
                if let Ok(proxy_url) = std::env::var("HTTPS_PROXY")
                    .or_else(|_| std::env::var("https_proxy"))
                    .or_else(|_| std::env::var("HTTP_PROXY"))
                    .or_else(|_| std::env::var("http_proxy"))
                    .or_else(|_| std::env::var("ALL_PROXY"))
                {
                    if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                        tracing::info!(proxy = %proxy_url, "using explicit HTTP proxy");
                        builder = builder.proxy(proxy);
                    }
                }

                builder.build().unwrap_or_else(|_| reqwest::Client::new())
            },
            stream_provider,
            config,
        })
    }

    pub fn new(config: ApiClientConfig) -> Self {
        Self::try_new(config).expect("invalid API client configuration")
    }

    pub fn supports_exact_token_count(&self) -> bool {
        matches!(
            self.config.provider,
            ApiProvider::Anthropic { .. }
                | ApiProvider::Azure { .. }
                | ApiProvider::Google { .. }
                | ApiProvider::Bedrock { .. }
                | ApiProvider::Vertex { .. }
        )
    }

    /// Count input tokens with a provider endpoint when one is available.
    ///
    /// Unsupported providers return an error so callers can fall back to
    /// `cc-utils`' heuristic report without adding provider coupling there.
    pub async fn count_input_tokens_exact(
        &self,
        request: &MessagesRequest,
    ) -> Result<ExactTokenCount> {
        let resolved_request =
            resolve_request_model_for_provider(&self.config.provider, request.clone())?;
        let request = &resolved_request;
        match &self.config.provider {
            ApiProvider::Anthropic {
                auth,
                base_url,
                endpoint_kind,
            } => {
                let provider = if *endpoint_kind == AnthropicEndpointKind::CompatibleAnthropic {
                    "anthropic-compatible"
                } else {
                    "anthropic"
                };
                self.count_anthropic_input_tokens(
                    auth,
                    base_url.as_deref().unwrap_or("https://api.anthropic.com"),
                    request,
                    provider,
                )
                .await
            }
            ApiProvider::Azure { endpoint, api_key } => {
                let auth = AnthropicAuth::ApiKey(api_key.clone());
                self.count_anthropic_input_tokens(&auth, endpoint, request, "azure")
                    .await
            }
            ApiProvider::Google { api_key, base_url } => {
                let input_tokens = crate::api::google_provider::google_count_tokens(
                    &self.http, base_url, api_key, request,
                )
                .await?;
                Ok(ExactTokenCount {
                    input_tokens,
                    provider: "google".to_string(),
                })
            }
            ApiProvider::Bedrock {
                region,
                auth,
                base_url_override,
            } => {
                let input_tokens = crate::api::bedrock::bedrock_count_tokens(
                    &self.http,
                    region,
                    auth,
                    base_url_override.as_deref(),
                    request,
                )
                .await?;
                Ok(ExactTokenCount {
                    input_tokens,
                    provider: "bedrock".to_string(),
                })
            }
            ApiProvider::Vertex {
                project_id,
                region,
                access_token,
            } => {
                let input_tokens = crate::api::vertex::vertex_count_tokens(
                    &self.http,
                    project_id,
                    region,
                    access_token,
                    request,
                )
                .await?;
                Ok(ExactTokenCount {
                    input_tokens,
                    provider: "vertex".to_string(),
                })
            }
            provider => bail!(
                "provider `{}` does not support exact token counting",
                provider.langfuse_provider_name()
            ),
        }
    }

    pub async fn count_token_usage_exact(
        &self,
        request: &MessagesRequest,
    ) -> Result<allthecodes_utils::tokens::TokenUsageReport> {
        let count = self.count_input_tokens_exact(request).await?;
        Ok(allthecodes_utils::tokens::token_usage_report_from_count(
            count.input_tokens,
            &request.model,
            allthecodes_utils::tokens::TokenCountMethod::ProviderExact,
            Some(count.provider),
        ))
    }

    async fn count_anthropic_input_tokens(
        &self,
        auth: &AnthropicAuth,
        base_url: &str,
        request: &MessagesRequest,
        provider: &str,
    ) -> Result<ExactTokenCount> {
        #[derive(serde::Deserialize)]
        struct CountTokensResponse {
            input_tokens: u64,
        }

        let url = format!(
            "{}/v1/messages/count_tokens",
            base_url.trim_end_matches('/')
        );
        let mut body = build_anthropic_count_tokens_body(request);
        let direct_official_anthropic = is_official_anthropic_base_url(base_url);
        if direct_official_anthropic {
            apply_prompt_cache_policy_to_body(
                &mut body,
                PromptCacheCapability {
                    explicit_markers: true,
                    ttl_1h: true,
                    global_scope: true,
                    direct_official_anthropic: true,
                },
            );
        } else {
            strip_anthropic_compatible_only_fields(&mut body);
        }
        let headers = if direct_official_anthropic {
            build_anthropic_headers_for_body(auth, true, &body)?
        } else {
            build_anthropic_headers_for_body_with_beta_policy(auth, true, &body, false)?
        };
        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("failed to send Anthropic count_tokens request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            bail!(
                "Anthropic count_tokens error (HTTP {}): {}",
                status,
                error_body
            );
        }

        let parsed: CountTokensResponse = response
            .json()
            .await
            .context("failed to parse Anthropic count_tokens response")?;
        Ok(ExactTokenCount {
            input_tokens: parsed.input_tokens,
            provider: provider.to_string(),
        })
    }

    /// Build the messages endpoint URL based on provider.
    ///
    /// Only used for Anthropic-format providers (Anthropic, Azure).
    /// OpenAI-compat and Google providers build their URLs internally.
    pub fn build_url(&self) -> String {
        match &self.config.provider {
            ApiProvider::Anthropic { base_url, .. } => {
                let base = base_url.as_deref().unwrap_or("https://api.anthropic.com");
                let base = base.trim_end_matches('/');
                format!("{}/v1/messages", base)
            }
            ApiProvider::Azure { endpoint, .. } => {
                let endpoint = endpoint.trim_end_matches('/');
                format!("{}/v1/messages", endpoint)
            }
            ApiProvider::OpenAiCompat { name, base_url, .. } => {
                build_openai_compat_url(base_url, name)
            }
            ApiProvider::Google { base_url, .. } => base_url.clone(),
            ApiProvider::Bedrock {
                region,
                base_url_override,
                ..
            } => crate::api::bedrock::build_invoke_stream_url(
                region,
                &allthecodes_models::to_bedrock_model_id(&self.config.default_model),
                base_url_override.as_deref(),
            ),
            ApiProvider::Vertex {
                project_id, region, ..
            } => {
                let region = crate::api::vertex::resolve_region_for_model_with_default(
                    Some(&self.config.default_model),
                    region,
                );
                crate::api::vertex::build_stream_url(
                    &region,
                    project_id,
                    &allthecodes_models::to_vertex_model_id(&self.config.default_model),
                )
            }
        }
    }

    /// Construct an `ApiClient` from a `ProviderInfo` and API key.
    pub fn from_provider_info(info: &ProviderInfo, api_key: &str) -> Self {
        let provider = match info.protocol {
            ProviderProtocol::Anthropic => ApiProvider::Anthropic {
                auth: AnthropicAuth::ApiKey(api_key.to_string()),
                base_url: Some(info.base_url.to_string()),
                endpoint_kind: AnthropicEndpointKind::DirectAnthropic,
            },
            ProviderProtocol::OpenAiCompat => ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key: api_key.to_string(),
                base_url: info.base_url.to_string(),
                default_model: info.default_model.to_string(),
            },
            ProviderProtocol::Google => ApiProvider::Google {
                api_key: api_key.to_string(),
                base_url: info.base_url.to_string(),
            },
        };
        let default_model = if matches!(info.protocol, ProviderProtocol::Anthropic) {
            resolve_anthropic_default_model(AnthropicEndpointKind::DirectAnthropic)
                .expect("invalid Anthropic default model configuration")
        } else {
            info.default_model.to_string()
        };
        Self::new(ApiClientConfig {
            provider,
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
    }

    /// Auto-detect provider from environment variables and construct an `ApiClient`.
    ///
    /// Priority:
    /// 1. `ALLTHECODES_USE_FOUNDRY=1` -> fail early; Foundry has no adapter yet
    /// 2. `ALLTHECODES_USE_BEDROCK=1` -> AWS Bedrock (Claude)
    /// 3. `ALLTHECODES_USE_VERTEX=1`  -> GCP Vertex AI (Claude)
    /// 4. First of the registered API-key providers (Anthropic, Azure, OpenAI, ...)
    ///    that has its env var set.
    ///
    /// For Azure OpenAI, the base URL is read from `AZURE_BASE_URL` since it is
    /// deployment-specific (e.g. `https://<resource>.openai.azure.com/openai/v1/`).
    ///
    /// Returns `None` if no provider is configured.
    pub fn from_env_result() -> Result<Option<Self>> {
        // Env-flag cloud providers are checked BEFORE API-key providers,
        // matching claude-code-bun. Foundry is recognized but intentionally
        // unsupported until a request/auth adapter exists.
        if is_env_truthy("ALLTHECODES_USE_FOUNDRY") {
            let validation = crate::api::providers::validate_provider_name("azure-foundry");
            let reason = validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or(crate::api::providers::FOUNDRY_UNSUPPORTED_REASON);
            bail!("{reason}");
        }
        if is_env_truthy("ALLTHECODES_USE_BEDROCK") {
            return Self::from_bedrock_env_result().map(Some);
        }
        if is_env_truthy("ALLTHECODES_USE_VERTEX") {
            return Self::from_vertex_env_result().map(Some);
        }

        let Some(info) = crate::api::providers::detect_provider() else {
            return Ok(None);
        };
        let api_key = std::env::var(info.env_key)
            .with_context(|| format!("{} was detected but could not be read", info.env_key))?;

        // Azure OpenAI: override the placeholder base_url with AZURE_BASE_URL
        if info.name == "azure" {
            let base_url = std::env::var("AZURE_BASE_URL")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| info.base_url.to_string());
            let base_url = base_url.trim_end_matches('/').to_string();

            let provider = ApiProvider::OpenAiCompat {
                name: "azure".to_string(),
                api_key,
                base_url,
                default_model: info.default_model.to_string(),
            };
            return Self::try_new(ApiClientConfig {
                provider,
                default_model: info.default_model.to_string(),
                max_retries: 3,
                timeout_secs: 120,
            })
            .map(Some);
        }

        // OpenAI Codex: allow runtime base_url/model overrides.
        if info.name == OPENAI_CODEX_PROVIDER_NAME {
            let base_url = std::env::var(OPENAI_CODEX_BASE_URL_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| info.base_url.to_string())
                .trim_end_matches('/')
                .to_string();
            let default_model = std::env::var(OPENAI_CODEX_MODEL_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| info.default_model.to_string());

            let provider = ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key,
                base_url,
                default_model: default_model.clone(),
            };
            return Self::try_new(ApiClientConfig {
                provider,
                default_model,
                max_retries: 3,
                timeout_secs: 120,
            })
            .map(Some);
        }

        if info.name == "anthropic" {
            let base_url = anthropic_base_url_from_env();
            let endpoint_kind =
                crate::api::providers::anthropic_endpoint_kind_for_base_url(base_url.as_deref());
            let default_model = resolve_anthropic_default_model(endpoint_kind)?;
            return Self::try_new(ApiClientConfig {
                provider: ApiProvider::Anthropic {
                    auth: AnthropicAuth::ApiKey(api_key),
                    base_url,
                    endpoint_kind,
                },
                default_model,
                max_retries: 3,
                timeout_secs: 120,
            })
            .map(Some);
        }

        Ok(Some(Self::from_provider_info(info, &api_key)))
    }

    pub fn from_env() -> Option<Self> {
        match Self::from_env_result() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "API provider environment rejected");
                None
            }
        }
    }

    /// Construct an `ApiClient` for AWS Bedrock using environment variables.
    ///
    /// Honors (matching claude-code-bun):
    /// - `AWS_REGION` / `AWS_DEFAULT_REGION` 鈥?region selection
    /// - `AWS_BEARER_TOKEN_BEDROCK` 鈥?preferred auth (Bedrock API key)
    /// - `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` 鈥?SigV4
    /// - `ANTHROPIC_BEDROCK_BASE_URL` 鈥?override the default endpoint
    ///
    /// Returns `None` if neither auth mode is available.
    pub fn from_bedrock_env_result() -> Result<Self> {
        let auth = crate::api::bedrock::BedrockAuth::from_env().ok_or_else(|| {
            anyhow::anyhow!(
                "Bedrock provider was requested with ALLTHECODES_USE_BEDROCK, but no Bedrock auth was found. Set AWS_BEARER_TOKEN_BEDROCK or AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY."
            )
        })?;
        let region = crate::api::bedrock::resolve_region();
        let base_url_override = std::env::var("ANTHROPIC_BEDROCK_BASE_URL")
            .ok()
            .filter(|v| !v.is_empty());
        let default_model = std::env::var("ANTHROPIC_MODEL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL_ALIAS.to_string());
        let default_model =
            resolve_anthropic_model_alias(&default_model, AnthropicEndpointKind::DirectAnthropic)?;
        Self::try_new(ApiClientConfig {
            provider: ApiProvider::Bedrock {
                region,
                auth,
                base_url_override,
            },
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
    }

    /// Construct an `ApiClient` for GCP Vertex AI using environment variables.
    ///
    /// Honors:
    /// - `CLOUD_ML_REGION` 鈥?region (default: `us-east5`)
    /// - `ANTHROPIC_VERTEX_PROJECT_ID` / `GOOGLE_CLOUD_PROJECT` / `GCLOUD_PROJECT` 鈥?project ID
    /// - `ALLTHECODES_VERTEX_ACCESS_TOKEN` / `GOOGLE_OAUTH_ACCESS_TOKEN` 鈥?access token
    /// - `GOOGLE_APPLICATION_CREDENTIALS` service-account JSON
    ///   (falls back to `gcloud auth application-default print-access-token` subprocess)
    ///
    /// Returns `None` if project ID or access token can't be resolved.
    pub fn from_vertex_env_result() -> Result<Self> {
        let project_id = crate::api::vertex::resolve_project_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Vertex provider was requested with ALLTHECODES_USE_VERTEX, but no project id was found. Set ANTHROPIC_VERTEX_PROJECT_ID, GOOGLE_CLOUD_PROJECT, or GCLOUD_PROJECT."
            )
        })?;
        let region = crate::api::vertex::resolve_region();
        let access_token = crate::api::vertex::VertexAccessToken::from_env_or_gcloud().ok_or_else(|| {
            anyhow::anyhow!(
                "Vertex provider was requested with ALLTHECODES_USE_VERTEX, but no OAuth access token was found. Set ALLTHECODES_VERTEX_ACCESS_TOKEN, GOOGLE_OAUTH_ACCESS_TOKEN, or GOOGLE_APPLICATION_CREDENTIALS, or run `gcloud auth application-default login`."
            )
        })?;
        let default_model = std::env::var("ANTHROPIC_MODEL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL_ALIAS.to_string());
        let default_model =
            resolve_anthropic_model_alias(&default_model, AnthropicEndpointKind::DirectAnthropic)?;
        Self::try_new(ApiClientConfig {
            provider: ApiProvider::Vertex {
                project_id,
                region,
                access_token,
            },
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
    }

    /// Construct an `ApiClient` for the OpenAI Codex provider.
    ///
    /// Auth source (in priority order):
    /// - `OPENAI_CODEX_AUTH_TOKEN`
    /// - OAuth token saved by `/login 4`
    ///
    /// Optional:
    /// - `OPENAI_CODEX_BASE_URL` (default: https://chatgpt.com/backend-api)
    /// - `OPENAI_CODEX_MODEL` (default: gpt-5.4)
    pub fn from_codex_auth() -> Option<Self> {
        match Self::from_codex_auth_result() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "OpenAI Codex auth configuration rejected");
                None
            }
        }
    }

    pub fn from_codex_auth_result() -> Result<Option<Self>> {
        let Some(info) = crate::api::providers::get_provider(OPENAI_CODEX_PROVIDER_NAME) else {
            return Ok(None);
        };
        let Some(api_key) = allthecodes_auth::try_resolve_codex_auth_token()? else {
            return Ok(None);
        };

        let base_url = std::env::var(OPENAI_CODEX_BASE_URL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| info.base_url.to_string())
            .trim_end_matches('/')
            .to_string();
        let default_model = std::env::var(OPENAI_CODEX_MODEL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| info.default_model.to_string());

        Self::try_new(ApiClientConfig {
            provider: ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key,
                base_url,
                default_model: default_model.clone(),
            },
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
        .map(Some)
    }

    /// Construct an OpenAI-compatible client from the provider-scoped OpenAI
    /// Platform API key stored in allthecodes's keychain.
    pub fn from_openai_api_keychain_result() -> Result<Option<Self>> {
        let Some(info) = crate::api::providers::get_provider(OPENAI_PROVIDER_NAME) else {
            return Ok(None);
        };
        let Some(api_key) = allthecodes_auth::try_resolve_openai_api_key()? else {
            return Ok(None);
        };
        Self::try_new(ApiClientConfig {
            provider: ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key,
                base_url: info.base_url.to_string(),
                default_model: info.default_model.to_string(),
            },
            default_model: info.default_model.to_string(),
            max_retries: 3,
            timeout_secs: 120,
        })
        .map(Some)
    }

    /// Construct an `ApiClient` for a specific backend.
    ///
    /// - `codex` backend: force the OpenAI Codex auth path.
    /// - other backends: use the standard auth chain.
    pub fn from_backend_result(backend: Option<&str>) -> Result<Option<Self>> {
        if backend.is_some_and(is_codex_backend) {
            return Self::from_codex_auth_result();
        }
        Self::from_auth_result()
    }

    pub fn from_backend(backend: Option<&str>) -> Option<Self> {
        match Self::from_backend_result(backend) {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "API backend configuration rejected");
                None
            }
        }
    }

    /// Construct an `ApiClient` using the full auth resolution chain.
    ///
    /// Resolution order:
    /// 1. Explicit cloud provider env flags (Bedrock, Vertex, Foundry)
    /// 2. Persisted / active settings provider selection
    /// 3. Multi-provider environment variable detection (Anthropic, OpenAI, Google, etc.)
    /// 4. `ANTHROPIC_AUTH_TOKEN` environment variable
    /// 5. API key from system keychain
    ///
    /// Returns `None` if no authentication is available.
    pub fn from_auth_result() -> Result<Option<Self>> {
        // Env-flag cloud providers are explicit one-off selections and should
        // still fail fast before persisted provider selection is considered.
        if let Some(client) = Self::from_env_flag_provider_result()? {
            return Ok(Some(client));
        }

        // Honor the active settings provider before generic env detection.
        // This prevents unrelated inherited tokens, especially
        // OPENAI_CODEX_AUTH_TOKEN from the TypeScript/Codex install, from
        // silently routing a allthecodes Anthropic profile to openai-codex.
        if let Some(provider) = selected_api_provider_from_settings()? {
            return Self::from_selected_provider_result(&provider);
        }

        // Try multi-provider env detection when settings did not select a
        // provider explicitly.
        if let Some(client) = Self::from_env_result()? {
            return Ok(Some(client));
        }

        // Fall back to Anthropic auth resolution (keychain, external token, OAuth).
        Self::from_anthropic_auth_result()
    }

    fn from_env_flag_provider_result() -> Result<Option<Self>> {
        if is_env_truthy("ALLTHECODES_USE_FOUNDRY") {
            let validation = crate::api::providers::validate_provider_name("azure-foundry");
            let reason = validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or(crate::api::providers::FOUNDRY_UNSUPPORTED_REASON);
            bail!("{reason}");
        }
        if is_env_truthy("ALLTHECODES_USE_BEDROCK") {
            return Self::from_bedrock_env_result().map(Some);
        }
        if is_env_truthy("ALLTHECODES_USE_VERTEX") {
            return Self::from_vertex_env_result().map(Some);
        }
        Ok(None)
    }

    fn from_selected_provider_result(provider: &str) -> Result<Option<Self>> {
        match provider {
            allthecodes_config::settings::API_PROVIDER_OPENAI => {
                if let Some(info) = crate::api::providers::get_provider(OPENAI_PROVIDER_NAME) {
                    if let Ok(api_key) = std::env::var(info.env_key) {
                        if !api_key.trim().is_empty() {
                            return Ok(Some(Self::from_provider_info(info, &api_key)));
                        }
                    }
                }
                Self::from_openai_api_keychain_result()
            }
            allthecodes_config::settings::API_PROVIDER_OPENAI_CODEX => {
                Self::from_codex_auth_result()
            }
            allthecodes_config::settings::API_PROVIDER_ANTHROPIC => {
                Self::from_anthropic_auth_result()
            }
            provider => bail!("unsupported apiProvider `{provider}`"),
        }
    }

    fn from_anthropic_auth_result() -> Result<Option<Self>> {
        let auth = allthecodes_auth::try_resolve_auth()?;
        let auth = match auth {
            allthecodes_auth::AuthMethod::ApiKey(api_key) => AnthropicAuth::ApiKey(api_key),
            allthecodes_auth::AuthMethod::ExternalToken(token) => AnthropicAuth::BearerToken(token),
            allthecodes_auth::AuthMethod::OAuthToken { access_token, .. } => {
                AnthropicAuth::BearerToken(access_token)
            }
            allthecodes_auth::AuthMethod::None => return Ok(None),
        };
        let base_url = anthropic_base_url_from_env();
        let endpoint_kind =
            crate::api::providers::anthropic_endpoint_kind_for_base_url(base_url.as_deref());
        let default_model = resolve_anthropic_default_model(endpoint_kind)?;
        Self::try_new(ApiClientConfig {
            provider: ApiProvider::Anthropic {
                auth,
                base_url,
                endpoint_kind,
            },
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
        .map(Some)
    }

    pub fn from_auth() -> Option<Self> {
        match Self::from_auth_result() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "API auth configuration rejected");
                None
            }
        }
    }

    /// Build the required HTTP headers for Anthropic-format providers.
    pub fn build_headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Ok(value) = HeaderValue::from_str(&allthecodes_config::user_agent::api_user_agent())
        {
            headers.insert(USER_AGENT, value);
        }

        match &self.config.provider {
            ApiProvider::Anthropic {
                auth,
                endpoint_kind,
                ..
            } => {
                let provider_headers = if *endpoint_kind == AnthropicEndpointKind::DirectAnthropic {
                    build_anthropic_headers(auth, false)
                } else {
                    build_anthropic_headers_for_body_with_beta_policy(
                        auth,
                        false,
                        &Value::Null,
                        false,
                    )
                };
                match provider_headers {
                    Ok(provider_headers) => headers.extend(provider_headers),
                    Err(error) => tracing::warn!(%error, "failed to build Anthropic headers"),
                }
            }
            ApiProvider::Azure { api_key, .. } => {
                match build_anthropic_headers(&AnthropicAuth::ApiKey(api_key.clone()), false) {
                    Ok(provider_headers) => headers.extend(provider_headers),
                    Err(error) => tracing::warn!(%error, "failed to build Azure Anthropic headers"),
                }
            }
            ApiProvider::OpenAiCompat { api_key, .. } => {
                let bearer = format!("Bearer {}", api_key);
                if let Ok(val) = HeaderValue::from_str(&bearer) {
                    headers.insert("Authorization", val);
                }
            }
            ApiProvider::Google { .. } => {
                // Google uses API key in URL query param, no auth header needed
            }
            _ => {}
        }

        headers
    }

    /// Header accessor as a simple map (works without network feature, for tests).
    pub fn build_headers_map(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        map.insert("content-type".to_string(), "application/json".to_string());

        match &self.config.provider {
            ApiProvider::Anthropic {
                auth,
                endpoint_kind,
                ..
            } => {
                let headers = if *endpoint_kind == AnthropicEndpointKind::DirectAnthropic {
                    build_anthropic_headers(auth, false)
                } else {
                    build_anthropic_headers_for_body_with_beta_policy(
                        auth,
                        false,
                        &Value::Null,
                        false,
                    )
                };
                if let Ok(headers) = headers {
                    extend_header_string_map(&mut map, &headers);
                }
            }
            ApiProvider::Azure { api_key, .. } => {
                if let Ok(headers) =
                    build_anthropic_headers(&AnthropicAuth::ApiKey(api_key.clone()), false)
                {
                    extend_header_string_map(&mut map, &headers);
                }
            }
            ApiProvider::OpenAiCompat { api_key, .. } => {
                map.insert("Authorization".to_string(), format!("Bearer {}", api_key));
            }
            ApiProvider::Google { .. } => {}
            _ => {}
        }

        map
    }

    /// Send a messages request and return the response as a stream of events.
    ///
    /// Delegates to the provider-specific `StreamProvider` implementation
    /// (Anthropic, OpenAI-compat, or Google Gemini).
    pub async fn messages_stream(
        &self,
        request: MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let retry_config = RetryConfig {
            max_retries: self.config.max_retries,
            ..RetryConfig::default()
        };
        self.messages_stream_with_backoff(request, retry_config, tokio::time::sleep)
            .await
    }

    async fn messages_stream_with_backoff<SleepFn, SleepFuture>(
        &self,
        request: MessagesRequest,
        retry_config: RetryConfig,
        mut sleep: SleepFn,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>
    where
        SleepFn: FnMut(Duration) -> SleepFuture,
        SleepFuture: Future<Output = ()>,
    {
        let request = resolve_request_model_for_provider(&self.config.provider, request)?;
        let mut retry_attempt = 0;

        loop {
            match self.stream_provider.stream(&self.http, &request).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    let error_message = error.to_string();
                    let category = categorize_stream_start_error(&error_message);

                    if !category.is_retryable() || retry_attempt >= retry_config.max_retries {
                        return Err(error);
                    }

                    let delay = retry_delay(&retry_config, retry_attempt);
                    tracing::warn!(
                        attempt = retry_attempt + 1,
                        max_retries = retry_config.max_retries,
                        delay_ms = delay.as_millis() as u64,
                        category = ?category,
                        error = %error_message,
                        "streaming API call failed before first event; retrying with backoff"
                    );

                    sleep(delay).await;
                    retry_attempt += 1;
                }
            }
        }
    }

    /// Send a non-streaming messages request.
    ///
    /// Internally uses the streaming endpoint and collects all events via
    /// `StreamAccumulator`.
    pub async fn messages(&self, request: MessagesRequest) -> Result<AssistantMessage> {
        use futures::StreamExt;

        let request = resolve_request_model_for_provider(&self.config.provider, request)?;
        let model = request.model.clone();
        let stream = self.messages_stream(request).await?;
        let mut stream = std::pin::pin!(stream);

        let mut accumulator = StreamAccumulator::new();

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    accumulator.process_event(&event);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(accumulator.build(&model))
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &ApiClientConfig {
        &self.config
    }

    pub fn langfuse_provider_name(&self) -> &str {
        self.config.provider.langfuse_provider_name()
    }

    pub fn provider_diagnostic(&self) -> crate::api::providers::ProviderDiagnostic {
        crate::api::providers::ProviderDiagnostic {
            code: "provider_selected",
            message: format!(
                "Using provider `{}`",
                self.config.provider.langfuse_provider_name()
            ),
            endpoint_kind: self.config.provider.endpoint_kind(),
            base_url_host: self.config.provider.base_url_host(),
        }
    }
}

/// Return true if env var `name` is set to a truthy value (`1`, `true`, `yes`,
/// `on`). Matches claude-code-bun's `isEnvTruthy` semantics.
pub fn is_env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn is_codex_backend(value: &str) -> bool {
    value.eq_ignore_ascii_case("codex")
}
