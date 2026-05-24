//! OpenAI-compatible provider 鈥?handles all providers using the
//! OpenAI chat/completions API format.
//!
//! Covers: OpenAI, DeepSeek, Groq, OpenRouter, Qwen, Zhipu, Moonshot,
//! Baichuan, MiniMax, Yi, SiliconFlow, StepFun, Spark.
//!
//! Converts our internal MessagesRequest (Anthropic format) to OpenAI format,
//! sends the streaming request, and parses the SSE response back into our
//! StreamEvent type so the rest of the system (StreamAccumulator, etc.) works
//! unchanged.
//!
//! Reference: code-iris/crates/iris-llm/src/openai.rs

use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;
use serde_json::{json, Value};

use crate::api::client::{
    build_openai_compat_url, is_openai_codex_provider, strip_anthropic_cache_fields,
    MessagesRequest,
};
use allthecodes_types::message::{ContentBlock, MessageDelta, StreamEvent, Usage};

// ---------------------------------------------------------------------------
// Message format conversion (Anthropic 鈫?OpenAI)
// ---------------------------------------------------------------------------

fn reasoning_output_tokens_from_usage(usage: &Value) -> u64 {
    usage
        .get("completion_tokens_details")
        .or_else(|| usage.get("output_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

/// Extract text from Anthropic system prompt blocks.
///
/// System blocks look like: `[{"type": "text", "text": "Be helpful."}]`
fn extract_system_text(system: &[Value]) -> String {
    system
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
        .join("\n")
}

/// Flatten Anthropic content (array of blocks or string) to a single string.
///
/// Anthropic: `{"content": [{"type":"text","text":"Hello"}]}` or `{"content": "Hello"}`
/// OpenAI:    `{"content": "Hello"}`
fn flatten_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string()),
                Some("tool_result") => block
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn content_items_for_responses(content: Option<&Value>, output: bool) -> Vec<Value> {
    let item_type = if output { "output_text" } else { "input_text" };
    let text = flatten_content(content);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": item_type, "text": text })]
    }
}

fn build_responses_input(request: &MessagesRequest) -> Vec<Value> {
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let messages = messages.as_array().cloned().unwrap_or_default();
    let mut input = Vec::new();

    for msg in &messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");

        if role == "assistant" {
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    text_parts.push(text.to_string());
                                }
                            }
                        }
                        Some("tool_use") => {
                            let call_id = block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("call_unknown");
                            let name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("unknown");
                            let arguments = block
                                .get("input")
                                .cloned()
                                .unwrap_or_else(|| json!({}))
                                .to_string();
                            input.push(json!({
                                "type": "function_call",
                                "name": name,
                                "arguments": arguments,
                                "call_id": call_id,
                            }));
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": text_parts.join("\n"),
                        }],
                    }));
                }
            } else {
                let content = content_items_for_responses(content, true);
                if !content.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": content,
                    }));
                }
            }
        } else if role == "user" {
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("tool_result") => {
                            let call_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("call_unknown");
                            let output = block
                                .get("content")
                                .map(|c| match c {
                                    Value::String(s) => s.clone(),
                                    Value::Array(arr) => arr
                                        .iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    other => other.to_string(),
                                })
                                .unwrap_or_default();
                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output,
                            }));
                        }
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    text_parts.push(text.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": text_parts.join("\n"),
                        }],
                    }));
                }
            } else {
                let content = content_items_for_responses(content, false);
                if !content.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content,
                    }));
                }
            }
        } else {
            let content = content_items_for_responses(content, false);
            if !content.is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": role,
                    "content": content,
                }));
            }
        }
    }

    input
}

fn build_responses_tools(request: &MessagesRequest) -> Vec<Value> {
    let mut tools = request.tools.clone().map(Value::Array);
    if let Some(value) = tools.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let tools = tools.and_then(|value| value.as_array().cloned());
    tools
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?;
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let parameters = sanitize_responses_function_parameters(
                tool.get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            )?;
            Some(json!({
                "type": "function",
                "name": name,
                "description": description,
                "parameters": parameters,
            }))
        })
        .collect()
}

fn sanitize_responses_function_parameters(parameters: Value) -> Option<Value> {
    let object = parameters.as_object()?;
    if object.get("type").and_then(|v| v.as_str()) != Some("object") {
        return None;
    }
    if ["oneOf", "anyOf", "allOf", "enum", "not"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return None;
    }
    Some(parameters)
}

