use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{Stream, StreamExt};
use serde_json::{json, Map, Value};

use allthecodes_api::api::client::{
    AnthropicAuth, ApiClient, ApiClientConfig, ApiProvider, MessagesRequest,
};
use allthecodes_config::settings::{
    load_global_config, ProviderProfileSettings, API_PROVIDER_OPENAI_CODEX,
};
use allthecodes_types::message::{AssistantMessage, ContentBlock, StreamEvent, Usage};

use super::helpers::{normalized_non_empty, profile_enabled, provider_api_proxy_status};
use super::types::ProviderApiProxyStatus;

const DEFAULT_MAX_TOKENS: usize = 1024;

/// POST /anthropic-proxy/{provider_id}/v1/messages
pub async fn anthropic_proxy_messages_handler(
    AxumPath(provider_id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let client = match resolve_proxy_client(&provider_id) {
        Ok(client) => client,
        Err(error) => return anthropic_error_response(error),
    };
    let request = match anthropic_messages_request(body) {
        Ok(request) => request,
        Err(error) => {
            return anthropic_error_response(ProxyError::invalid_request(error.to_string()));
        }
    };
    let model = request.model.clone();

    if stream {
        match client.messages_stream(request).await {
            Ok(events) => anthropic_stream_response(events, model),
            Err(error) => anthropic_error_response(upstream_proxy_error(error)),
        }
    } else {
        match client.messages(request).await {
            Ok(message) => Json(anthropic_message_response(message, &model)).into_response(),
            Err(error) => anthropic_error_response(upstream_proxy_error(error)),
        }
    }
}

/// POST /anthropic-proxy/{provider_id}/v1/messages/count_tokens
pub async fn anthropic_proxy_count_tokens_handler(
    AxumPath(provider_id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let client = match resolve_proxy_client(&provider_id) {
        Ok(client) => client,
        Err(error) => return anthropic_error_response(error),
    };
    let request = match anthropic_messages_request(body) {
        Ok(request) => request,
        Err(error) => {
            return anthropic_error_response(ProxyError::invalid_request(error.to_string()));
        }
    };

    match client.count_input_tokens_exact(&request).await {
        Ok(count) => Json(json!({ "input_tokens": count.input_tokens })).into_response(),
        Err(_error) if !client.supports_exact_token_count() => {
            let input_tokens = heuristic_input_tokens(&request);
            Json(json!({ "input_tokens": input_tokens })).into_response()
        }
        Err(error) => anthropic_error_response(upstream_proxy_error(error)),
    }
}

/// POST /proxy/{provider_id}/v1/responses
pub async fn openai_proxy_responses_handler(
    AxumPath(provider_id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let client = match resolve_proxy_client(&provider_id) {
        Ok(client) => client,
        Err(error) => return openai_error_response(error),
    };
    let request = match openai_responses_request(body) {
        Ok(request) => request,
        Err(error) => return openai_error_response(ProxyError::invalid_request(error.to_string())),
    };
    let model = request.model.clone();

    if stream {
        match client.messages_stream(request).await {
            Ok(events) => openai_stream_response(events, model),
            Err(error) => openai_error_response(upstream_proxy_error(error)),
        }
    } else {
        match client.messages(request).await {
            Ok(message) => Json(openai_response(message, &model)).into_response(),
            Err(error) => openai_error_response(upstream_proxy_error(error)),
        }
    }
}

fn resolve_proxy_client(provider_id: &str) -> Result<ApiClient, ProxyError> {
    let settings = load_global_config().map_err(|error| {
        ProxyError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings_load_failed",
            error.to_string(),
        )
    })?;
    let profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(provider_id))
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::NOT_FOUND,
                "unknown_provider",
                format!("Provider '{provider_id}' is not configured"),
            )
        })?;

    match provider_api_proxy_status(provider_id, profile) {
        ProviderApiProxyStatus::Ready => {}
        ProviderApiProxyStatus::ProviderDisabled => {
            return Err(ProxyError::new(
                StatusCode::FORBIDDEN,
                "provider_disabled",
                format!("Provider '{provider_id}' is disabled"),
            ));
        }
        ProviderApiProxyStatus::MissingCredential => {
            return Err(ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "missing_credential",
                format!("Provider '{provider_id}' has no configured credential"),
            ));
        }
        ProviderApiProxyStatus::Unsupported => {
            return Err(ProxyError::new(
                StatusCode::NOT_IMPLEMENTED,
                "unsupported_provider",
                format!("Provider '{provider_id}' is not supported by the local API proxy"),
            ));
        }
        ProviderApiProxyStatus::Unknown => {
            return Err(ProxyError::new(
                StatusCode::NOT_FOUND,
                "unknown_provider",
                format!("Provider '{provider_id}' is unknown"),
            ));
        }
    }

    client_from_profile(provider_id, profile).map_err(|error| {
        let message = error.to_string();
        let status = if message.contains("credential")
            || message.contains("API key")
            || message.contains("OAuth")
        {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::BAD_REQUEST
        };
        ProxyError::new(status, "provider_configuration_invalid", message)
    })
}

