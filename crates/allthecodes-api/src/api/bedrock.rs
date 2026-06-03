//! AWS Bedrock provider 鈥?routes Claude requests to AWS-managed Claude endpoints.
//!
//! Bedrock is one of several "third-party cloud" providers for Claude; this
//! module is a thin adaptation layer that reuses the existing streaming
//! architecture. It is NOT a separate product 鈥?it's an alternative transport
//! for the same Claude conversation loop.
//!
//! # Endpoint
//!
//! `POST https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/invoke-with-response-stream`
//!
//! Exact token count uses:
//!
//! `POST https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/count-tokens`
//!
//! With `ANTHROPIC_BEDROCK_BASE_URL` set, the base is overridden (useful for
//! proxies / mock servers).
//!
//! # Authentication (MVP)
//!
//! Two modes supported:
//! - `AWS_BEARER_TOKEN_BEDROCK` (Bedrock API key) 鈫?`Authorization: Bearer ...`
//! - `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ optional `AWS_SESSION_TOKEN`)
//!   鈫?SigV4 request signing
//!
//! # Request body
//!
//! Bedrock expects the Anthropic Messages body with two differences:
//! - The `model` field is removed (model is in the URL).
//! - `anthropic_version: "bedrock-2023-05-31"` is required.
//! - `stream` is removed (endpoint suffix determines streaming).
//!
//! # Streaming
//!
//! Bedrock returns AWS EventStream frames from `/invoke-with-response-stream`.
//! Each `chunk` payload is an Anthropic Messages stream event encoded as JSON,
//! so this module decodes the EventStream envelope and then reuses the common
//! Anthropic event parser.

use std::collections::HashMap;
use std::pin::Pin;

use anyhow::{bail, Context, Result};
use base64::Engine;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::api::client::{strip_anthropic_cache_fields, MessagesRequest};
use crate::api::sigv4::{self, AwsCredentials, SignRequest};
use crate::api::streaming::parse_sse_event;
use allthecodes_models::to_bedrock_model_id;
use allthecodes_types::message::StreamEvent;

pub const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// How the caller authenticates to Bedrock.
#[derive(Debug, Clone)]
pub enum BedrockAuth {
    /// Pre-issued Bedrock API key (`AWS_BEARER_TOKEN_BEDROCK`).
    BearerToken(String),
    /// Standard AWS credentials that will be used to SigV4-sign each request.
    AwsCredentials(AwsCredentials),
}

impl BedrockAuth {
    /// Resolve auth from the environment.
    ///
    /// Priority:
    /// 1. `AWS_BEARER_TOKEN_BEDROCK` (simpler; matches claude-code-bun).
    /// 2. `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (SigV4).
    pub fn from_env() -> Option<Self> {
        if let Ok(tok) = std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
            if !tok.is_empty() {
                return Some(Self::BearerToken(tok));
            }
        }
        AwsCredentials::from_env().map(Self::AwsCredentials)
    }
}

/// Resolve the AWS region for Bedrock.
///
/// Matches claude-code-bun: `AWS_REGION` 鈫?`AWS_DEFAULT_REGION`
/// 鈫?default `us-east-1`.
pub fn resolve_region() -> String {
    std::env::var("AWS_REGION")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::env::var("AWS_DEFAULT_REGION")
                .ok()
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "us-east-1".to_string())
}

/// Build the Bedrock streaming invoke URL for a given model ID.
pub fn build_invoke_stream_url(
    region: &str,
    model_id: &str,
    base_url_override: Option<&str>,
) -> String {
    let model_id_encoded = urlencoding::encode(model_id);
    if let Some(base) = base_url_override {
        let base = base.trim_end_matches('/');
        return format!(
            "{}/model/{}/invoke-with-response-stream",
            base, model_id_encoded
        );
    }
    format!(
        "https://bedrock-runtime.{}.amazonaws.com/model/{}/invoke-with-response-stream",
        region, model_id_encoded
    )
}