fn build_codex_responses_request(request: &MessagesRequest) -> Value {
    let mut system = request.system.clone().map(Value::Array);
    if let Some(value) = system.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let instructions = system
        .and_then(|value| value.as_array().cloned())
        .map(|system| extract_system_text(&system))
        .unwrap_or_default();
    let tools = build_responses_tools(request);

    let mut body = json!({
        "model": request.model,
        "instructions": instructions,
        "input": build_responses_input(request),
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": !tools.is_empty(),
        "store": false,
        "stream": true,
        "include": [],
    });
    if let Some(effort) = request
        .reasoning_effort
        .as_deref()
        .and_then(normalize_codex_reasoning_effort)
    {
        body["reasoning"] = json!({ "effort": effort });
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    body
}

fn normalize_codex_reasoning_effort(effort: &str) -> Option<&str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "none" => Some("none"),
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        _ => None,
    }
}

/// Convert a MessagesRequest (Anthropic format) to OpenAI chat completions body.
///
/// `provider_name` is used to select the correct token-limit parameter:
/// Azure OpenAI and OpenAI newer models require `max_completion_tokens`
/// instead of the legacy `max_tokens`.
fn build_openai_request(request: &MessagesRequest, provider_name: &str) -> Value {
    if is_openai_codex_provider(provider_name) {
        return build_codex_responses_request(request);
    }

    let is_deepseek_provider = provider_name.eq_ignore_ascii_case("deepseek");
    let mut oai_messages: Vec<Value> = Vec::new();
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let messages = messages.as_array().cloned().unwrap_or_default();

    // System prompt 鈫?system message
    let mut system = request.system.clone().map(Value::Array);
    if let Some(value) = system.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let system = system.and_then(|value| value.as_array().cloned());
    if let Some(system) = &system {
        let text = extract_system_text(system);
        if !text.is_empty() {
            oai_messages.push(json!({"role": "system", "content": text}));
        }
    }

    // User/assistant messages 鈥?convert to OpenAI format.
    // Handles: text messages, assistant tool_use blocks, user tool_result blocks.
    for msg in &messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");

        if role == "assistant" {
            // Check if assistant message contains tool_use blocks
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts: Vec<String> = Vec::new();
                let mut reasoning_parts: Vec<String> = Vec::new();
                let mut has_reasoning_content = false;
                let mut tool_calls_out: Vec<Value> = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                if !t.is_empty() {
                                    text_parts.push(t.to_string());
                                }
                            }
                        }
                        Some("thinking") => {
                            if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                                has_reasoning_content = true;
                                reasoning_parts.push(t.to_string());
                            }
                        }
                        Some("tool_use") => {
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let empty_obj = json!({});
                            let input = block.get("input").unwrap_or(&empty_obj);
                            tool_calls_out.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": input.to_string(),
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                let content_val = if text_parts.is_empty() {
                    Value::Null
                } else {
                    Value::String(text_parts.join("\n"))
                };

                if tool_calls_out.is_empty() {
                    if !text_parts.is_empty() {
                        let mut assistant_msg =
                            json!({"role": "assistant", "content": content_val});
                        if is_deepseek_provider && has_reasoning_content {
                            assistant_msg["reasoning_content"] = json!(reasoning_parts.join("\n"));
                        }
                        oai_messages.push(assistant_msg);
                    } else if is_deepseek_provider && has_reasoning_content {
                        oai_messages.push(json!({
                            "role": "assistant",
                            "content": "",
                            "reasoning_content": reasoning_parts.join("\n"),
                        }));
                    }
                } else {
                    let mut assistant_msg = json!({"role": "assistant"});
                    if !text_parts.is_empty() {
                        assistant_msg["content"] = content_val;
                    } else if is_deepseek_provider {
                        assistant_msg["content"] = json!("");
                    }
                    if is_deepseek_provider && has_reasoning_content {
                        assistant_msg["reasoning_content"] = json!(reasoning_parts.join("\n"));
                    }
                    assistant_msg["tool_calls"] = json!(tool_calls_out);
                    oai_messages.push(assistant_msg);
                }
            } else {
                let text = flatten_content(content);
                if !text.is_empty() {
                    oai_messages.push(json!({"role": "assistant", "content": text}));
                }
            }
        } else if role == "user" {
            // Check for tool_result blocks 鈫?convert to OpenAI "tool" role messages
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts: Vec<String> = Vec::new();
                let mut tool_results: Vec<(String, String)> = Vec::new();

                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("tool_result") => {
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string();
                            let result_content = block
                                .get("content")
                                .map(|c| match c {
                                    Value::String(s) => s.clone(),
                                    Value::Array(arr) => arr
                                        .iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    other => other.to_string(),
                                })
                                .unwrap_or_default();
                            tool_results.push((tool_use_id, result_content));
                        }
                        Some("text") => {
                            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                if !t.is_empty() {
                                    text_parts.push(t.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Emit tool result messages first
                for (tool_use_id, result) in tool_results {
                    oai_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": result,
                    }));
                }
                // Then any text parts as a regular user message
                if !text_parts.is_empty() {
                    oai_messages.push(json!({"role": "user", "content": text_parts.join("\n")}));
                }
            } else {
                let text = flatten_content(content);
                if !text.is_empty() {
                    oai_messages.push(json!({"role": "user", "content": text}));
                }
            }
        } else {
            let text = flatten_content(content);
            if !text.is_empty() {
                oai_messages.push(json!({"role": role, "content": text}));
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": oai_messages,
        "stream": true,
    });
    body["stream_options"] = json!({ "include_usage": true });

    if request.max_tokens > 0 {
        // Azure OpenAI and OpenAI newer models (gpt-4o, o1, o3, gpt-5, etc.)
        // require `max_completion_tokens`; legacy `max_tokens` is rejected.
        let uses_new_param = provider_name.eq_ignore_ascii_case("azure")
            || provider_name.eq_ignore_ascii_case("openai")
            || request.model.starts_with("gpt-4o")
            || request.model.starts_with("gpt-5")
            || request.model.starts_with("o1")
            || request.model.starts_with("o3")
            || request.model.starts_with("o4");
        let key = if uses_new_param {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        body[key] = json!(request.max_tokens);
    }

    // Convert Anthropic-format tools to OpenAI function-calling format.
    // Anthropic: {"name": "X", "description": "...", "input_schema": {...}}
    // OpenAI:    {"type": "function", "function": {"name": "X", "description": "...", "parameters": {...}}}
    let mut tools = request.tools.clone().map(Value::Array);
    if let Some(value) = tools.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let tools = tools.and_then(|value| value.as_array().cloned());
    if let Some(tools) = &tools {
        let oai_tools: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?;
                let description = t.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let parameters = t
                    .get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                    }
                }))
            })
            .collect();
        if !oai_tools.is_empty() {
            body["tools"] = json!(oai_tools);
        }
    }

    body
}