fn client_from_profile(provider_id: &str, profile: &ProviderProfileSettings) -> Result<ApiClient> {
    if !profile_enabled(profile) {
        bail!("provider is disabled");
    }
    let kind = profile.api_provider.as_deref().unwrap_or(provider_id);
    if kind == API_PROVIDER_OPENAI_CODEX {
        return ApiClient::from_codex_auth_result()?
            .ok_or_else(|| anyhow!("OpenAI Codex OAuth credential is not available"));
    }

    let info = allthecodes_api::api::providers::get_provider(kind)
        .ok_or_else(|| anyhow!("unknown provider kind `{kind}`"))?;
    let base_url = normalized_non_empty(profile.base_url.as_deref())
        .unwrap_or_else(|| info.base_url.to_string());
    let default_model = normalized_non_empty(profile.model.as_deref())
        .or_else(|| {
            profile
                .available_models
                .as_ref()
                .and_then(|models| models.first())
                .and_then(|model| normalized_non_empty(Some(model)))
        })
        .unwrap_or_else(|| info.default_model.to_string());
    let api_key = profile_api_key(profile, info.env_key)
        .ok_or_else(|| anyhow!("provider credential is missing"))?;

    let provider = match info.protocol {
        allthecodes_api::api::providers::ProviderProtocol::Anthropic => {
            let endpoint_kind =
                allthecodes_api::api::providers::anthropic_endpoint_kind_for_base_url(Some(
                    &base_url,
                ));
            ApiProvider::Anthropic {
                auth: AnthropicAuth::ApiKey(api_key),
                base_url: Some(base_url),
                endpoint_kind,
            }
        }
        allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat => {
            ApiProvider::OpenAiCompat {
                name: info.name.to_string(),
                api_key,
                base_url,
                default_model: default_model.clone(),
            }
        }
        allthecodes_api::api::providers::ProviderProtocol::Google => {
            ApiProvider::Google { api_key, base_url }
        }
    };

    ApiClient::try_new(ApiClientConfig {
        provider,
        default_model,
        max_retries: 3,
        timeout_secs: 120,
    })
}

fn profile_api_key(profile: &ProviderProfileSettings, env_key: &str) -> Option<String> {
    normalized_non_empty(profile.api_key.as_deref()).or_else(|| {
        profile
            .env
            .as_ref()
            .and_then(|env| env.get(env_key))
            .and_then(|value| normalized_non_empty(Some(value)))
    })
}