/// Build the Bedrock CountTokens URL for a given model ID.
pub fn build_count_tokens_url(
    region: &str,
    model_id: &str,
    base_url_override: Option<&str>,
) -> String {
    let model_id_encoded = urlencoding::encode(model_id);
    if let Some(base) = base_url_override {
        let base = base.trim_end_matches('/');
        return format!("{}/model/{}/count-tokens", base, model_id_encoded);
    }
    format!(
        "https://bedrock-runtime.{}.amazonaws.com/model/{}/count-tokens",
        region, model_id_encoded
    )
}

/// Convert a `MessagesRequest` into the Bedrock-specific JSON body.
fn to_bedrock_body(request: &MessagesRequest) -> Result<Vec<u8>> {
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let mut body = json!({
        "anthropic_version": BEDROCK_ANTHROPIC_VERSION,
        "max_tokens": request.max_tokens,
        "messages": messages.as_array().cloned().unwrap_or_default(),
    });
    if let Some(system) = &request.system {
        let mut value = Value::Array(system.clone());
        strip_anthropic_cache_fields(&mut value);
        body["system"] = value;
    }
    if let Some(tools) = &request.tools {
        let mut value = Value::Array(tools.clone());
        strip_anthropic_cache_fields(&mut value);
        body["tools"] = value;
    }
    if let Some(thinking) = &request.thinking {
        body["thinking"] = thinking.clone();
    }
    if let Some(tool_choice) = &request.tool_choice {
        body["tool_choice"] = tool_choice.clone();
    }
    serde_json::to_vec(&body).context("failed to serialize Bedrock request body")
}

/// Convert a `MessagesRequest` into the raw Bedrock CountTokens body.
///
/// The runtime API models `input.invokeModel.body` as a blob, so this raw HTTP
/// client sends the serialized InvokeModel body as base64 inside the JSON
/// envelope.
fn to_bedrock_count_tokens_body(request: &MessagesRequest) -> Result<Vec<u8>> {
    let invoke_model_body = to_bedrock_body(request)?;
    let body = json!({
        "input": {
            "invokeModel": {
                "body": base64::engine::general_purpose::STANDARD.encode(invoke_model_body),
            },
        },
    });
    serde_json::to_vec(&body).context("failed to serialize Bedrock CountTokens request body")
}

pub(crate) async fn bedrock_count_tokens(
    http: &reqwest::Client,
    region: &str,
    auth: &BedrockAuth,
    base_url_override: Option<&str>,
    request: &MessagesRequest,
) -> Result<u64> {
    #[derive(serde::Deserialize)]
    struct CountTokensResponse {
        #[serde(rename = "inputTokens")]
        input_tokens: u64,
    }

    let bedrock_model = to_bedrock_model_id(&request.model);
    let url = build_count_tokens_url(region, &bedrock_model, base_url_override);
    let body = to_bedrock_count_tokens_body(request)?;

    let mut builder = http
        .post(&url)
        .header("content-type", "application/json")
        .body(body.clone());

    match auth {
        BedrockAuth::BearerToken(tok) => {
            builder = builder.header("authorization", format!("Bearer {}", tok));
        }
        BedrockAuth::AwsCredentials(creds) => {
            let parsed = url::Url::parse(&url).context("invalid Bedrock CountTokens URL")?;
            let host = parsed
                .host_str()
                .context("Bedrock CountTokens URL missing host")?;
            let path = parsed.path().to_string();
            let (amz_date, date_stamp) = sigv4::current_timestamps();
            let signed = sigv4::sign(
                &SignRequest {
                    method: "POST",
                    host,
                    path: &path,
                    region,
                    service: "bedrock",
                    body: &body,
                    content_type: "application/json",
                    amz_date,
                    date_stamp,
                },
                creds,
            )?;
            builder = builder
                .header("authorization", signed.authorization)
                .header("x-amz-date", signed.x_amz_date)
                .header("x-amz-content-sha256", signed.x_amz_content_sha256);
            if let Some(tok) = signed.x_amz_security_token {
                builder = builder.header("x-amz-security-token", tok);
            }
        }
    }

    let response = builder
        .send()
        .await
        .context("failed to send Bedrock CountTokens request")?;
    let status = response.status();
    if !status.is_success() {
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("(failed to read Bedrock CountTokens body)"));
        bail!(
            "Bedrock CountTokens error (HTTP {}): {}",
            status.as_u16(),
            body_text
        );
    }

    let parsed: CountTokensResponse = response
        .json()
        .await
        .context("failed to parse Bedrock CountTokens response")?;
    Ok(parsed.input_tokens)
}

