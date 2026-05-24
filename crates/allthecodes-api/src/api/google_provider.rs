//! Google Gemini provider 鈥?streamGenerateContent API.
//!
//! Auth: `GOOGLE_API_KEY` passed as `?key=` query parameter.
//! Endpoint: `{base}/models/{model}:streamGenerateContent?key=...&alt=sse`
//!
//! Response format differs from OpenAI: each SSE chunk is a full JSON object
//! containing Gemini parts, which are adapted into Anthropic-style stream
//! events for the rest of the runtime.
//!
//! Reference: code-iris/crates/iris-llm/src/google.rs

use std::collections::HashMap;
use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::client::{strip_anthropic_cache_fields, MessagesRequest};
use allthecodes_types::message::{ContentBlock, MessageDelta, StreamEvent, Usage};

// ---------------------------------------------------------------------------
// Gemini response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    #[serde(rename = "role")]
    _role: Option<String>,
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: Option<String>,
    args: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u64,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u64,
    #[serde(rename = "thoughtsTokenCount", default)]
    thoughts_token_count: u64,
}

// ---------------------------------------------------------------------------
// Message format conversion (Anthropic 鈫?Gemini)
// ---------------------------------------------------------------------------

/// Convert MessagesRequest (Anthropic format) to Gemini request body.
///
/// Key differences:
/// - Gemini uses "model" role instead of "assistant"
/// - System prompt goes in `system_instruction` field
/// - Consecutive same-role messages must be merged (Gemini requires alternating turns)
pub(crate) fn build_gemini_request(request: &MessagesRequest) -> Value {
    let mut contents: Vec<Value> = Vec::new();
    let mut tool_names_by_id = HashMap::<String, String>::new();
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let messages = messages.as_array().cloned().unwrap_or_default();

    for msg in &messages {
        let role_str = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let role = match role_str {
            "assistant" => "model",
            _ => "user",
        };

        let parts = match msg.get("content") {
            Some(content) if role == "model" => {
                gemini_parts_from_assistant_content(content, &mut tool_names_by_id)
            }
            Some(content) => gemini_parts_from_user_content(content, &tool_names_by_id),
            None => Vec::new(),
        };

        if !parts.is_empty() {
            contents.push(json!({
                "role": role,
                "parts": parts
            }));
        }
    }

    // Merge consecutive same-role messages (Gemini requires alternating turns)
    let mut merged: Vec<Value> = Vec::new();
    for item in contents {
        let role = item["role"].as_str().unwrap_or("").to_string();
        if let Some(last) = merged.last_mut() {
            if last["role"].as_str() == Some(&role) {
                if let (Some(last_parts), Some(new_parts)) =
                    (last["parts"].as_array_mut(), item["parts"].as_array())
                {
                    last_parts.extend(new_parts.iter().cloned());
                }
                continue;
            }
        }
        merged.push(item);
    }

    let mut body = json!({
        "contents": merged,
        "generationConfig": {
            "maxOutputTokens": request.max_tokens,
        }
    });

    if let Some(thinking) = gemini_thinking_config(request.thinking.as_ref()) {
        body["generationConfig"]["thinkingConfig"] = thinking;
    }

    let mut tools = request.tools.clone().map(Value::Array);
    if let Some(value) = tools.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let tools = tools.and_then(|value| value.as_array().cloned());
    if let Some(tools) = gemini_tools_from_anthropic(tools.as_deref()) {
        body["tools"] = tools;
    }

    if let Some(tool_choice) = gemini_tool_choice(request.tool_choice.as_ref()) {
        body["toolConfig"] = json!({
            "functionCallingConfig": tool_choice
        });
    }

    // System instruction
    let mut system = request.system.clone().map(Value::Array);
    if let Some(value) = system.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let system = system.and_then(|value| value.as_array().cloned());
    if let Some(system) = &system {
        let system_text: String = system
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    block.as_str().map(|s| s.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !system_text.is_empty() {
            body["system_instruction"] = json!({
                "parts": [{"text": system_text}]
            });
        }
    }

    body
}

pub(crate) fn build_gemini_count_tokens_request(request: &MessagesRequest) -> Value {
    json!({
        "generateContentRequest": build_gemini_request(request),
    })
}

fn gemini_parts_from_user_content(
    content: &Value,
    tool_names_by_id: &HashMap<String, String>,
) -> Vec<Value> {
    match content {
        Value::String(text) => text_part(text).into_iter().collect(),
        Value::Array(blocks) => blocks
            .iter()
            .flat_map(|block| match block {
                Value::String(text) => text_part(text).into_iter().collect(),
                Value::Object(_) if block.get("type").and_then(|v| v.as_str()) == Some("text") => {
                    block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .and_then(text_part)
                        .into_iter()
                        .collect()
                }
                Value::Object(_)
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") =>
                {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let name = tool_names_by_id
                        .get(tool_use_id)
                        .cloned()
                        .unwrap_or_else(|| tool_use_id.to_string());
                    let response = tool_result_response_object(
                        block.get("content").unwrap_or(&Value::Null),
                        block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    );
                    vec![json!({
                        "functionResponse": {
                            "name": name,
                            "response": response,
                        }
                    })]
                }
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn gemini_parts_from_assistant_content(
    content: &Value,
    tool_names_by_id: &mut HashMap<String, String>,
) -> Vec<Value> {
    match content {
        Value::String(text) => text_part(text).into_iter().collect(),
        Value::Array(blocks) => blocks
            .iter()
            .flat_map(|block| match block {
                Value::String(text) => text_part(text).into_iter().collect(),
                Value::Object(_) if block.get("type").and_then(|v| v.as_str()) == Some("text") => {
                    block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .and_then(text_part)
                        .into_iter()
                        .collect()
                }
                Value::Object(_)
                    if block.get("type").and_then(|v| v.as_str()) == Some("thinking") =>
                {
                    let Some(text) = block.get("thinking").and_then(|v| v.as_str()) else {
                        return Vec::new();
                    };
                    if text.is_empty() {
                        return Vec::new();
                    }
                    let mut part = json!({
                        "text": text,
                        "thought": true,
                    });
                    if let Some(signature) = block.get("signature").and_then(|v| v.as_str()) {
                        part["thoughtSignature"] = json!(signature);
                    }
                    vec![part]
                }
                Value::Object(_)
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") =>
                {
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                        tool_names_by_id.insert(id.to_string(), name.clone());
                    }
                    vec![json!({
                        "functionCall": {
                            "name": name,
                            "args": normalize_tool_use_input(block.get("input")),
                        }
                    })]
                }
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn text_part(text: &str) -> Option<Value> {
    if text.is_empty() {
        None
    } else {
        Some(json!({ "text": text }))
    }
}

fn normalize_tool_use_input(input: Option<&Value>) -> Value {
    match input {
        Some(Value::Object(_)) => input.cloned().unwrap_or_else(|| json!({})),
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .ok()
            .filter(|v| v.is_object())
            .unwrap_or_else(|| json!({ "value": s })),
        Some(Value::Null) | None => json!({}),
        Some(other) => json!({ "value": other }),
    }
}

fn tool_result_response_object(content: &Value, is_error: bool) -> Value {
    let normalized = normalize_tool_result_content(content);
    let mut response = match normalized {
        Value::Object(map) => Value::Object(map),
        other => json!({ "result": other }),
    };
    if is_error {
        if let Value::Object(map) = &mut response {
            map.insert("is_error".to_string(), Value::Bool(true));
        }
    }
    response
}

fn normalize_tool_result_content(content: &Value) -> Value {
    match content {
        Value::String(text) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.clone()))
        }
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(_) => part
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))
        }
        other => other.clone(),
    }
}

fn gemini_tools_from_anthropic(tools: Option<&[Value]>) -> Option<Value> {
    let declarations: Vec<Value> = tools?
        .iter()
        .filter(|tool| {
            !matches!(
                tool.get("type").and_then(|v| v.as_str()),
                Some("server" | "advisor_20260301" | "computer_20250124")
            )
        })
        .map(|tool| {
            let parameters = tool
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            json!({
                "name": tool.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
                "description": tool.get("description").and_then(|v| v.as_str()).unwrap_or_default(),
                "parametersJsonSchema": parameters,
            })
        })
        .collect();

    if declarations.is_empty() {
        None
    } else {
        Some(json!([{ "functionDeclarations": declarations }]))
    }
}

fn gemini_tool_choice(tool_choice: Option<&Value>) -> Option<Value> {
    let choice = tool_choice?;
    match choice.get("type").and_then(|v| v.as_str()) {
        Some("auto") => Some(json!({ "mode": "AUTO" })),
        Some("any") => Some(json!({ "mode": "ANY" })),
        Some("tool") => {
            let mut out = json!({ "mode": "ANY" });
            if let Some(name) = choice.get("name").and_then(|v| v.as_str()) {
                out["allowedFunctionNames"] = json!([name]);
            }
            Some(out)
        }
        _ => None,
    }
}

fn gemini_thinking_config(thinking: Option<&Value>) -> Option<Value> {
    let thinking = thinking?;
    if thinking.get("type").and_then(|v| v.as_str()) == Some("disabled") {
        return None;
    }

    let mut config = json!({ "includeThoughts": true });
    if let Some(tokens) = thinking.get("budget_tokens").and_then(|v| v.as_u64()) {
        config["thinkingBudget"] = json!(tokens);
    }
    Some(config)
}

// ---------------------------------------------------------------------------
// Streaming implementation
// ---------------------------------------------------------------------------

/// Send a streaming request to Google Gemini API.
pub(crate) async fn google_stream(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &MessagesRequest,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    let url = format!(
        "{}/models/{}:streamGenerateContent?key={}&alt=sse",
        base_url.trim_end_matches('/'),
        request.model,
        api_key,
    );

    let body = build_gemini_request(request);

    let response = http
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("failed to send Google Gemini request")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!("Google Gemini error (HTTP {}): {}", status, error_body);
    }

    let stream = parse_gemini_sse_byte_stream(response.bytes_stream());
    Ok(Box::pin(stream))
}

/// Count input tokens using Gemini's `models.countTokens` endpoint.
pub(crate) async fn google_count_tokens(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &MessagesRequest,
) -> Result<u64> {
    #[derive(Deserialize)]
    struct CountTokensResponse {
        #[serde(rename = "totalTokens")]
        total_tokens: u64,
    }

    let url = format!(
        "{}/models/{}:countTokens?key={}",
        base_url.trim_end_matches('/'),
        request.model,
        api_key,
    );
    let body = build_gemini_count_tokens_request(request);
    let response = http
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("failed to send Google Gemini countTokens request")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Google Gemini countTokens error (HTTP {}): {}",
            status,
            error_body
        );
    }

    let parsed: CountTokensResponse = response
        .json()
        .await
        .context("failed to parse Google Gemini countTokens response")?;
    Ok(parsed.total_tokens)
}

