use serde_json::{json, Value};

use crate::api::client::{is_openai_codex_provider, strip_anthropic_cache_fields, MessagesRequest};

use super::format::{
    build_responses_input, build_responses_tools, extract_system_text, flatten_content,
};

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
        "max" => Some("max"),
        _ => None,
    }
}

/// Convert a MessagesRequest (Anthropic format) to OpenAI chat completions body.
///
/// `provider_name` is used to select the correct token-limit parameter:
/// Azure OpenAI and OpenAI newer models require `max_completion_tokens`
/// instead of the legacy `max_tokens`.
pub(super) fn build_openai_request(request: &MessagesRequest, provider_name: &str) -> Value {
    if is_openai_codex_provider(provider_name) {
        return build_codex_responses_request(request);
    }

    let is_deepseek_provider = provider_name.eq_ignore_ascii_case("deepseek");
    let mut oai_messages: Vec<Value> = Vec::new();
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let messages = messages.as_array().cloned().unwrap_or_default();

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

    for msg in &messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");

        if role == "assistant" {
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

                for (tool_use_id, result) in tool_results {
                    oai_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": result,
                    }));
                }
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

#[cfg(test)]
#[path = "builder_tests.rs"]
mod builder_tests;
