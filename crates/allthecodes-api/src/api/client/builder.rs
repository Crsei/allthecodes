//! ApiClient factory / construction methods.

use anyhow::{bail, Context, Result};

use super::model::{
    anthropic_base_url_from_env, resolve_anthropic_default_model, resolve_anthropic_model_alias,
    selected_api_provider_from_settings,
};
use super::types::{
    AnthropicAuth, ApiClient, ApiClientConfig, ApiProvider, ANTHROPIC_DEFAULT_MODEL_ALIAS,
    OPENAI_CODEX_BASE_URL_ENV, OPENAI_CODEX_MODEL_ENV, OPENAI_CODEX_PROVIDER_NAME,
    OPENAI_PROVIDER_NAME,
};
use crate::api::providers::{AnthropicEndpointKind, ProviderInfo, ProviderProtocol};

// ---------------------------------------------------------------------------
// Stream provider factory
// ---------------------------------------------------------------------------

pub(super) fn make_stream_provider(
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

// ---------------------------------------------------------------------------
// Proxy resolution (settings.json → env vars)
// ---------------------------------------------------------------------------

/// Resolve the HTTP proxy URL to use for the API client.
///
/// Priority:
/// 1. `settings.json::proxyUrl` (read via `load_effective(cwd)`, which merges
///    the full managed → user → project → local layer stack), gated only by
///    `proxyEnabled: false` as an explicit opt-out. Presence of a non-empty
///    `proxyUrl` is treated as "use it" so users don't have to also flip
///    `proxyEnabled`. This mirrors the read pattern used by
///    [`selected_api_provider_from_settings`](super::model::selected_api_provider_from_settings):
///    the user-level `~/.allthecodes/settings.json` is the natural home for a
///    machine-wide proxy, while a project `.allthecodes/settings.json` can
///    override it per-repo. Settings-load failures are non-fatal: we log at
///    `debug` and fall through to env.
/// 2. `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` (upper- then lower-case),
///    preserving the original precedence so existing shell/env flows keep
///    working when no `proxyUrl` is configured on disk.
fn resolve_proxy_url() -> Option<String> {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(loaded) = allthecodes_config::settings::load_effective(&cwd) {
            let eff = &loaded.effective;
            if !matches!(eff.proxy_enabled, Some(false)) {
                if let Some(url) = eff.proxy_url.as_deref() {
                    let trimmed = url.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        } else {
            tracing::debug!("could not load effective settings for proxy_url");
        }
    }

    std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .or_else(|_| std::env::var("ALL_PROXY"))
        .ok()
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

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

fn build_http_client(
    timeout: std::time::Duration,
    proxy: Option<reqwest::Proxy>,
    streaming: bool,
) -> reqwest::Client {
    let mut builder =
        reqwest::Client::builder().user_agent(allthecodes_config::user_agent::api_user_agent());
    builder = if streaming {
        builder.connect_timeout(timeout).read_timeout(timeout)
    } else {
        builder.timeout(timeout)
    };
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    } else {
        builder = builder.no_proxy();
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

// ---------------------------------------------------------------------------
// ApiClient construction methods
// ---------------------------------------------------------------------------

impl ApiClient {
    /// Try to construct a new `ApiClient` from a fully-formed config.
    /// Validates the provider configuration before building.
    pub fn try_new(config: ApiClientConfig) -> Result<Self> {
        validate_provider_config(&config.provider)?;
        require_non_empty(&config.default_model, "default model")?;

        let stream_provider = make_stream_provider(&config.provider);
        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let proxy =
            resolve_proxy_url().and_then(|proxy_url| match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy) => {
                    tracing::info!(proxy = %proxy_url, "using explicit HTTP proxy");
                    Some(proxy)
                }
                Err(_) => {
                    tracing::warn!(proxy = %proxy_url, "invalid proxy URL, ignoring");
                    None
                }
            });
        Ok(Self {
            http: build_http_client(timeout, proxy.clone(), false),
            stream_http: build_http_client(timeout, proxy, true),
            stream_provider,
            config,
        })
    }

    /// Construct a new `ApiClient`, panicking on invalid config.
    #[cfg(test)]
    pub fn new(config: ApiClientConfig) -> Self {
        Self::try_new(config).expect("invalid API client configuration")
    }

    /// Construct an `ApiClient` from a `ProviderInfo` and API key.
    pub fn from_provider_info(info: &ProviderInfo, api_key: &str) -> Result<Self> {
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
            resolve_anthropic_default_model(AnthropicEndpointKind::DirectAnthropic)?
        } else {
            info.default_model.to_string()
        };
        Self::try_new(ApiClientConfig {
            provider,
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
    }

    /// Auto-detect provider from environment variables and construct an
    /// `ApiClient`.
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
        if super::is_env_truthy("ALLTHECODES_USE_FOUNDRY") {
            let validation = crate::api::providers::validate_provider_name("azure-foundry");
            let reason = validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or(crate::api::providers::FOUNDRY_UNSUPPORTED_REASON);
            bail!("{reason}");
        }
        if super::is_env_truthy("ALLTHECODES_USE_BEDROCK") {
            return Self::from_bedrock_env_result().map(Some);
        }
        if super::is_env_truthy("ALLTHECODES_USE_VERTEX") {
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

        Self::from_provider_info(info, &api_key).map(Some)
    }

    /// Convenience wrapper that logs and returns `None` on error.
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
    /// - `AWS_REGION` / `AWS_DEFAULT_REGION` -- region selection
    /// - `AWS_BEARER_TOKEN_BEDROCK` -- preferred auth (Bedrock API key)
    /// - `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` -- SigV4
    /// - `ANTHROPIC_BEDROCK_BASE_URL` -- override the default endpoint
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
    /// - `CLOUD_ML_REGION` -- region (default: `us-east5`)
    /// - `ANTHROPIC_VERTEX_PROJECT_ID` / `GOOGLE_CLOUD_PROJECT` / `GCLOUD_PROJECT` -- project ID
    /// - `ALLTHECODES_VERTEX_ACCESS_TOKEN` / `GOOGLE_OAUTH_ACCESS_TOKEN` -- access token
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
    /// - `OPENAI_CODEX_MODEL` (default: gpt-5.6-sol)
    pub fn from_codex_auth() -> Option<Self> {
        match Self::from_codex_auth_result() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "OpenAI Codex auth configuration rejected");
                None
            }
        }
    }

    /// Result-returning variant of [`from_codex_auth`].
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
        if backend.is_some_and(super::is_codex_backend) {
            return Self::from_codex_auth_result();
        }
        Self::from_auth_result()
    }

    /// Convenience wrapper that logs and returns `None` on error.
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
        if super::is_env_truthy("ALLTHECODES_USE_FOUNDRY") {
            let validation = crate::api::providers::validate_provider_name("azure-foundry");
            let reason = validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or(crate::api::providers::FOUNDRY_UNSUPPORTED_REASON);
            bail!("{reason}");
        }
        if super::is_env_truthy("ALLTHECODES_USE_BEDROCK") {
            return Self::from_bedrock_env_result().map(Some);
        }
        if super::is_env_truthy("ALLTHECODES_USE_VERTEX") {
            return Self::from_vertex_env_result().map(Some);
        }
        Ok(None)
    }

    fn from_selected_provider_result(provider: &str) -> Result<Option<Self>> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let loaded = allthecodes_config::settings::load_effective(&cwd)?;
        let profile = loaded
            .effective
            .active_auth_profile
            .as_ref()
            .and_then(|id| loaded.effective.auth_profiles.get(id));

        if provider == allthecodes_config::settings::API_PROVIDER_BEDROCK {
            return Self::from_bedrock_profile_result(profile).map(Some);
        }
        if provider == allthecodes_config::settings::API_PROVIDER_VERTEX {
            return Self::from_vertex_profile_result(profile).map(Some);
        }
        if provider == allthecodes_config::settings::API_PROVIDER_FOUNDRY {
            bail!("{}", crate::api::providers::FOUNDRY_UNSUPPORTED_REASON);
        }

        if let Some(info) = crate::api::providers::get_provider(provider) {
            let profile_key = profile
                .and_then(|profile| profile.api_key.clone())
                .or_else(|| {
                    profile
                        .and_then(|profile| profile.env.as_ref())
                        .and_then(|env| env.get(info.env_key).cloned())
                })
                .filter(|value| !value.trim().is_empty());
            if let Some(api_key) = profile_key {
                return Self::from_provider_profile(info, &api_key, profile).map(Some);
            }
        }

        match provider {
            allthecodes_config::settings::API_PROVIDER_OPENAI => {
                if let Some(info) = crate::api::providers::get_provider(OPENAI_PROVIDER_NAME) {
                    if let Ok(api_key) = std::env::var(info.env_key) {
                        if !api_key.trim().is_empty() {
                            return Self::from_provider_info(info, &api_key).map(Some);
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

    fn from_provider_profile(
        info: &ProviderInfo,
        api_key: &str,
        profile: Option<&allthecodes_config::settings::ProviderProfileSettings>,
    ) -> Result<Self> {
        let base_url = profile
            .and_then(|profile| profile.base_url.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| info.base_url.to_string())
            .trim_end_matches('/')
            .to_string();
        let default_model = profile
            .and_then(|profile| profile.model.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| info.default_model.to_string());
        let provider = match info.protocol {
            ProviderProtocol::Anthropic => ApiProvider::Anthropic {
                auth: AnthropicAuth::ApiKey(api_key.to_string()),
                endpoint_kind: crate::api::providers::anthropic_endpoint_kind_for_base_url(Some(
                    &base_url,
                )),
                base_url: Some(base_url),
            },
            ProviderProtocol::OpenAiCompat => ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key: api_key.to_string(),
                base_url,
                default_model: default_model.clone(),
            },
            ProviderProtocol::Google => ApiProvider::Google {
                api_key: api_key.to_string(),
                base_url,
            },
        };
        Self::try_new(ApiClientConfig {
            provider,
            default_model,
            max_retries: 3,
            timeout_secs: 120,
        })
    }

    fn from_bedrock_profile_result(
        profile: Option<&allthecodes_config::settings::ProviderProfileSettings>,
    ) -> Result<Self> {
        let env = profile.and_then(|profile| profile.env.as_ref());
        let bearer = profile
            .and_then(|profile| profile.api_key.clone())
            .or_else(|| env.and_then(|env| env.get("AWS_BEARER_TOKEN_BEDROCK").cloned()))
            .filter(|value| !value.trim().is_empty());
        let auth = if let Some(token) = bearer {
            crate::api::bedrock::BedrockAuth::BearerToken(token)
        } else if let (Some(access_key_id), Some(secret_access_key)) = (
            env.and_then(|env| env.get("AWS_ACCESS_KEY_ID").cloned()),
            env.and_then(|env| env.get("AWS_SECRET_ACCESS_KEY").cloned()),
        ) {
            crate::api::bedrock::BedrockAuth::AwsCredentials(crate::api::sigv4::AwsCredentials {
                access_key_id,
                secret_access_key,
                session_token: env.and_then(|env| env.get("AWS_SESSION_TOKEN").cloned()),
            })
        } else {
            crate::api::bedrock::BedrockAuth::from_env().ok_or_else(|| {
                anyhow::anyhow!("active Bedrock profile has no usable credentials")
            })?
        };
        let region = env
            .and_then(|env| {
                env.get("AWS_REGION")
                    .or_else(|| env.get("AWS_DEFAULT_REGION"))
            })
            .cloned()
            .unwrap_or_else(crate::api::bedrock::resolve_region);
        let base_url_override = profile
            .and_then(|profile| profile.base_url.clone())
            .or_else(|| env.and_then(|env| env.get("ANTHROPIC_BEDROCK_BASE_URL").cloned()));
        let default_model = profile
            .and_then(|profile| profile.model.clone())
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

    fn from_vertex_profile_result(
        profile: Option<&allthecodes_config::settings::ProviderProfileSettings>,
    ) -> Result<Self> {
        let env = profile.and_then(|profile| profile.env.as_ref());
        let project_id = env
            .and_then(|env| {
                env.get("ANTHROPIC_VERTEX_PROJECT_ID")
                    .or_else(|| env.get("GOOGLE_CLOUD_PROJECT"))
                    .or_else(|| env.get("GCLOUD_PROJECT"))
            })
            .cloned()
            .or_else(crate::api::vertex::resolve_project_id)
            .ok_or_else(|| anyhow::anyhow!("active Vertex profile has no project id"))?;
        let access_token = profile
            .and_then(|profile| profile.api_key.clone())
            .or_else(|| {
                env.and_then(|env| {
                    env.get("ALLTHECODES_VERTEX_ACCESS_TOKEN")
                        .or_else(|| env.get("GOOGLE_OAUTH_ACCESS_TOKEN"))
                        .cloned()
                })
            })
            .filter(|value| !value.trim().is_empty())
            .map(crate::api::vertex::VertexAccessToken)
            .or_else(crate::api::vertex::VertexAccessToken::from_env_or_gcloud)
            .ok_or_else(|| anyhow::anyhow!("active Vertex profile has no OAuth access token"))?;
        let region = env
            .and_then(|env| env.get("CLOUD_ML_REGION").cloned())
            .unwrap_or_else(crate::api::vertex::resolve_region);
        let default_model = profile
            .and_then(|profile| profile.model.clone())
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

    /// Convenience wrapper that logs and returns `None` on error.
    pub fn from_auth() -> Option<Self> {
        match Self::from_auth_result() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, "API auth configuration rejected");
                None
            }
        }
    }
}

#[cfg(test)]
mod provider_profile_tests {
    use super::*;
    use futures::StreamExt;
    use std::collections::HashMap;
    use std::io::{Read, Write};

    #[test]
    fn every_static_provider_builds_from_profile_credentials() {
        for info in crate::api::providers::PROVIDERS {
            let profile = allthecodes_config::settings::ProviderProfileSettings {
                api_provider: Some(info.name.to_string()),
                model: Some(format!("{}-profile-model", info.name)),
                base_url: Some(info.base_url.to_string()),
                api_key: Some("profile-secret".to_string()),
                ..Default::default()
            };
            let client = ApiClient::from_provider_profile(info, "profile-secret", Some(&profile))
                .unwrap_or_else(|error| panic!("{} profile failed: {error}", info.name));
            assert_eq!(
                client.config.default_model,
                format!("{}-profile-model", info.name)
            );
        }
    }

    #[test]
    fn bedrock_and_vertex_build_from_profile_env_without_process_mutation() {
        let bedrock = allthecodes_config::settings::ProviderProfileSettings {
            api_provider: Some("bedrock".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            env: Some(HashMap::from([
                (
                    "AWS_BEARER_TOKEN_BEDROCK".to_string(),
                    "bedrock-secret".to_string(),
                ),
                ("AWS_REGION".to_string(), "us-west-2".to_string()),
            ])),
            ..Default::default()
        };
        assert!(ApiClient::from_bedrock_profile_result(Some(&bedrock)).is_ok());

        let vertex = allthecodes_config::settings::ProviderProfileSettings {
            api_provider: Some("vertex".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            env: Some(HashMap::from([
                (
                    "ANTHROPIC_VERTEX_PROJECT_ID".to_string(),
                    "project".to_string(),
                ),
                (
                    "ALLTHECODES_VERTEX_ACCESS_TOKEN".to_string(),
                    "vertex-secret".to_string(),
                ),
                ("CLOUD_ML_REGION".to_string(), "us-east5".to_string()),
            ])),
            ..Default::default()
        };
        assert!(ApiClient::from_vertex_profile_result(Some(&vertex)).is_ok());
    }

    #[tokio::test]
    async fn streaming_http_timeout_resets_after_each_successful_read() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept request");
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .expect("set request read timeout");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\na\r\n",
                )
                .expect("write response headers");
            socket.flush().expect("flush first chunk");
            std::thread::sleep(std::time::Duration::from_millis(600));
            socket.write_all(b"1\r\nb\r\n").expect("write second chunk");
            socket.flush().expect("flush second chunk");
            std::thread::sleep(std::time::Duration::from_millis(600));
            socket
                .write_all(b"1\r\nc\r\n0\r\n\r\n")
                .expect("write final chunk");
        });

        let client = build_http_client(std::time::Duration::from_secs(1), None, true);
        let started = std::time::Instant::now();
        let response = client
            .get(format!("http://{address}/stream"))
            .send()
            .await
            .expect("establish streaming response");
        let mut body = response.bytes_stream();
        let mut received = Vec::new();
        while let Some(chunk) = body.next().await {
            received.extend_from_slice(&chunk.expect("read streaming chunk"));
        }
        server.join().expect("join test server");

        assert_eq!(received, b"abc");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(1_100),
            "stream should outlive the one-second timeout as long as every read makes progress"
        );
    }
}