fn anthropic_messages_request(body: Value) -> Result<MessagesRequest> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow!("request body must be a JSON object"))?;
    let model = required_string(obj, "model")?;
    let messages = required_array(obj, "messages")?;
    let max_tokens = optional_usize(obj, "max_tokens")?.unwrap_or(DEFAULT_MAX_TOKENS);

    Ok(MessagesRequest {
        model,
        messages,
        system: optional_system(obj)?,
        max_tokens,
        tools: optional_value_array(obj, "tools")?,
        stream: obj.get("stream").and_then(Value::as_bool).unwrap_or(false),
        metadata: obj.get("metadata").cloned(),
        service_tier: optional_string(obj, "service_tier")?,
        stop_sequences: optional_string_array(obj, "stop_sequences")?,
        temperature: optional_f64(obj, "temperature")?,
        top_p: optional_f64(obj, "top_p")?,
        top_k: optional_u32(obj, "top_k")?,
        context_management: obj.get("context_management").cloned(),
        thinking: obj.get("thinking").cloned(),
        output_config: obj.get("output_config").cloned(),
        tool_choice: obj.get("tool_choice").cloned(),
        reasoning_effort: None,
        advisor_model: optional_string(obj, "advisor_model")?,
    })
}

fn openai_responses_request(body: Value) -> Result<MessagesRequest> {
    let obj = body
        .as_object()
        .ok_or_else(|| anyhow!("request body must be a JSON object"))?;
    let model = required_string(obj, "model")?;
    let max_tokens = optional_usize(obj, "max_output_tokens")?.unwrap_or(DEFAULT_MAX_TOKENS);
    let mut system = Vec::new();
    if let Some(instructions) = optional_string(obj, "instructions")? {
        system.push(json!({ "type": "text", "text": instructions }));
    }
    let messages = responses_input_to_messages(
        obj.get("input")
            .ok_or_else(|| anyhow!("`input` is required"))?,
    )?;
    let tools = match obj.get("tools") {
        Some(value) => Some(responses_tools_to_anthropic(value)?),
        None => None,
    };
    let reasoning_effort = obj
        .get("reasoning")
        .and_then(Value::as_object)
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(MessagesRequest {
        model,
        messages,
        system: if system.is_empty() {
            None
        } else {
            Some(system)
        },
        max_tokens,
        tools,
        stream: obj.get("stream").and_then(Value::as_bool).unwrap_or(false),
        metadata: obj.get("metadata").cloned(),
        service_tier: optional_string(obj, "service_tier")?,
        stop_sequences: None,
        temperature: optional_f64(obj, "temperature")?,
        top_p: optional_f64(obj, "top_p")?,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort,
        advisor_model: None,
    })
}

fn responses_input_to_messages(input: &Value) -> Result<Vec<Value>> {
    if let Some(text) = input.as_str() {
        return Ok(vec![json!({ "role": "user", "content": text })]);
    }
    let array = input
        .as_array()
        .ok_or_else(|| anyhow!("`input` must be a string or array"))?;
    let mut messages = Vec::new();
    for item in array {
        let obj = item
            .as_object()
            .ok_or_else(|| anyhow!("Responses input array items must be objects"))?;
        match obj.get("type").and_then(Value::as_str) {
            Some("message") | None => {
                let role = optional_obj_string(obj, "role")?.unwrap_or_else(|| "user".to_string());
                let content = obj
                    .get("content")
                    .ok_or_else(|| anyhow!("Responses message input requires `content`"))?;
                messages.push(json!({
                    "role": role,
                    "content": responses_content_to_anthropic(content)?
                }));
            }
            Some("function_call_output") => {
                let call_id = required_obj_string(obj, "call_id")?;
                let output = required_obj_string(obj, "output")?;
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": output
                    }]
                }));
            }
            Some(other) => bail!("unsupported Responses input item type `{other}`"),
        }
    }
    if messages.is_empty() {
        bail!("`input` must contain at least one message");
    }
    Ok(messages)
}

fn responses_content_to_anthropic(content: &Value) -> Result<Value> {
    if content.is_string() {
        return Ok(content.clone());
    }
    let array = content
        .as_array()
        .ok_or_else(|| anyhow!("Responses message content must be a string or array"))?;
    let mut blocks = Vec::new();
    for block in array {
        let obj = block
            .as_object()
            .ok_or_else(|| anyhow!("Responses content blocks must be objects"))?;
        match obj.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") => {
                blocks.push(json!({
                    "type": "text",
                    "text": required_obj_string(obj, "text")?
                }));
            }
            Some("input_image") => bail!("Responses input_image content is not supported"),
            Some(other) => bail!("unsupported Responses content type `{other}`"),
            None => bail!("Responses content block requires `type`"),
        }
    }
    Ok(Value::Array(blocks))
}