// ---------------------------------------------------------------------------
// Streaming implementation (network feature required)
// ---------------------------------------------------------------------------

/// Send a streaming request to an OpenAI-compatible provider and return
/// a stream of `StreamEvent` compatible with `StreamAccumulator`.
pub(crate) async fn openai_compat_stream(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    provider_name: &str,
    request: &MessagesRequest,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    let url = build_openai_compat_url(base_url, provider_name);
    let body = build_openai_request(request, provider_name);

    tracing::debug!(
        provider = provider_name,
        url = %url,
        body = %serde_json::to_string_pretty(&body).unwrap_or_default(),
        "OpenAI-compat request"
    );

    let mut req_builder = http.post(&url).header("Content-Type", "application/json");
    if is_openai_codex_provider(provider_name) {
        req_builder = req_builder.header("Accept", "text/event-stream");
    }

    // Azure OpenAI uses `api-key` header; others use `Authorization: Bearer`
    if provider_name.eq_ignore_ascii_case("azure") {
        req_builder = req_builder.header("api-key", api_key);
    } else {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = match req_builder.json(&body).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(
                provider = provider_name,
                url = %url,
                error = %e,
                error_debug = ?e,
                "HTTP request failed"
            );
            anyhow::bail!("failed to send request to {}: {}", provider_name, e);
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Provider {} error (HTTP {}): {}",
            provider_name,
            status,
            error_body
        );
    }

    let stream = parse_openai_sse_byte_stream(
        response.bytes_stream(),
        is_openai_codex_provider(provider_name),
    );
    Ok(Box::pin(stream))
}

