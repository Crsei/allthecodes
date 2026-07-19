//! StreamProvider trait 鈥?unified provider dispatch for streaming API calls.
//!
//! Each LLM provider (Anthropic, OpenAI-compatible, Google Gemini) implements
//! this trait. The `ApiClient` stores a `Box<dyn StreamProvider>` and dispatches
//! through it, eliminating match-based routing in `messages_stream()`.

use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;

use crate::api::client::{
    apply_prompt_cache_policy_to_body, is_official_anthropic_base_url, parse_sse_byte_stream,
    strip_anthropic_compatible_only_fields, AnthropicAuth, MessagesRequest, PromptCacheCapability,
};
use crate::api::provider_runtime::{
    metadata_from_response, provider_error_from_response, ProviderEndpoint, ProviderStreamTransport,
};
use allthecodes_types::message::StreamEvent;

/// Trait for provider-specific streaming implementations.
#[async_trait::async_trait]
pub trait StreamProvider: Send + Sync {
    /// Send a streaming request and return a stream of unified `StreamEvent`s.
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>;
}

// ---------------------------------------------------------------------------
// Anthropic / Azure (native Messages API SSE)
// ---------------------------------------------------------------------------

pub struct AnthropicStreamProvider {
    pub auth: AnthropicAuth,
    pub base_url: String,
}

#[async_trait::async_trait]
impl StreamProvider for AnthropicStreamProvider {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let mut req_body = request.clone();
        req_body.stream = true;

        let mut body_value =
            serde_json::to_value(&req_body).context("failed to serialize request body")?;
        let direct_official_anthropic = is_official_anthropic_base_url(&self.base_url);
        if direct_official_anthropic {
            apply_prompt_cache_policy_to_body(
                &mut body_value,
                PromptCacheCapability {
                    explicit_markers: true,
                    ttl_1h: true,
                    global_scope: true,
                    direct_official_anthropic: true,
                },
            );
        } else {
            strip_anthropic_compatible_only_fields(&mut body_value);
        }
        let endpoint = ProviderEndpoint::anthropic(
            &self.auth,
            self.base_url.clone(),
            direct_official_anthropic,
            &body_value,
        )?;
        let url = endpoint.url_for_path("/v1/messages")?;
        let body_json =
            serde_json::to_string(&body_value).context("failed to serialize request body")?;

        let response = endpoint
            .apply_headers(http.post(url))
            .body(body_json)
            .send()
            .await
            .context("failed to send HTTP request")?;

        if !response.status().is_success() {
            return Err(provider_error_from_response("anthropic", response)
                .await
                .into());
        }

        let metadata =
            metadata_from_response("anthropic", ProviderStreamTransport::SseHttp, &response);
        tracing::debug!(?metadata, "provider stream established");
        let byte_stream = response.bytes_stream();
        let sse_stream = parse_sse_byte_stream(byte_stream);

        Ok(Box::pin(sse_stream))
    }
}

// ---------------------------------------------------------------------------
// OpenAI-compatible (DeepSeek, Groq, Qwen, Azure OpenAI, etc.)
// ---------------------------------------------------------------------------

pub struct OpenAiCompatStreamProvider {
    pub name: String,
    pub api_key: String,
    pub base_url: String,
    pub request_timeout: std::time::Duration,
    pub stream_idle_timeout: std::time::Duration,
}

#[async_trait::async_trait]
impl StreamProvider for OpenAiCompatStreamProvider {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        crate::api::openai_compat::openai_compat_stream(
            http,
            &self.base_url,
            &self.api_key,
            &self.name,
            request,
            self.request_timeout,
            self.stream_idle_timeout,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Google Gemini (streamGenerateContent)
// ---------------------------------------------------------------------------

pub struct GoogleStreamProvider {
    pub api_key: String,
    pub base_url: String,
}

#[async_trait::async_trait]
impl StreamProvider for GoogleStreamProvider {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        crate::api::google_provider::google_stream(http, &self.base_url, &self.api_key, request)
            .await
    }
}