fn responses_tools_to_anthropic(value: &Value) -> Result<Vec<Value>> {
    let tools = value
        .as_array()
        .ok_or_else(|| anyhow!("`tools` must be an array"))?;
    let mut converted = Vec::new();
    for tool in tools {
        let obj = tool
            .as_object()
            .ok_or_else(|| anyhow!("Responses tools must be objects"))?;
        match obj.get("type").and_then(Value::as_str) {
            Some("function") => {
                let name = required_obj_string(obj, "name")?;
                let description = optional_obj_string(obj, "description")?.unwrap_or_default();
                let input_schema = obj
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                converted.push(json!({
                    "name": name,
                    "description": description,
                    "input_schema": input_schema
                }));
            }
            Some(other) => bail!("unsupported Responses tool type `{other}`"),
            None => bail!("Responses tool requires `type`"),
        }
    }
    Ok(converted)
}

fn anthropic_message_response(message: AssistantMessage, model: &str) -> Value {
    json!({
        "id": format!("msg_{}", message.uuid.simple()),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": message.content,
        "stop_reason": message.stop_reason,
        "stop_sequence": null,
        "usage": usage_value(message.usage.as_ref()),
    })
}

fn openai_response(message: AssistantMessage, model: &str) -> Value {
    let id = format!("resp_{}", message.uuid.simple());
    let output = openai_output_items(&message.content);
    json!({
        "id": id,
        "object": "response",
        "created_at": now_unix(),
        "status": "completed",
        "model": model,
        "output": output,
        "output_text": content_text(&message.content),
        "usage": openai_usage_value(message.usage.as_ref()),
    })
}

fn anthropic_stream_response(
    events: impl Stream<Item = Result<StreamEvent>> + Send + 'static,
    model: String,
) -> Response {
    Sse::new(events.map(move |event| anthropic_sse_event(event, &model)))
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn openai_stream_response(
    events: impl Stream<Item = Result<StreamEvent>> + Send + 'static,
    model: String,
) -> Response {
    let mut content = Vec::<ContentBlock>::new();
    let stream = async_stream::stream! {
        yield Ok::<Event, Infallible>(sse_json("response.created", json!({
            "type": "response.created",
            "response": {
                "id": format!("resp_{}", now_unix()),
                "object": "response",
                "created_at": now_unix(),
                "model": model.clone(),
                "status": "in_progress"
            }
        })));

        futures::pin_mut!(events);
        while let Some(item) = events.next().await {
            match item {
                Ok(event) => {
                    collect_content_block(&mut content, &event);
                    if let Some(delta) = openai_delta_event(&event) {
                        yield Ok(delta);
                    }
                }
                Err(error) => {
                    yield Ok(sse_json("error", openai_error_body(&ProxyError::bad_gateway(error.to_string()))));
                    return;
                }
            }
        }
        yield Ok(sse_json("response.completed", json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "model": model.clone(),
                "output_text": content_text(&content),
                "output": openai_output_items(&content)
            }
        })));
        yield Ok(Event::default().data("[DONE]"));
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn anthropic_sse_event(item: Result<StreamEvent>, model: &str) -> Result<Event, Infallible> {
    let event = match item {
        Ok(StreamEvent::MessageStart { usage }) => sse_json(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": format!("msg_{}", now_unix()),
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": usage,
                }
            }),
        ),
        Ok(StreamEvent::ContentBlockStart {
            index,
            content_block,
        }) => sse_json(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": content_block,
            }),
        ),
        Ok(StreamEvent::ContentBlockDelta { index, delta }) => sse_json(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": delta,
            }),
        ),
        Ok(StreamEvent::ContentBlockStop { index }) => sse_json(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": index,
            }),
        ),
        Ok(StreamEvent::MessageDelta { delta, usage }) => sse_json(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": delta,
                "usage": usage,
            }),
        ),
        Ok(StreamEvent::MessageStop) => sse_json("message_stop", json!({ "type": "message_stop" })),
        Err(error) => sse_json(
            "error",
            anthropic_error_body(&ProxyError::bad_gateway(error.to_string())),
        ),
    };
    Ok(event)
}