/// Parse a Gemini SSE byte stream into StreamEvent values.
///
/// Gemini chunks can carry text, thinking, and function-call parts. Text-like
/// parts are accepted as either deltas or cumulative chunks; cumulative chunks
/// are diffed against the text already emitted for the active block.
///
/// Emits Anthropic-style StreamEvent sequence:
///   MessageStart -> ContentBlockStart -> ContentBlockDelta* ->
///   ContentBlockStop -> MessageDelta -> MessageStop
fn parse_gemini_sse_byte_stream<S>(byte_stream: S) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = String::new();
        let mut started = false;
        let mut next_content_index = 0usize;
        let mut open_text_like_block: Option<(usize, TextLikeBlockType)> = None;
        let mut emitted_text_by_block = HashMap::<usize, String>::new();
        let mut saw_tool_use = false;
        let mut finish_reason: Option<String> = None;
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = chunk_result.context("error reading Gemini response chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                let data = if let Some(rest) = line.strip_prefix("data:") {
                    rest.trim()
                } else {
                    continue;
                };

                if data == "[DONE]" || data.is_empty() {
                    continue;
                }

                let gemini_resp: GeminiResponse = match serde_json::from_str(data) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if let Some(ref usage) = gemini_resp.usage_metadata {
                    input_tokens = usage.prompt_token_count;
                    output_tokens = usage.candidates_token_count + usage.thoughts_token_count;
                }

                if !started {
                    started = true;
                    yield StreamEvent::MessageStart {
                        usage: Usage {
                            input_tokens,
                            output_tokens: 0,
                            ..Usage::default()
                        },
                    };
                }

                for candidate in gemini_resp.candidates.unwrap_or_default() {
                    if let Some(reason) = candidate.finish_reason {
                        finish_reason = Some(reason);
                    }

                    if let Some(content) = candidate.content {
                        for part in content.parts.unwrap_or_default() {
                            if let Some(function_call) = part.function_call {
                                if let Some((index, _)) = open_text_like_block.take() {
                                    yield StreamEvent::ContentBlockStop { index };
                                }

                                saw_tool_use = true;
                                let tool_index = next_content_index;
                                next_content_index += 1;
                                let tool_id = format!(
                                    "toolu_{}",
                                    Uuid::new_v4()
                                        .simple()
                                        .to_string()
                                        .chars()
                                        .take(24)
                                        .collect::<String>()
                                );

                                yield StreamEvent::ContentBlockStart {
                                    index: tool_index,
                                    content_block: ContentBlock::ToolUse {
                                        id: tool_id,
                                        name: function_call.name.unwrap_or_default(),
                                        input: json!({}),
                                    },
                                };

                                if let Some(args) = function_call.args.filter(|v| !v.is_null()) {
                                    yield StreamEvent::ContentBlockDelta {
                                        index: tool_index,
                                        delta: json!({
                                            "type": "input_json_delta",
                                            "partial_json": serde_json::to_string(&args).unwrap_or_default(),
                                        }),
                                    };
                                }

                                yield StreamEvent::ContentBlockStop { index: tool_index };
                                continue;
                            }

                            if let Some(full_text) = part.text {
                                let block_type = if part.thought {
                                    TextLikeBlockType::Thinking
                                } else {
                                    TextLikeBlockType::Text
                                };

                                if open_text_like_block.map(|(_, ty)| ty) != Some(block_type) {
                                    if let Some((index, _)) = open_text_like_block.take() {
                                        yield StreamEvent::ContentBlockStop { index };
                                    }

                                    let index = next_content_index;
                                    next_content_index += 1;
                                    open_text_like_block = Some((index, block_type));
                                    emitted_text_by_block.insert(index, String::new());

                                    yield StreamEvent::ContentBlockStart {
                                        index,
                                        content_block: match block_type {
                                            TextLikeBlockType::Text => ContentBlock::Text {
                                                text: String::new(),
                                            },
                                            TextLikeBlockType::Thinking => ContentBlock::Thinking {
                                                thinking: String::new(),
                                                signature: None,
                                            },
                                        },
                                    };
                                }

                                let (index, _) =
                                    open_text_like_block.expect("text-like block is open");
                                if let Some(delta_text) = text_delta_for_block(
                                    index,
                                    &full_text,
                                    &mut emitted_text_by_block,
                                ) {
                                    yield StreamEvent::ContentBlockDelta {
                                        index,
                                        delta: match block_type {
                                            TextLikeBlockType::Text => {
                                                json!({"type": "text_delta", "text": delta_text})
                                            }
                                            TextLikeBlockType::Thinking => {
                                                json!({"type": "thinking_delta", "thinking": delta_text})
                                            }
                                        },
                                    };
                                }

                                if let (Some(signature), TextLikeBlockType::Thinking) =
                                    (part.thought_signature, block_type)
                                {
                                    yield StreamEvent::ContentBlockDelta {
                                        index,
                                        delta: json!({
                                            "type": "signature_delta",
                                            "signature": signature,
                                        }),
                                    };
                                }
                            } else if let (
                                Some(signature),
                                Some((index, TextLikeBlockType::Thinking)),
                            ) = (part.thought_signature, open_text_like_block)
                            {
                                yield StreamEvent::ContentBlockDelta {
                                    index,
                                    delta: json!({
                                        "type": "signature_delta",
                                        "signature": signature,
                                    }),
                                };
                            }
                        }
                    }
                }
            }
        }

        if !started {
            return;
        }

        if let Some((index, _)) = open_text_like_block.take() {
            yield StreamEvent::ContentBlockStop { index };
        }

        yield StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some(map_gemini_finish_reason(
                    finish_reason.as_deref(),
                    saw_tool_use,
                )),
            },
            usage: Some(Usage {
                output_tokens,
                ..Usage::default()
            }),
        };
        yield StreamEvent::MessageStop;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextLikeBlockType {
    Text,
    Thinking,
}

