//! Type definitions, enums, structs, and constants for the API client.

use std::sync::Once;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::api::providers::{AnthropicEndpointKind, ProviderCapabilities};

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Private (crate/super-only) constants
// ---------------------------------------------------------------------------

pub(super) const ANTHROPIC_OFFICIAL_SOTA_MODEL: &str = "claude-opus-4-7";
pub(super) const ANTHROPIC_OFFICIAL_MOTA_MODEL: &str = "claude-sonnet-4-6";
pub(super) const ANTHROPIC_OFFICIAL_FOTA_MODEL: &str = "claude-haiku-4-5-20251001";
pub(super) const ANTHROPIC_DEFAULT_MODEL_ALIAS: &str = "MOTA";

// ---------------------------------------------------------------------------
// Legacy model env warning (one-shot)
// ---------------------------------------------------------------------------

pub(super) static ANTHROPIC_LEGACY_MODEL_ENV_WARNING: Once = Once::new();

// ---------------------------------------------------------------------------
// Prompt cache types
// ---------------------------------------------------------------------------

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
                && super::is_env_truthy("ALLTHECODES_PROMPT_CACHE_GLOBAL"),
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

// ---------------------------------------------------------------------------
// Helpers used by PromptCachePolicy
// ---------------------------------------------------------------------------

fn is_env_value(name: &str, expected: &str) -> bool {
    std::env::var(name)
        .map(|value| value.trim().eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// ApiProvider enum + inherent methods
// ---------------------------------------------------------------------------

/// API provider enum -- determines wire protocol and auth method.
#[derive(Debug, Clone)]
pub enum ApiProvider {
    /// Direct Anthropic API (native Messages API).
    Anthropic {
        auth: AnthropicAuth,
        base_url: Option<String>,
        endpoint_kind: AnthropicEndpointKind,
    },
    /// Azure Foundry (Anthropic-compatible).
    Azure { endpoint: String, api_key: String },
    /// OpenAI-compatible provider (OpenAI, DeepSeek, Groq, Qwen, etc.).
    OpenAiCompat {
        name: String,
        api_key: String,
        base_url: String,
        default_model: String,
    },
    /// Google Gemini (streamGenerateContent API).
    Google { api_key: String, base_url: String },
    /// AWS Bedrock -- Claude via AWS-managed endpoints.
    ///
    /// `base_url_override` is read from `ANTHROPIC_BEDROCK_BASE_URL` when set.
    Bedrock {
        region: String,
        auth: crate::api::bedrock::BedrockAuth,
        base_url_override: Option<String>,
    },
    /// GCP Vertex AI -- Claude via Google-managed endpoints.
    Vertex {
        project_id: String,
        region: String,
        access_token: crate::api::vertex::VertexAccessToken,
    },
}

impl ApiProvider {
    /// The kind of Anthropic endpoint, if this provider speaks the
    /// Anthropic Messages wire format.
    pub fn endpoint_kind(&self) -> Option<AnthropicEndpointKind> {
        match self {
            ApiProvider::Anthropic { endpoint_kind, .. } => Some(*endpoint_kind),
            ApiProvider::Azure { .. }
            | ApiProvider::Bedrock { .. }
            | ApiProvider::Vertex { .. } => Some(AnthropicEndpointKind::DirectAnthropic),
            ApiProvider::OpenAiCompat { .. } | ApiProvider::Google { .. } => None,
        }
    }

    /// Human-readable host name for the provider's API endpoint.
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

    /// Langfuse-compatible provider name for tracing.
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

    /// Return the provider's [`ProviderCapabilities`] (feature matrix).
    pub fn capabilities(&self) -> ProviderCapabilities {
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

// ---------------------------------------------------------------------------
// AnthropicAuth
// ---------------------------------------------------------------------------

/// Authentication method for Anthropic-format native requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicAuth {
    ApiKey(String),
    BearerToken(String),
}

impl AnthropicAuth {
    /// Return the raw secret value (key or token).
    pub fn secret(&self) -> &str {
        match self {
            Self::ApiKey(value) | Self::BearerToken(value) => value,
        }
    }

    /// Human-readable label for the auth method.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ApiKey(_) => "Anthropic API key",
            Self::BearerToken(_) => "Anthropic bearer token",
        }
    }

    /// Insert the appropriate auth header into a [`HeaderMap`].
    pub fn insert_auth_header(&self, headers: &mut HeaderMap) -> Result<()> {
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

// ---------------------------------------------------------------------------
// MessagesRequest
// ---------------------------------------------------------------------------

/// Request body for the Messages API.
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

// ---------------------------------------------------------------------------
// ExactTokenCount
// ---------------------------------------------------------------------------

/// Provider-level token count returned by an exact count endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactTokenCount {
    pub input_tokens: u64,
    pub provider: String,
}

// ---------------------------------------------------------------------------
// ApiClientConfig / ApiClient
// ---------------------------------------------------------------------------

/// API client configuration.
#[derive(Debug, Clone)]
pub struct ApiClientConfig {
    pub provider: ApiProvider,
    pub default_model: String,
    pub max_retries: usize,
    pub timeout_secs: u64,
}

/// The API client -- uses reqwest under the hood.
pub struct ApiClient {
    pub config: ApiClientConfig,
    pub http: reqwest::Client,
    pub stream_provider: Box<dyn crate::api::stream_provider::StreamProvider>,
}
