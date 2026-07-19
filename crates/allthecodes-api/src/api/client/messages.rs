//! Core ApiClient operational methods: streaming, counting, headers, URL building.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::Stream;
use serde_json::Value;

use super::body::{
    apply_prompt_cache_policy_to_body, is_official_anthropic_base_url,
    strip_anthropic_compatible_only_fields,
};
use super::headers::{
    build_anthropic_headers, build_anthropic_headers_for_body,
    build_anthropic_headers_for_body_with_beta_policy, extend_header_string_map,
};
use super::model::{build_anthropic_count_tokens_body, resolve_request_model_for_provider};
use super::provider::build_openai_compat_url;
use super::types::{
    AnthropicAuth, ApiClient, ApiClientConfig, ApiProvider, ExactTokenCount, MessagesRequest,
    PromptCacheCapability,
};
use crate::api::providers::AnthropicEndpointKind;
use crate::api::retry::{categorize_stream_start_error, retry_delay, RetryConfig};
use allthecodes_types::message::{AssistantMessage, StreamEvent};

// ---------------------------------------------------------------------------
// Token counting
// ---------------------------------------------------------------------------

impl ApiClient {
    pub async fn messages_stream_once(
        &self,
        request: MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let request = resolve_request_model_for_provider(&self.config.provider, request)?;
        self.stream_provider
            .stream(&self.stream_http, &request)
            .await
    }

    /// Return `true` when the current provider supports exact token counting.
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

    /// Count input tokens and return a `TokenUsageReport`.
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
}

// ---------------------------------------------------------------------------
// URL building
// ---------------------------------------------------------------------------

impl ApiClient {
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
                &allthecodes_types::models::to_bedrock_model_id(&self.config.default_model),
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
                    &allthecodes_types::models::to_vertex_model_id(&self.config.default_model),
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Header building
// ---------------------------------------------------------------------------

impl ApiClient {
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
}

// ---------------------------------------------------------------------------
// Messages streaming
// ---------------------------------------------------------------------------

impl ApiClient {
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

    pub(super) async fn messages_stream_with_backoff<SleepFn, SleepFuture>(
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
            match self
                .stream_provider
                .stream(&self.stream_http, &request)
                .await
            {
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

        let mut accumulator = crate::api::streaming::StreamAccumulator::new();

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

    /// Langfuse provider name for the current client.
    pub fn langfuse_provider_name(&self) -> &str {
        self.config.provider.langfuse_provider_name()
    }

    /// Whether this client uses the OpenAI Codex Responses streaming protocol.
    pub fn uses_codex_responses(&self) -> bool {
        matches!(
            &self.config.provider,
            ApiProvider::OpenAiCompat { name, .. } if super::is_openai_codex_provider(name)
        )
    }

    /// Return a provider diagnostic struct describing the current provider.
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