fn text_delta_for_block(
    index: usize,
    text: &str,
    emitted_text_by_block: &mut HashMap<usize, String>,
) -> Option<String> {
    let emitted = emitted_text_by_block.entry(index).or_default();
    if text.is_empty() {
        return None;
    }

    let delta = if text.starts_with(emitted.as_str()) {
        if text.len() == emitted.len() {
            return None;
        }
        let suffix = text[emitted.len()..].to_string();
        *emitted = text.to_string();
        suffix
    } else {
        emitted.push_str(text);
        text.to_string()
    };

    if delta.is_empty() {
        None
    } else {
        Some(delta)
    }
}

fn map_gemini_finish_reason(reason: Option<&str>, saw_tool_use: bool) -> String {
    match reason {
        Some("MAX_TOKENS") => "max_tokens",
        _ if saw_tool_use => "tool_use",
        _ => "end_turn",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn test_build_gemini_request_basic() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({"role": "user", "content": "Hello"})],
            system: None,
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 1024);

        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_build_gemini_request_with_system() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({"role": "user", "content": "Hello"})],
            system: Some(vec![json!({"type": "text", "text": "Be helpful."})]),
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        assert_eq!(
            body["system_instruction"]["parts"][0]["text"],
            "Be helpful."
        );
    }

    #[test]
    fn test_build_gemini_request_strips_anthropic_cache_fields() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "Hello",
                    "cache_control": {"type": "ephemeral"}
                }]
            })],
            system: Some(vec![json!({
                "type": "text",
                "text": "Be helpful.",
                "cache_control": {"type": "ephemeral"}
            })]),
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        assert!(serde_json::to_string(&body)
            .unwrap()
            .find("cache_")
            .is_none());
        assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_build_gemini_count_tokens_request_wraps_generate_content_request() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({"role": "user", "content": "Hello"})],
            system: Some(vec![json!({"type": "text", "text": "Be helpful."})]),
            max_tokens: 1024,
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

        let body = build_gemini_count_tokens_request(&req);

        assert!(body.get("generateContentRequest").is_some());
        assert_eq!(
            body["generateContentRequest"]["contents"][0]["parts"][0]["text"],
            "Hello"
        );
        assert_eq!(
            body["generateContentRequest"]["system_instruction"]["parts"][0]["text"],
            "Be helpful."
        );
    }

    #[test]
    fn test_build_gemini_request_role_mapping() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![
                json!({"role": "user", "content": "Hi"}),
                json!({"role": "assistant", "content": "Hello!"}),
                json!({"role": "user", "content": "How are you?"}),
            ],
            system: None,
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model"); // "assistant" 鈫?"model"
        assert_eq!(contents[2]["role"], "user");
    }

    #[test]
    fn test_build_gemini_request_merges_consecutive_roles() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![
                json!({"role": "user", "content": "Hello"}),
                json!({"role": "user", "content": "How are you?"}),
                json!({"role": "assistant", "content": "Fine."}),
            ],
            system: None,
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        let contents = body["contents"].as_array().unwrap();
        // Two user messages should be merged into one
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
        assert_eq!(contents[0]["parts"][1]["text"], "How are you?");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn test_build_gemini_request_content_blocks() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": "Part A"},
                    {"type": "text", "text": "Part B"},
                ]
            })],
            system: None,
            max_tokens: 1024,
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
        let body = build_gemini_request(&req);
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["parts"][0]["text"], "Part A");
        assert_eq!(contents[0]["parts"][1]["text"], "Part B");
    }

    #[test]
    fn test_build_gemini_request_with_tools_thinking_and_tool_choice() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![json!({"role": "user", "content": "Use the tool"})],
            system: None,
            max_tokens: 1024,
            tools: Some(vec![json!({
                "name": "Read",
                "description": "Read a file",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    }
                }
            })]),
            stream: true,
            metadata: None,
            service_tier: None,
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            context_management: None,
            thinking: Some(json!({"type": "enabled", "budget_tokens": 512})),
            output_config: None,
            tool_choice: Some(json!({"type": "tool", "name": "Read"})),
            reasoning_effort: None,
            advisor_model: None,
        };

        let body = build_gemini_request(&req);

        assert_eq!(
            body["generationConfig"]["thinkingConfig"],
            json!({"includeThoughts": true, "thinkingBudget": 512})
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["properties"]
                ["path"]["type"],
            "string"
        );
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"],
            json!({"mode": "ANY", "allowedFunctionNames": ["Read"]})
        );
    }

    #[test]
    fn test_build_gemini_request_round_trips_tool_use_and_result() {
        let req = MessagesRequest {
            model: "gemini-2.0-flash".to_string(),
            messages: vec![
                json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "Read",
                        "input": {"path": "Cargo.toml"}
                    }]
                }),
                json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "content": "{\"ok\":true}"
                    }]
                }),
            ],
            system: None,
            max_tokens: 1024,
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

        let body = build_gemini_request(&req);
        let contents = body["contents"].as_array().unwrap();

        assert_eq!(
            contents[0]["parts"][0]["functionCall"],
            json!({"name": "Read", "args": {"path": "Cargo.toml"}})
        );
        assert_eq!(
            contents[1]["parts"][0]["functionResponse"],
            json!({"name": "Read", "response": {"ok": true}})
        );
    }

    #[tokio::test]
    async fn test_parse_gemini_stream_emits_thinking_and_tool_use() {
        let payload = concat!(
            "data: {\"usageMetadata\":{\"promptTokenCount\":5},",
            "\"candidates\":[{\"content\":{\"parts\":[",
            "{\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"sig\"}",
            "]}}]}\n\n",
            "data: {\"usageMetadata\":{\"candidatesTokenCount\":2,\"thoughtsTokenCount\":3},",
            "\"candidates\":[{\"content\":{\"parts\":[",
            "{\"functionCall\":{\"name\":\"Read\",\"args\":{\"path\":\"Cargo.toml\"}}}",
            "]},\"finishReason\":\"STOP\"}]}\n\n"
        );
        let input =
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(payload))]);

        let events: Vec<_> = parse_gemini_sse_byte_stream(input)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();

        assert!(matches!(
            events.first(),
            Some(StreamEvent::MessageStart {
                usage: Usage {
                    input_tokens: 5,
                    ..
                }
            })
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Thinking { .. }
            }
        ));
        assert!(matches!(
            &events[2],
            StreamEvent::ContentBlockDelta { index: 0, delta }
                if delta == &json!({"type": "thinking_delta", "thinking": "plan"})
        ));
        assert!(matches!(
            &events[3],
            StreamEvent::ContentBlockDelta { index: 0, delta }
                if delta == &json!({"type": "signature_delta", "signature": "sig"})
        ));
        assert!(matches!(
            &events[5],
            StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlock::ToolUse { name, .. }
            } if name == "Read"
        ));
        assert!(matches!(
            &events[6],
            StreamEvent::ContentBlockDelta { index: 1, delta }
                if delta["type"] == "input_json_delta"
                    && delta["partial_json"].as_str().unwrap().contains("Cargo.toml")
        ));
        assert!(matches!(
            events.iter().find_map(|event| match event {
                StreamEvent::MessageDelta { delta, usage } => {
                    Some((
                        delta.stop_reason.as_deref(),
                        usage.as_ref().map(|u| u.output_tokens),
                    ))
                }
                _ => None,
            }),
            Some((Some("tool_use"), Some(5)))
        ));
        assert!(matches!(events.last(), Some(StreamEvent::MessageStop)));
    }
}