#[derive(Debug)]
struct EventStreamMessage {
    headers: HashMap<String, String>,
    payload: Vec<u8>,
}

fn parse_eventstream_byte_stream<S>(
    byte_stream: S,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = Vec::<u8>::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = chunk_result.context("error reading Bedrock event stream chunk")?;
            buffer.extend_from_slice(&chunk);

            while let Some(frame) = take_complete_eventstream_frame(&mut buffer)? {
                let message = decode_eventstream_message(&frame)?;
                for event in bedrock_message_to_stream_events(message)? {
                    yield event;
                }
            }
        }

        if !buffer.is_empty() {
            Err(anyhow::anyhow!("truncated Bedrock event stream frame"))?;
        }
    }
}

fn take_complete_eventstream_frame(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>> {
    if buffer.len() < 12 {
        return Ok(None);
    }

    let total_len = read_u32_be_at(buffer, 0, "event stream total length")? as usize;
    let headers_len = read_u32_be_at(buffer, 4, "event stream headers length")? as usize;
    if total_len < 16 {
        bail!("invalid Bedrock event stream frame length {}", total_len);
    }
    if headers_len > total_len.saturating_sub(16) {
        bail!(
            "invalid Bedrock event stream headers length {} for frame {}",
            headers_len,
            total_len
        );
    }
    if buffer.len() < total_len {
        return Ok(None);
    }

    Ok(Some(buffer.drain(..total_len).collect()))
}

fn decode_eventstream_message(frame: &[u8]) -> Result<EventStreamMessage> {
    if frame.len() < 16 {
        bail!("Bedrock event stream frame is too short");
    }

    let total_len = read_u32_be_at(frame, 0, "event stream total length")? as usize;
    let headers_len = read_u32_be_at(frame, 4, "event stream headers length")? as usize;
    if total_len != frame.len() {
        bail!(
            "Bedrock event stream frame length mismatch: header {}, actual {}",
            total_len,
            frame.len()
        );
    }

    let expected_prelude_crc = read_u32_be_at(frame, 8, "event stream prelude CRC")?;
    let actual_prelude_crc = crc32(&frame[0..8]);
    if expected_prelude_crc != actual_prelude_crc {
        bail!("Bedrock event stream prelude CRC mismatch");
    }

    let expected_message_crc = read_u32_be_at(frame, frame.len() - 4, "event stream message CRC")?;
    let actual_message_crc = crc32(&frame[..frame.len() - 4]);
    if expected_message_crc != actual_message_crc {
        bail!("Bedrock event stream message CRC mismatch");
    }

    let headers_start = 12;
    let headers_end = headers_start + headers_len;
    if headers_end > frame.len().saturating_sub(4) {
        bail!("Bedrock event stream headers exceed frame length");
    }
    let payload_end = frame.len() - 4;
    let headers = decode_eventstream_headers(&frame[headers_start..headers_end])?;
    let payload = frame[headers_end..payload_end].to_vec();

    Ok(EventStreamMessage { headers, payload })
}

fn decode_eventstream_headers(bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut headers = HashMap::new();
    let mut offset = 0usize;

    while offset < bytes.len() {
        let name_len = read_u8(bytes, &mut offset)? as usize;
        let name_bytes = read_bytes(bytes, &mut offset, name_len)?;
        let name = std::str::from_utf8(name_bytes)
            .context("Bedrock event stream header name is not UTF-8")?
            .to_string();
        let value_type = read_u8(bytes, &mut offset)?;

        match value_type {
            0 => {
                headers.insert(name, "true".to_string());
            }
            1 => {
                headers.insert(name, "false".to_string());
            }
            2 => {
                let _ = read_u8(bytes, &mut offset)?;
            }
            3 => {
                let _ = read_bytes(bytes, &mut offset, 2)?;
            }
            4 => {
                let _ = read_bytes(bytes, &mut offset, 4)?;
            }
            5 | 8 => {
                let _ = read_bytes(bytes, &mut offset, 8)?;
            }
            6 => {
                let len = read_u16(bytes, &mut offset)? as usize;
                let _ = read_bytes(bytes, &mut offset, len)?;
            }
            7 => {
                let len = read_u16(bytes, &mut offset)? as usize;
                let value_bytes = read_bytes(bytes, &mut offset, len)?;
                let value = std::str::from_utf8(value_bytes)
                    .context("Bedrock event stream string header is not UTF-8")?
                    .to_string();
                headers.insert(name, value);
            }
            9 => {
                let _ = read_bytes(bytes, &mut offset, 16)?;
            }
            other => bail!("unsupported Bedrock event stream header type {}", other),
        }
    }

    Ok(headers)
}

fn bedrock_message_to_stream_events(message: EventStreamMessage) -> Result<Vec<StreamEvent>> {
    let event_type = message.headers.get(":event-type").map(String::as_str);
    let message_type = message.headers.get(":message-type").map(String::as_str);

    if event_type == Some("chunk") {
        return anthropic_json_payload_to_stream_event(&message.payload)
            .map(|event| event.into_iter().collect());
    }

    let payload = String::from_utf8_lossy(&message.payload);
    match (message_type, event_type) {
        (Some("exception"), Some(kind)) => {
            bail!("Bedrock event stream exception {kind}: {payload}")
        }
        (_, Some(kind)) => bail!("unsupported Bedrock event stream event {kind}: {payload}"),
        _ => bail!("Bedrock event stream message missing :event-type: {payload}"),
    }
}

fn anthropic_json_payload_to_stream_event(payload: &[u8]) -> Result<Option<StreamEvent>> {
    let text = std::str::from_utf8(payload).context("Bedrock chunk payload is not UTF-8")?;
    let value: Value =
        serde_json::from_str(text).context("failed to parse Bedrock chunk payload JSON")?;

    if let Some(encoded) = value.get("bytes").and_then(|v| v.as_str()).or_else(|| {
        value
            .get("chunk")
            .and_then(|chunk| chunk.get("bytes"))
            .and_then(|v| v.as_str())
    }) {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .context("failed to decode Bedrock chunk bytes")?;
        return anthropic_json_payload_to_stream_event(&decoded);
    }

    let Some(event_type) = value.get("type").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    parse_sse_event(event_type, text)
}

fn read_u8(bytes: &[u8], offset: &mut usize) -> Result<u8> {
    let raw = read_bytes(bytes, offset, 1)?;
    Ok(raw[0])
}

fn read_u16(bytes: &[u8], offset: &mut usize) -> Result<u16> {
    let raw = read_bytes(bytes, offset, 2)?;
    Ok(u16::from_be_bytes([raw[0], raw[1]]))
}

fn read_bytes<'a>(bytes: &'a [u8], offset: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = offset.saturating_add(len);
    if end > bytes.len() {
        bail!("truncated Bedrock event stream header");
    }
    let out = &bytes[*offset..end];
    *offset = end;
    Ok(out)
}

