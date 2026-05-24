//! StreamProvider trait 鈥?unified provider dispatch for streaming API calls.
//!
//! Each LLM provider (Anthropic, OpenAI-compatible, Google Gemini) implements
//! this trait. The `ApiClient` stores a `Box<dyn StreamProvider>` and dispatches
//! through it, eliminating match-based routing in `messages_stream()`.

use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;

use crate::api::client::{
    apply_prompt_cache_policy_to_body, build_anthropic_headers_for_body,
    build_anthropic_headers_for_body_with_beta_policy, is_official_anthropic_base_url,
    parse_sse_byte_stream, strip_anthropic_compatible_only_fields, AnthropicAuth, MessagesRequest,
    PromptCacheCapability,
};
use crate::api::streaming::normalize_api_error_body;
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
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

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
        let headers = if direct_official_anthropic {
            build_anthropic_headers_for_body(&self.auth, false, &body_value)?
        } else {
            build_anthropic_headers_for_body_with_beta_policy(
                &self.auth,
                false,
                &body_value,
                false,
            )?
        };
        let body_json =
            serde_json::to_string(&body_value).context("failed to serialize request body")?;

        let response = http
            .post(&url)
            .headers(headers)
            .body(body_json)
            .send()
            .await
            .context("failed to send HTTP request")?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            let request_id = response
                .headers()
                .get("request-id")
                .or_else(|| response.headers().get("x-request-id"))
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("(failed to read error body)"));
            return Err(normalize_api_error_body(
                "anthropic",
                Some(status),
                &error_body,
                request_id,
            )
            .into());
        }

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