fn openai_delta_event(event: &StreamEvent) -> Option<Event> {
    match event {
        StreamEvent::ContentBlockDelta { delta, .. } => {
            if delta.get("type").and_then(Value::as_str) == Some("text_delta") {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(sse_json(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "delta": text
                    }),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn collect_content_block(content: &mut Vec<ContentBlock>, event: &StreamEvent) {
    match event {
        StreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            while content.len() <= *index {
                content.push(ContentBlock::Text {
                    text: String::new(),
                });
            }
            content[*index] = content_block.clone();
        }
        StreamEvent::ContentBlockDelta { index, delta } => {
            if let Some(block) = content.get_mut(*index) {
                if delta.get("type").and_then(Value::as_str) == Some("text_delta") {
                    if let Some(text) = delta.get("text").and_then(Value::as_str) {
                        if let ContentBlock::Text { text: current } = block {
                            current.push_str(text);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn openai_output_items(content: &[ContentBlock]) -> Vec<Value> {
    content
        .iter()
        .enumerate()
        .map(|(index, block)| match block {
            ContentBlock::Text { text } => json!({
                "id": format!("msg_{index}"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text, "annotations": [] }]
            }),
            ContentBlock::ToolUse { id, name, input }
            | ContentBlock::ServerToolUse { id, name, input } => json!({
                "id": id,
                "type": "function_call",
                "call_id": id,
                "name": name,
                "arguments": input.to_string(),
                "status": "completed"
            }),
            other => json!({
                "id": format!("item_{index}"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": content_block_text(other), "annotations": [] }]
            }),
        })
        .collect()
}

fn content_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .map(content_block_text)
        .collect::<Vec<_>>()
        .join("")
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::Thinking { thinking, .. } => thinking.clone(),
        ContentBlock::ConnectorText { connector_text, .. } => connector_text.clone(),
        _ => String::new(),
    }
}

fn sse_json(event: &str, data: Value) -> Event {
    let data = serde_json::to_string(&data)
        .unwrap_or_else(|error| format!(r#"{{"error":"serialization failed: {error}"}}"#));
    Event::default().event(event).data(data)
}

#[derive(Debug)]
struct ProxyError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ProxyError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "upstream_error", message)
    }
}

fn upstream_proxy_error(error: anyhow::Error) -> ProxyError {
    let message = error.to_string();
    let status = status_from_upstream_error(&message).unwrap_or(StatusCode::BAD_GATEWAY);
    ProxyError::new(status, "upstream_error", message)
}

fn status_from_upstream_error(message: &str) -> Option<StatusCode> {
    for marker in ["HTTP ", "status="] {
        if let Some(start) = message.find(marker) {
            let digits = message[start + marker.len()..]
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if let Ok(code) = digits.parse::<u16>() {
                if let Ok(status) = StatusCode::from_u16(code) {
                    return Some(status);
                }
            }
        }
    }
    None
}

fn anthropic_error_response(error: ProxyError) -> Response {
    (error.status, Json(anthropic_error_body(&error))).into_response()
}

fn anthropic_error_body(error: &ProxyError) -> Value {
    json!({
        "type": "error",
        "error": {
            "type": anthropic_error_type(error.status),
            "message": error.message,
        }
    })
}

fn anthropic_error_type(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::BAD_REQUEST => "invalid_request_error",
        _ => "api_error",
    }
}

fn openai_error_response(error: ProxyError) -> Response {
    (error.status, Json(openai_error_body(&error))).into_response()
}

fn openai_error_body(error: &ProxyError) -> Value {
    json!({
        "error": {
            "message": error.message,
            "type": openai_error_type(error.status),
            "code": error.code
        }
    })
}

fn openai_error_type(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        StatusCode::BAD_REQUEST => "invalid_request_error",
        _ => "api_error",
    }
}

fn usage_value(usage: Option<&Usage>) -> Value {
    let usage = usage.cloned().unwrap_or_default();
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_read_input_tokens": usage.cache_read_input_tokens,
        "cache_creation_input_tokens": usage.cache_creation_input_tokens,
    })
}

fn openai_usage_value(usage: Option<&Usage>) -> Value {
    let usage = usage.cloned().unwrap_or_default();
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.input_tokens + usage.output_tokens,
        "output_tokens_details": {
            "reasoning_tokens": usage.reasoning_output_tokens
        }
    })
}

fn heuristic_input_tokens(request: &MessagesRequest) -> u64 {
    let mut chars = request.model.len()
        + serde_json::to_string(&request.messages)
            .unwrap_or_default()
            .len();
    if let Some(system) = &request.system {
        chars += serde_json::to_string(system).unwrap_or_default().len();
    }
    (chars as u64).div_ceil(4)
}

fn required_string(obj: &Map<String, Value>, field: &str) -> Result<String> {
    optional_string(obj, field)?.ok_or_else(|| anyhow!("`{field}` is required"))
}

fn required_obj_string(obj: &Map<String, Value>, field: &str) -> Result<String> {
    optional_obj_string(obj, field)?.ok_or_else(|| anyhow!("`{field}` is required"))
}

fn optional_string(obj: &Map<String, Value>, field: &str) -> Result<Option<String>> {
    optional_obj_string(obj, field)
}

fn optional_obj_string(obj: &Map<String, Value>, field: &str) -> Result<Option<String>> {
    match obj.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be a string"),
    }
}