/// Parse an OpenAI-format SSE byte stream into StreamEvent values.
///
/// OpenAI SSE format (no `event:` field, just `data:` lines):
/// ```text
/// data: {"choices":[{"delta":{"content":"Hello"}}]}
///
/// data: {"choices":[{"delta":{},"finish_reason":"stop"}]}
///
/// data: [DONE]
/// ```
///
/// Emits Anthropic-style StreamEvent sequence for compatibility with the
/// existing StreamAccumulator:
///
///   MessageStart 鈫?ContentBlockStart 鈫?ContentBlockDelta* 鈫?///   ContentBlockStop 鈫?MessageDelta 鈫?MessageStop
fn parse_openai_sse_byte_stream<S>(
    byte_stream: S,
    is_codex_provider: bool,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = String::new();
        let mut header_emitted = false;

        // Track the next content_block index (text = 0, tool_calls start at 1+).
        let mut block_index: usize = 0;
        // Track whether we are inside a text content block.
        let mut _text_block_open = false;
        let mut thinking_block_open = false;
        let mut codex_tool_call_emitted = false;
        // Track active tool calls: index 鈫?(id, name, accumulated arguments).
        let mut tool_calls: std::collections::HashMap<u64, (String, String, String)> =
            std::collections::HashMap::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = chunk_result.context("error reading OpenAI response chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines
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

                // [DONE] marker
                if data == "[DONE]" {
                    // Close any open text block
                    if _text_block_open {
                        yield StreamEvent::ContentBlockStop { index: block_index };
                        _text_block_open = false;
                    }
                    if thinking_block_open {
                        yield StreamEvent::ContentBlockStop { index: block_index };
                    }
                    if header_emitted {
                        yield StreamEvent::MessageDelta {
                            delta: MessageDelta { stop_reason: Some("end_turn".to_string()) },
                            usage: None,
                        };
                    }
                    yield StreamEvent::MessageStop;
                    return;
                }

                let v: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Emit Anthropic-style header on first data chunk
                if !header_emitted {
                    header_emitted = true;
                    yield StreamEvent::MessageStart { usage: Usage::default() };
                }

                if is_codex_provider {
                    match v.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
                        "response.output_text.delta" => {
                            if let Some(content) = v.get("delta").and_then(|c| c.as_str()) {
                                if !content.is_empty() {
                                    if thinking_block_open {
                                        yield StreamEvent::ContentBlockStop { index: block_index };
                                        block_index += 1;
                                        thinking_block_open = false;
                                    }
                                    if !_text_block_open {
                                        yield StreamEvent::ContentBlockStart {
                                            index: block_index,
                                            content_block: ContentBlock::Text { text: String::new() },
                                        };
                                        _text_block_open = true;
                                    }
                                    yield StreamEvent::ContentBlockDelta {
                                        index: block_index,
                                        delta: json!({"type": "text_delta", "text": content}),
                                    };
                                }
                            }
                            continue;
                        }
                        "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                            if let Some(reasoning_content) = v.get("delta").and_then(|c| c.as_str()) {
                                if !reasoning_content.is_empty() {
                                    if _text_block_open {
                                        yield StreamEvent::ContentBlockStop { index: block_index };
                                        block_index += 1;
                                        _text_block_open = false;
                                    }
                                    if !thinking_block_open {
                                        yield StreamEvent::ContentBlockStart {
                                            index: block_index,
                                            content_block: ContentBlock::Thinking {
                                                thinking: String::new(),
                                                signature: None,
                                            },
                                        };
                                        thinking_block_open = true;
                                    }
                                    yield StreamEvent::ContentBlockDelta {
                                        index: block_index,
                                        delta: json!({
                                            "type": "thinking_delta",
                                            "thinking": reasoning_content,
                                        }),
                                    };
                                }
                            }
                            continue;
                        }
                        "response.output_item.done" => {
                            if let Some(item) = v.get("item") {
                                match item.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
                                    "function_call" => {
                                        if _text_block_open {
                                            yield StreamEvent::ContentBlockStop { index: block_index };
                                            block_index += 1;
                                            _text_block_open = false;
                                        }
                                        if thinking_block_open {
                                            yield StreamEvent::ContentBlockStop { index: block_index };
                                            block_index += 1;
                                            thinking_block_open = false;
                                        }
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|i| i.as_str())
                                            .or_else(|| item.get("id").and_then(|i| i.as_str()))
                                            .unwrap_or("call_unknown")
                                            .to_string();
                                        let name = item
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let input = item
                                            .get("arguments")
                                            .and_then(|a| a.as_str())
                                            .and_then(|args| serde_json::from_str(args).ok())
                                            .unwrap_or_else(|| json!({}));
                                        yield StreamEvent::ContentBlockStart {
                                            index: block_index,
                                            content_block: ContentBlock::ToolUse {
                                                id: call_id,
                                                name,
                                                input,
                                            },
                                        };
                                        yield StreamEvent::ContentBlockStop { index: block_index };
                                        block_index += 1;
                                        codex_tool_call_emitted = true;
                                    }
                                    "custom_tool_call" => {
                                        if _text_block_open {
                                            yield StreamEvent::ContentBlockStop { index: block_index };
                                            block_index += 1;
                                            _text_block_open = false;
                                        }
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|i| i.as_str())
                                            .or_else(|| item.get("id").and_then(|i| i.as_str()))
                                            .unwrap_or("call_unknown")
                                            .to_string();
                                        let name = item
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let input = item
                                            .get("input")
                                            .and_then(|a| a.as_str())
                                            .map(|input| json!({ "input": input }))
                                            .unwrap_or_else(|| json!({}));
                                        yield StreamEvent::ContentBlockStart {
                                            index: block_index,
                                            content_block: ContentBlock::ToolUse {
                                                id: call_id,
                                                name,
                                                input,
                                            },
                                        };
                                        yield StreamEvent::ContentBlockStop { index: block_index };
                                        block_index += 1;
                                        codex_tool_call_emitted = true;
                                    }
                                    _ => {}
                                }
                            }
                            continue;
                        }
                        "response.failed" => {
                            let message = v
                                .get("response")
                                .and_then(|r| r.get("error"))
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .unwrap_or("response.failed event received");
                            Err(anyhow::anyhow!("Provider openai-codex error: {}", message))?;
                        }
                        "response.completed" => {
                            if _text_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                            }
                            if thinking_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                            }

                            if let Some(usage) = v.get("response").and_then(|r| r.get("usage")) {
                                let input = usage
                                    .get("input_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let output = usage
                                    .get("output_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                if input > 0 || output > 0 {
                                    let reasoning_output =
                                        reasoning_output_tokens_from_usage(usage);
                                    yield StreamEvent::MessageDelta {
                                        delta: MessageDelta { stop_reason: None },
                                        usage: Some(Usage {
                                            input_tokens: input,
                                            output_tokens: output,
                                            reasoning_output_tokens: reasoning_output,
                                            ..Usage::default()
                                        }),
                                    };
                                }
                            }

                            yield StreamEvent::MessageDelta {
                                delta: MessageDelta {
                                    stop_reason: Some(if codex_tool_call_emitted {
                                        "tool_use"
                                    } else {
                                        "end_turn"
                                    }.to_string()),
                                },
                                usage: None,
                            };
                            yield StreamEvent::MessageStop;
                            return;
                        }
                        _ => {
                            continue;
                        }
                    }
                }

                // Process choices array
                if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
                    for choice in choices {
                        let delta = match choice.get("delta") {
                            Some(d) => d,
                            None => continue,
                        };

                        // 鈹€鈹€ Reasoning content delta 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
                        // DeepSeek thinking mode streams chain-of-thought in
                        // delta.reasoning_content. Preserve it even when the
                        // value is an empty string; tool-call turns must pass
                        // the field back verbatim on the next request.
                        if let Some(reasoning_content) =
                            delta.get("reasoning_content").and_then(|c| c.as_str())
                        {
                            if _text_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                _text_block_open = false;
                            }
                            if !thinking_block_open {
                                yield StreamEvent::ContentBlockStart {
                                    index: block_index,
                                    content_block: ContentBlock::Thinking {
                                        thinking: String::new(),
                                        signature: None,
                                    },
                                };
                                thinking_block_open = true;
                            }
                            if !reasoning_content.is_empty() {
                                yield StreamEvent::ContentBlockDelta {
                                    index: block_index,
                                    delta: json!({
                                        "type": "thinking_delta",
                                        "thinking": reasoning_content,
                                    }),
                                };
                            }
                        }

                        // 鈹€鈹€ Text content delta 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty() {
                                if thinking_block_open {
                                    yield StreamEvent::ContentBlockStop { index: block_index };
                                    block_index += 1;
                                    thinking_block_open = false;
                                }
                                if !_text_block_open {
                                    yield StreamEvent::ContentBlockStart {
                                        index: block_index,
                                        content_block: ContentBlock::Text { text: String::new() },
                                    };
                                    _text_block_open = true;
                                }
                                yield StreamEvent::ContentBlockDelta {
                                    index: block_index,
                                    delta: json!({"type": "text_delta", "text": content}),
                                };
                            }
                        }

                        // 鈹€鈹€ Tool call deltas 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
                        // OpenAI streams tool_calls as:
                        //   delta.tool_calls: [{"index":0, "id":"call_xxx", "type":"function",
                        //                       "function":{"name":"Read","arguments":""}}]
                        // Subsequent chunks only have:
                        //   delta.tool_calls: [{"index":0, "function":{"arguments":"{\"fi"}}]
                        if let Some(tc_arr) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                            for tc in tc_arr {
                                let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);

                                // New tool call: has "id" and "function.name"
                                if let Some(tc_id) = tc.get("id").and_then(|i| i.as_str()) {
                                    let fn_name = tc
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let initial_args = tc
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|a| a.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    tool_calls.insert(tc_index, (tc_id.to_string(), fn_name, initial_args));
                                } else if let Some(entry) = tool_calls.get_mut(&tc_index) {
                                    // Continuation: accumulate arguments
                                    if let Some(args_chunk) = tc
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|a| a.as_str())
                                    {
                                        entry.2.push_str(args_chunk);
                                    }
                                }
                            }
                        }

                        // 鈹€鈹€ Finish reason 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
                        if let Some(reason) = choice
                            .get("finish_reason")
                            .and_then(|r| r.as_str())
                        {
                            // Close any open text block
                            if _text_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                _text_block_open = false;
                            }
                            if thinking_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                thinking_block_open = false;
                            }

                            // Emit accumulated tool_calls as Anthropic-style ToolUse blocks
                            let mut sorted_tc: Vec<_> = tool_calls.drain().collect();
                            sorted_tc.sort_by_key(|(idx, _)| *idx);
                            for (_tc_idx, (tc_id, tc_name, tc_args)) in sorted_tc {
                                let input: Value = serde_json::from_str(&tc_args)
                                    .unwrap_or_else(|_| json!({}));
                                yield StreamEvent::ContentBlockStart {
                                    index: block_index,
                                    content_block: ContentBlock::ToolUse {
                                        id: tc_id,
                                        name: tc_name,
                                        input,
                                    },
                                };
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                            }

                            let stop_reason = match reason {
                                "stop" => "end_turn",
                                "length" => "max_tokens",
                                "tool_calls" => "tool_use",
                                other => other,
                            };
                            yield StreamEvent::MessageDelta {
                                delta: MessageDelta {
                                    stop_reason: Some(stop_reason.to_string()),
                                },
                                usage: None,
                            };
                            // Don't return yet 鈥?the usage-only chunk follows
                            // before [DONE] and we need to process it.
                        }
                    }
                }

                // Usage info 鈥?Azure/OpenAI send a final chunk with usage after finish_reason.
                // The chunk has `"usage": null` for content chunks, and a real object for the last.
                if let Some(usage) = v.get("usage").filter(|u| !u.is_null()) {
                    let input = usage
                        .get("prompt_tokens")
                        .or_else(|| usage.get("input_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output = usage
                        .get("completion_tokens")
                        .or_else(|| usage.get("output_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if input > 0 || output > 0 {
                        let reasoning_output = reasoning_output_tokens_from_usage(usage);
                        yield StreamEvent::MessageDelta {
                            delta: MessageDelta { stop_reason: None },
                            usage: Some(Usage {
                                input_tokens: input,
                                output_tokens: output,
                                reasoning_output_tokens: reasoning_output,
                                ..Usage::default()
                            }),
                        };
                    }
                }
            }
        }

        // Stream ended without [DONE] or finish_reason 鈥?still close properly
        if _text_block_open {
            yield StreamEvent::ContentBlockStop { index: block_index };
        }
        if thinking_block_open {
            yield StreamEvent::ContentBlockStop { index: block_index };
        }
        if header_emitted {
            yield StreamEvent::MessageDelta {
                delta: MessageDelta { stop_reason: Some("end_turn".to_string()) },
                usage: None,
            };
        }
        yield StreamEvent::MessageStop;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::OPENAI_CODEX_PROVIDER_NAME;

    #[test]
    fn test_extract_system_text_blocks() {
        let system = vec![
            json!({"type": "text", "text": "You are helpful."}),
            json!({"type": "text", "text": "Be concise."}),
        ];
        assert_eq!(
            extract_system_text(&system),
            "You are helpful.\nBe concise."
        );
    }

    #[test]
    fn test_extract_system_text_empty() {
        let system: Vec<Value> = vec![];
        assert_eq!(extract_system_text(&system), "");
    }

    #[test]
    fn test_flatten_content_string() {
        let content = Value::String("Hello world".to_string());
        assert_eq!(flatten_content(Some(&content)), "Hello world");
    }

    #[test]
    fn test_flatten_content_text_blocks() {
        let content = json!([
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": " world"},
        ]);
        assert_eq!(flatten_content(Some(&content)), "Hello\n world");
    }

    #[test]
    fn test_flatten_content_none() {
        assert_eq!(flatten_content(None), "");
    }

    #[test]
    fn test_flatten_content_tool_result() {
        let content = json!([
            {"type": "tool_result", "tool_use_id": "id1", "content": "result text"},
        ]);
        assert_eq!(flatten_content(Some(&content)), "result text");
    }

    #[test]
    fn test_build_openai_request_basic() {
        let req = MessagesRequest {
            model: "gpt-4o".to_string(),
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
            reasoning_effort: Some("high".to_string()),
            advisor_model: None,
        };
        let body = build_openai_request(&req, "openai");
        assert_eq!(body["model"], "gpt-4o");
    }

    #[test]
    fn test_build_openai_request_strips_anthropic_cache_fields() {
        let req = MessagesRequest {
            model: "gpt-4o".to_string(),
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
            reasoning_effort: Some("high".to_string()),
            advisor_model: None,
        };
        let body = build_openai_request(&req, "openai");
        assert!(serde_json::to_string(&body)
            .unwrap()
            .find("cache_")
            .is_none());
        assert_eq!(body["stream"], true);
        // OpenAI/Azure use max_completion_tokens for newer models
        assert_eq!(body["max_completion_tokens"], 1024);
        assert!(body.get("max_tokens").is_none() || body["max_tokens"].is_null());

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "Be helpful.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello");
    }

    #[test]
    fn test_build_openai_request_codex_compatible_shape() {
        let req = MessagesRequest {
            model: "gpt-5.4".to_string(),
            messages: vec![json!({"role": "user", "content": "Hello"})],
            system: None,
            max_tokens: 4096,
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
            reasoning_effort: Some("high".to_string()),
            advisor_model: None,
        };
        let body = build_openai_request(&req, OPENAI_CODEX_PROVIDER_NAME);
        assert_eq!(body["model"], "gpt-5.4");
        assert_eq!(body["instructions"], "");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["tool_choice"], "auto");
        assert!(body.get("stream_options").is_none());
        assert!(body.get("messages").is_none());
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
        assert_eq!(body["reasoning"], json!({"effort": "high"}));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_build_codex_responses_skips_invalid_function_schema_roots() {
        let req = MessagesRequest {
            model: "gpt-5.4".to_string(),
            messages: vec![json!({"role": "user", "content": "Hello"})],
            system: None,
            max_tokens: 4096,
            tools: Some(vec![
                json!({
                    "name": "Valid",
                    "description": "valid tool",
                    "input_schema": {
                        "type": "object",
                        "properties": {"path": {"type": "string"}}
                    }
                }),
                json!({
                    "name": "Invalid",
                    "description": "invalid tool",
                    "input_schema": {
                        "oneOf": [
                            {"type": "object", "properties": {}}
                        ]
                    }
                }),
            ]),
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

        let body = build_openai_request(&req, OPENAI_CODEX_PROVIDER_NAME);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Valid");
    }

    #[test]
    fn test_build_openai_request_no_system() {
        let req = MessagesRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![json!({"role": "user", "content": "Hi"})],
            system: None,
            max_tokens: 512,
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
        let body = build_openai_request(&req, "deepseek");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        // DeepSeek uses legacy max_tokens
        assert_eq!(body["max_tokens"], 512);
    }

    #[test]
    fn test_build_openai_request_content_blocks() {
        let req = MessagesRequest {
            model: "gpt-4o".to_string(),
            messages: vec![json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello"},
                    {"type": "text", "text": "World"},
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
        let body = build_openai_request(&req, "openai");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"], "Hello\nWorld");
    }

    #[test]
    fn test_build_deepseek_request_preserves_empty_reasoning_content_for_tool_call() {
        let req = MessagesRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![json!({
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "", "signature": null},
                    {
                        "type": "tool_use",
                        "id": "call_empty_reasoning",
                        "name": "Read",
                        "input": {"file_path": "Cargo.toml"}
                    },
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
            thinking: Some(json!({"type": "enabled"})),
            output_config: None,
            tool_choice: None,
            reasoning_effort: None,
            advisor_model: None,
        };

        let body = build_openai_request(&req, "deepseek");
        let messages = body["messages"].as_array().unwrap();
        let assistant = messages[0].as_object().unwrap();

        assert!(assistant.contains_key("reasoning_content"));
        assert_eq!(assistant["reasoning_content"], "");
        assert_eq!(assistant["content"], "");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_empty_reasoning");
    }

    #[test]
    fn test_build_openai_request_does_not_send_deepseek_reasoning_to_other_providers() {
        let req = MessagesRequest {
            model: "gpt-4o".to_string(),
            messages: vec![json!({
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "", "signature": null},
                    {
                        "type": "tool_use",
                        "id": "call_1",
                        "name": "Read",
                        "input": {"file_path": "Cargo.toml"}
                    },
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

        let body = build_openai_request(&req, "openai");
        let messages = body["messages"].as_array().unwrap();
        let assistant = messages[0].as_object().unwrap();

        assert!(!assistant.contains_key("reasoning_content"));
        assert!(!assistant.contains_key("content"));
        assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
    }

    #[tokio::test]
    async fn test_parse_codex_responses_text_stream() {
        use futures::StreamExt;

        let chunks = vec![
            format!(
                "event: response.created\ndata: {}\n\n",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_1"}
                })
            ),
            format!(
                "event: response.output_text.delta\ndata: {}\n\n",
                json!({
                    "type": "response.output_text.delta",
                    "delta": "OK",
                })
            ),
            format!(
                "event: response.completed\ndata: {}\n\n",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_1",
                        "usage": {
                            "input_tokens": 10,
                            "output_tokens": 5,
                            "output_tokens_details": {
                                "reasoning_tokens": 2
                            }
                        }
                    }
                })
            ),
        ];
        let byte_stream = futures::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<bytes::Bytes, reqwest::Error>(bytes::Bytes::from(chunk))),
        );
        let mut stream = std::pin::pin!(parse_openai_sse_byte_stream(byte_stream, true));
        let mut accumulator = crate::api::streaming::StreamAccumulator::new();

        while let Some(event) = stream.next().await {
            accumulator.process_event(&event.unwrap());
        }

        let message = accumulator.build("gpt-5.4");
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "OK"),
            other => panic!("expected text block, got {:?}", other),
        }
        let usage = message.usage.expect("usage should be captured");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.reasoning_output_tokens, 2);
    }

    #[tokio::test]
    async fn test_parse_deepseek_empty_reasoning_content_tool_call() {
        use futures::StreamExt;

        let chunks = vec![
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "role": "assistant",
                            "reasoning_content": "",
                        }
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": "call_empty_reasoning",
                                "type": "function",
                                "function": {
                                    "name": "Read",
                                    "arguments": "{\"file_path\":\"Cargo.toml\"}",
                                }
                            }]
                        }
                    }]
                })
            ),
            format!(
                "data: {}\n\n",
                json!({
                    "choices": [{
                        "delta": {},
                        "finish_reason": "tool_calls",
                    }],
                    "usage": {
                        "prompt_tokens": 2,
                        "completion_tokens": 3,
                    }
                })
            ),
            "data: [DONE]\n\n".to_string(),
        ];
        let byte_stream = futures::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<bytes::Bytes, reqwest::Error>(bytes::Bytes::from(chunk))),
        );
        let mut stream = std::pin::pin!(parse_openai_sse_byte_stream(byte_stream, false));
        let mut accumulator = crate::api::streaming::StreamAccumulator::new();

        while let Some(event) = stream.next().await {
            accumulator.process_event(&event.unwrap());
        }

        let message = accumulator.build("deepseek-v4-pro");
        assert_eq!(message.content.len(), 2);
        match &message.content[0] {
            ContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, ""),
            other => panic!("expected empty thinking block, got {:?}", other),
        }
        match &message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_empty_reasoning");
                assert_eq!(name, "Read");
                assert_eq!(input["file_path"], "Cargo.toml");
            }
            other => panic!("expected tool use block, got {:?}", other),
        }
    }
}