fn read_u32_be_at(bytes: &[u8], offset: usize, label: &str) -> Result<u32> {
    let end = offset.saturating_add(4);
    if end > bytes.len() {
        bail!(
            "truncated Bedrock event stream {label}: need bytes {}..{}, frame has {} bytes",
            offset,
            end,
            bytes.len()
        );
    }
    Ok(u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// Bedrock stream provider (implements `StreamProvider`).
pub struct BedrockStreamProvider {
    pub region: String,
    pub auth: BedrockAuth,
    pub base_url_override: Option<String>,
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for BedrockStreamProvider {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let bedrock_model = to_bedrock_model_id(&request.model);
        let url = build_invoke_stream_url(
            &self.region,
            &bedrock_model,
            self.base_url_override.as_deref(),
        );
        let body = to_bedrock_body(request)?;

        let mut builder = http
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/vnd.amazon.eventstream")
            .body(body.clone());

        match &self.auth {
            BedrockAuth::BearerToken(tok) => {
                builder = builder.header("authorization", format!("Bearer {}", tok));
            }
            BedrockAuth::AwsCredentials(creds) => {
                let parsed = url::Url::parse(&url).context("invalid Bedrock URL")?;
                let host = parsed.host_str().context("Bedrock URL missing host")?;
                let path = parsed.path().to_string();
                let (amz_date, date_stamp) = sigv4::current_timestamps();
                let signed = sigv4::sign(
                    &SignRequest {
                        method: "POST",
                        host,
                        path: &path,
                        region: &self.region,
                        service: "bedrock",
                        body: &body,
                        content_type: "application/json",
                        amz_date,
                        date_stamp,
                    },
                    creds,
                )?;
                builder = builder
                    .header("authorization", signed.authorization)
                    .header("x-amz-date", signed.x_amz_date)
                    .header("x-amz-content-sha256", signed.x_amz_content_sha256);
                if let Some(tok) = signed.x_amz_security_token {
                    builder = builder.header("x-amz-security-token", tok);
                }
            }
        }

        let response = builder
            .send()
            .await
            .context("failed to send Bedrock streaming invoke request")?;
        let status = response.status();

        if !status.is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("(failed to read Bedrock response body)"));
            bail!(
                "Bedrock streaming invoke error (HTTP {}): {}",
                status.as_u16(),
                body_text
            );
        }

        let stream = parse_eventstream_byte_stream(response.bytes_stream());
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_defaults_to_us_east_1() {
        let saved_r = std::env::var("AWS_REGION").ok();
        let saved_dr = std::env::var("AWS_DEFAULT_REGION").ok();
        std::env::remove_var("AWS_REGION");
        std::env::remove_var("AWS_DEFAULT_REGION");

        assert_eq!(resolve_region(), "us-east-1");

        if let Some(v) = saved_r {
            std::env::set_var("AWS_REGION", v);
        }
        if let Some(v) = saved_dr {
            std::env::set_var("AWS_DEFAULT_REGION", v);
        }
    }

    #[test]
    fn stream_url_uses_region_and_model() {
        let url = build_invoke_stream_url(
            "eu-west-1",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            None,
        );
        assert_eq!(
            url,
            "https://bedrock-runtime.eu-west-1.amazonaws.com/model/us.anthropic.claude-sonnet-4-5-20250929-v1%3A0/invoke-with-response-stream"
        );
    }

    #[test]
    fn stream_url_override_from_base_url() {
        let url = build_invoke_stream_url("us-east-1", "foo", Some("https://proxy.example.com"));
        assert_eq!(
            url,
            "https://proxy.example.com/model/foo/invoke-with-response-stream"
        );
    }

    #[test]
    fn count_tokens_url_uses_region_and_model() {
        let url = build_count_tokens_url(
            "eu-west-1",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            None,
        );
        assert_eq!(
            url,
            "https://bedrock-runtime.eu-west-1.amazonaws.com/model/us.anthropic.claude-sonnet-4-5-20250929-v1%3A0/count-tokens"
        );
    }

    #[test]
    fn count_tokens_url_override_from_base_url() {
        let url = build_count_tokens_url("us-east-1", "foo", Some("https://proxy.example.com/"));
        assert_eq!(url, "https://proxy.example.com/model/foo/count-tokens");
    }

    #[test]
    fn body_strips_stream_and_model_adds_anthropic_version() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-5-20250929".to_string(),
            messages: vec![json!({"role":"user","content":"hi"})],
            system: None,
            max_tokens: 128,
            tools: None,
            stream: true,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking: None,
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: None,
        };
        let raw = to_bedrock_body(&req).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["anthropic_version"], BEDROCK_ANTHROPIC_VERSION);
        assert_eq!(v["max_tokens"], 128);
        assert!(v.get("model").is_none(), "model must not be in body");
        assert!(v.get("stream").is_none(), "stream must not be in body");
    }

    #[test]
    fn count_tokens_body_wraps_invoke_model_body() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-5-20250929".to_string(),
            messages: vec![json!({"role":"user","content":"hi"})],
            system: Some(vec![json!({"type":"text","text":"Be brief."})]),
            max_tokens: 128,
            tools: Some(vec![json!({
                "name": "Read",
                "description": "",
                "input_schema": {"type": "object"}
            })]),
            stream: true,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking: None,
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: Some("advisor".to_string()),
        };

        let raw = to_bedrock_count_tokens_body(&req).unwrap();
        let outer: Value = serde_json::from_slice(&raw).unwrap();
        let encoded = outer["input"]["invokeModel"]["body"]
            .as_str()
            .expect("CountTokens body must contain encoded InvokeModel body");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let inner: Value = serde_json::from_slice(&decoded).unwrap();

        assert_eq!(inner["anthropic_version"], BEDROCK_ANTHROPIC_VERSION);
        assert_eq!(inner["messages"][0]["content"], "hi");
        assert_eq!(inner["system"][0]["text"], "Be brief.");
        assert!(inner.get("tools").is_some());
        assert!(inner.get("model").is_none());
        assert!(inner.get("stream").is_none());
        assert!(inner.get("advisor_model").is_none());
    }

    #[test]
    fn eventstream_chunk_decodes_to_stream_event() {
        let payload = br#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        let frame = encode_eventstream_message(
            &[
                (":message-type", "event"),
                (":event-type", "chunk"),
                (":content-type", "application/json"),
            ],
            payload,
        );

        let decoded = decode_eventstream_message(&frame).unwrap();
        let events = bedrock_message_to_stream_events(decoded).unwrap();

        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(*index, 0);
                assert_eq!(delta["text"], "hi");
            }
            other => panic!("expected content block delta, got {other:?}"),
        }
    }

    #[test]
    fn eventstream_base64_wrapper_decodes_to_stream_event() {
        let inner = br#"{"type":"message_stop"}"#;
        let payload = json!({
            "bytes": base64::engine::general_purpose::STANDARD.encode(inner),
        })
        .to_string();
        let frame = encode_eventstream_message(
            &[
                (":message-type", "event"),
                (":event-type", "chunk"),
                (":content-type", "application/json"),
            ],
            payload.as_bytes(),
        );

        let decoded = decode_eventstream_message(&frame).unwrap();
        let events = bedrock_message_to_stream_events(decoded).unwrap();

        assert!(matches!(events.as_slice(), [StreamEvent::MessageStop]));
    }

    #[test]
    fn eventstream_rejects_bad_crc() {
        let mut frame = encode_eventstream_message(
            &[(":message-type", "event"), (":event-type", "chunk")],
            br#"{"type":"message_stop"}"#,
        );
        let last = frame.len() - 1;
        frame[last] ^= 0xff;

        let err = decode_eventstream_message(&frame).unwrap_err().to_string();
        assert!(err.contains("CRC mismatch"));
    }

    fn encode_eventstream_message(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
        let mut header_bytes = Vec::new();
        for (name, value) in headers {
            header_bytes.push(name.len() as u8);
            header_bytes.extend_from_slice(name.as_bytes());
            header_bytes.push(7);
            header_bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            header_bytes.extend_from_slice(value.as_bytes());
        }

        let total_len = 12 + header_bytes.len() + payload.len() + 4;
        let mut frame = Vec::with_capacity(total_len);
        frame.extend_from_slice(&(total_len as u32).to_be_bytes());
        frame.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
        let prelude_crc = crc32(&frame);
        frame.extend_from_slice(&prelude_crc.to_be_bytes());
        frame.extend_from_slice(&header_bytes);
        frame.extend_from_slice(payload);
        let message_crc = crc32(&frame);
        frame.extend_from_slice(&message_crc.to_be_bytes());
        frame
    }

    #[test]
    fn auth_from_env_prefers_bearer_token() {
        let saved_bearer = std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok();
        let saved_ak = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let saved_sk = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
        std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", "bedrock-key-123");
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIA");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "secret");

        match BedrockAuth::from_env() {
            Some(BedrockAuth::BearerToken(t)) => assert_eq!(t, "bedrock-key-123"),
            other => panic!("expected BearerToken, got {:?}", other),
        }

        std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_SECRET_ACCESS_KEY");
        if let Some(v) = saved_bearer {
            std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", v);
        }
        if let Some(v) = saved_ak {
            std::env::set_var("AWS_ACCESS_KEY_ID", v);
        }
        if let Some(v) = saved_sk {
            std::env::set_var("AWS_SECRET_ACCESS_KEY", v);
        }
    }
}