fn required_array(obj: &Map<String, Value>, field: &str) -> Result<Vec<Value>> {
    optional_value_array(obj, field)?.ok_or_else(|| anyhow!("`{field}` is required"))
}

fn optional_value_array(obj: &Map<String, Value>, field: &str) -> Result<Option<Vec<Value>>> {
    match obj.get(field) {
        Some(Value::Array(values)) => Ok(Some(values.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be an array"),
    }
}

fn optional_string_array(obj: &Map<String, Value>, field: &str) -> Result<Option<Vec<String>>> {
    match obj.get(field) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("`{field}` must contain only strings"))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be an array"),
    }
}

fn optional_system(obj: &Map<String, Value>) -> Result<Option<Vec<Value>>> {
    match obj.get("system") {
        Some(Value::String(text)) => Ok(Some(vec![json!({ "type": "text", "text": text })])),
        Some(Value::Array(values)) => Ok(Some(values.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`system` must be a string or array"),
    }
}

fn optional_usize(obj: &Map<String, Value>, field: &str) -> Result<Option<usize>> {
    match obj.get(field) {
        Some(value) if value.is_u64() => {
            let value = value.as_u64().unwrap_or_default();
            usize::try_from(value)
                .with_context(|| format!("`{field}` is too large"))
                .map(Some)
        }
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be a positive integer"),
    }
}

fn optional_u32(obj: &Map<String, Value>, field: &str) -> Result<Option<u32>> {
    match obj.get(field) {
        Some(value) if value.is_u64() => {
            let value = value.as_u64().unwrap_or_default();
            u32::try_from(value)
                .with_context(|| format!("`{field}` is too large"))
                .map(Some)
        }
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be a positive integer"),
    }
}

fn optional_f64(obj: &Map<String, Value>, field: &str) -> Result<Option<f64>> {
    match obj.get(field) {
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| anyhow!("`{field}` must be a number"))
            .map(Some),
        Some(Value::Null) | None => Ok(None),
        Some(_) => bail!("`{field}` must be a number"),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
