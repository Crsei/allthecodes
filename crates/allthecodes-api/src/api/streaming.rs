//! SSE (Server-Sent Events) stream parser for the Anthropic Messages API
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::fmt;

use allthecodes_types::message::{
    AssistantMessage, ContentBlock, MessageDelta, StreamEvent, Usage,
};

#[derive(Clone, Debug, PartialEq)]
pub struct CompletedToolUse {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedApiError {
    pub provider: String,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub error_type: Option<String>,
    pub message: String,
}

impl fmt::Display for NormalizedApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "API error")?;
        write!(f, " provider={}", self.provider)?;
        if let Some(status) = self.status {
            write!(f, " status={status}")?;
        }
        if let Some(request_id) = &self.request_id {
            write!(f, " request_id={request_id}")?;
        }
        if let Some(error_type) = &self.error_type {
            write!(f, " type={error_type}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for NormalizedApiError {}

pub fn normalize_api_error_body(
    provider: &str,
    status: Option<u16>,
    body: &str,
    request_id: Option<String>,
) -> NormalizedApiError {
    match serde_json::from_str::<Value>(body) {
        Ok(value) => normalize_api_error_value(provider, status, &value, request_id),
        Err(_) => NormalizedApiError {
            provider: provider.to_string(),
            status,
            request_id,
            error_type: None,
            message: body.trim().to_string(),
        },
    }
}

pub fn normalize_api_error_value(
    provider: &str,
    status: Option<u16>,
    value: &Value,
    request_id: Option<String>,
) -> NormalizedApiError {
    let error = value.get("error").unwrap_or(value);
    let request_id = request_id
        .or_else(|| string_field(value, "request_id"))
        .or_else(|| string_field(value, "request-id"))
        .or_else(|| string_field(error, "request_id"))
        .or_else(|| string_field(error, "request-id"));
    let status = status
        .or_else(|| u16_field(value, "status"))
        .or_else(|| u16_field(value, "status_code"))
        .or_else(|| u16_field(error, "status"))
        .or_else(|| u16_field(error, "status_code"));
    let error_type = string_field(error, "type")
        .or_else(|| string_field(error, "code"))
        .or_else(|| string_field(value, "error_type"));
    let message = string_field(error, "message")
        .or_else(|| string_field(value, "message"))
        .unwrap_or_else(|| value.to_string());

    NormalizedApiError {
        provider: provider.to_string(),
        status,
        request_id,
        error_type,
        message,
    }
}

/// Parse a single SSE line into a StreamEvent
pub fn parse_sse_event(event_type: &str, data: &str) -> Result<Option<StreamEvent>> {
    let parsed: Value = serde_json::from_str(data)?;

    match event_type {
        "message_start" => {
            let usage = parse_required_usage(&parsed, "message.usage")?;
            Ok(Some(StreamEvent::MessageStart { usage }))
        }
        "content_block_start" => {
            let index = required_index(&parsed)?;
            let block: ContentBlock =
                serde_json::from_value(required_field(&parsed, "content_block")?)
                    .context("malformed required SSE field `content_block`")?;
            Ok(Some(StreamEvent::ContentBlockStart {
                index,
                content_block: block,
            }))
        }
        "content_block_delta" => {
            let index = required_index(&parsed)?;
            let delta = required_object_field(&parsed, "delta")?;
            Ok(Some(StreamEvent::ContentBlockDelta { index, delta }))
        }
        "content_block_stop" => {
            let index = required_index(&parsed)?;
            Ok(Some(StreamEvent::ContentBlockStop { index }))
        }
        "message_delta" => {
            let delta: MessageDelta = serde_json::from_value(required_field(&parsed, "delta")?)
                .context("malformed required SSE field `delta`")?;
            let usage = parsed
                .get("usage")
                .map(|u| {
                    serde_json::from_value(u.clone())
                        .context("malformed optional SSE field `usage`")
                })
                .transpose()?;
            Ok(Some(StreamEvent::MessageDelta { delta, usage }))
        }
        "message_stop" => Ok(Some(StreamEvent::MessageStop)),
        "ping" => Ok(None),
        "error" => Err(normalize_api_error_value("anthropic", None, &parsed, None).into()),
        _ => Ok(None),
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn u16_field(value: &Value, field: &str) -> Option<u16> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
}

fn required_field(parsed: &Value, field: &str) -> Result<Value> {
    let mut current = parsed;
    for part in field.split('.') {
        current = current
            .get(part)
            .with_context(|| format!("missing required SSE field `{field}`"))?;
    }
    Ok(current.clone())
}

fn required_object_field(parsed: &Value, field: &str) -> Result<Value> {
    let value = required_field(parsed, field)?;
    if !value.is_object() {
        bail!("malformed required SSE field `{field}`: expected object");
    }
    Ok(value)
}

fn parse_required_usage(parsed: &Value, field: &str) -> Result<Usage> {
    serde_json::from_value(required_field(parsed, field)?)
        .with_context(|| format!("malformed required SSE field `{field}`"))
}

fn required_index(parsed: &Value) -> Result<usize> {
    let Some(index) = parsed.get("index") else {
        bail!("missing required SSE field `index`");
    };
    let Some(index) = index.as_u64() else {
        bail!("malformed required SSE field `index`: expected non-negative integer");
    };
    usize::try_from(index).context("malformed required SSE field `index`: value is too large")
}

/// Accumulate stream events into a complete AssistantMessage
pub struct StreamAccumulator {
    pub content_blocks: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: Option<String>,
    tool_input_partials: Vec<String>,
    stopped_blocks: Vec<bool>,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            content_blocks: Vec::new(),
            usage: Usage::default(),
            stop_reason: None,
            tool_input_partials: Vec::new(),
            stopped_blocks: Vec::new(),
        }
    }

    pub fn process_event(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::MessageStart { usage } => {
                self.usage = usage.clone();
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                while self.content_blocks.len() <= *index {
                    self.content_blocks.push(ContentBlock::Text {
                        text: String::new(),
                    });
                }
                while self.tool_input_partials.len() <= *index {
                    self.tool_input_partials.push(String::new());
                }
                while self.stopped_blocks.len() <= *index {
                    self.stopped_blocks.push(false);
                }
                self.content_blocks[*index] = content_block.clone();
                self.tool_input_partials[*index].clear();
                self.stopped_blocks[*index] = false;
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                if let Some(block) = self.content_blocks.get_mut(*index) {
                    let handled = match block {
                        ContentBlock::Text { text } => {
                            if delta_type_matches(delta, "text_delta") {
                                if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                                    text.push_str(t);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        ContentBlock::Thinking {
                            thinking,
                            signature,
                        } => {
                            let mut handled = false;
                            if delta_type_matches(delta, "thinking_delta") {
                                if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                                    thinking.push_str(t);
                                    handled = true;
                                }
                            }
                            if delta_type_matches(delta, "signature_delta") {
                                if let Some(s) = delta.get("signature").and_then(|v| v.as_str()) {
                                    signature.get_or_insert_with(String::new).push_str(s);
                                    handled = true;
                                }
                            }
                            handled
                        }
                        ContentBlock::ToolUse { .. } | ContentBlock::ServerToolUse { .. } => {
                            if delta_type_matches(delta, "input_json_delta") {
                                if let Some(partial_json) =
                                    delta.get("partial_json").and_then(|v| v.as_str())
                                {
                                    while self.tool_input_partials.len() <= *index {
                                        self.tool_input_partials.push(String::new());
                                    }
                                    self.tool_input_partials[*index].push_str(partial_json);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        ContentBlock::ConnectorText {
                            connector_text,
                            signature,
                        } => {
                            let mut handled = false;
                            if delta_type_matches(delta, "connector_text_delta") {
                                if let Some(t) =
                                    delta.get("connector_text").and_then(|v| v.as_str())
                                {
                                    connector_text.push_str(t);
                                    handled = true;
                                }
                            }
                            if delta_type_matches(delta, "signature_delta") {
                                if let Some(s) = delta.get("signature").and_then(|v| v.as_str()) {
                                    signature.get_or_insert_with(String::new).push_str(s);
                                    handled = true;
                                }
                            }
                            handled
                        }
                        _ => false,
                    };

                    if !handled {
                        log_unsupported_delta(*index, delta);
                    }
                } else {
                    tracing::warn!(
                        index = *index,
                        delta_type = delta_type_name(delta),
                        "received stream content block delta before content block start"
                    );
                }
            }
            StreamEvent::ContentBlockStop { index } => {
                self.finalize_tool_input(*index);
                while self.stopped_blocks.len() <= *index {
                    self.stopped_blocks.push(false);
                }
                self.stopped_blocks[*index] = true;
            }
            StreamEvent::MessageDelta { delta, usage } => {
                self.stop_reason = delta.stop_reason.clone();
                if let Some(u) = usage {
                    // OpenAI-compatible providers report both input and output
                    // tokens in the final chunk; Anthropic only sends output here.
                    if u.input_tokens > 0 {
                        self.usage.input_tokens = u.input_tokens;
                    }
                    if u.output_tokens > 0 {
                        self.usage.output_tokens = u.output_tokens;
                    }
                    if u.reasoning_output_tokens > 0 {
                        self.usage.reasoning_output_tokens = u.reasoning_output_tokens;
                    }
                }
            }
            _ => {}
        }
    }

    fn finalize_tool_input(&mut self, index: usize) {
        let Some(partial_json) = self.tool_input_partials.get_mut(index) else {
            return;
        };
        if partial_json.is_empty() {
            return;
        }

        let Ok(input) = serde_json::from_str::<Value>(partial_json) else {
            return;
        };

        match self.content_blocks.get_mut(index) {
            Some(ContentBlock::ToolUse {
                input: block_input, ..
            })
            | Some(ContentBlock::ServerToolUse {
                input: block_input, ..
            }) => {
                *block_input = input;
                partial_json.clear();
            }
            _ => {}
        }
    }

    pub fn completed_tool_use(&mut self, index: usize) -> Option<CompletedToolUse> {
        if !self.stopped_blocks.get(index).copied().unwrap_or(false) {
            return None;
        }

        self.finalize_tool_input(index);
        match self.content_blocks.get(index) {
            Some(ContentBlock::ToolUse { id, name, input }) => Some(CompletedToolUse {
                index,
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        }
    }

    /// Build the final AssistantMessage with cost calculated from model pricing.
    pub fn build(self, model: &str) -> AssistantMessage {
        self.build_with_uuid(model, uuid::Uuid::new_v4())
    }

    pub fn build_with_uuid(mut self, model: &str, uuid: uuid::Uuid) -> AssistantMessage {
        for index in 0..self.content_blocks.len() {
            self.finalize_tool_input(index);
        }

        let cost_usd = crate::api::pricing::calculate_cost(model, &self.usage);
        AssistantMessage {
            uuid,
            timestamp: chrono::Utc::now().timestamp(),
            role: "assistant".to_string(),
            content: self.content_blocks,
            usage: Some(self.usage),
            stop_reason: self.stop_reason,
            is_api_error_message: false,
            api_error: None,
            cost_usd,
        }
    }
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

fn delta_type_matches(delta: &Value, expected: &str) -> bool {
    delta
        .get("type")
        .and_then(|v| v.as_str())
        .is_none_or(|actual| actual == expected)
}

fn delta_type_name(delta: &Value) -> &str {
    delta
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
}

fn log_unsupported_delta(index: usize, delta: &Value) {
    tracing::warn!(
        index,
        delta_type = delta_type_name(delta),
        "ignoring unsupported stream content block delta"
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accumulates_tool_input_json_delta() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": "{\"command\":\"echo"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": " hi\",\"timeout\":1000}"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 0 });

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "Bash");
                assert_eq!(input["command"], "echo hi");
                assert_eq!(input["timeout"], 1000);
            }
            other => panic!("expected tool_use block, got {other:?}"),
        }
    }

    #[test]
    fn exposes_completed_tool_use_after_block_stop() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "toolu_done".to_string(),
                name: "Read".to_string(),
                input: json!({}),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": "{\"file_path\":\"src/main.rs\"}"
            }),
        });

        assert!(accumulator.completed_tool_use(0).is_none());

        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 0 });
        let completed = accumulator.completed_tool_use(0).expect("completed tool");

        assert_eq!(completed.index, 0);
        assert_eq!(completed.id, "toolu_done");
        assert_eq!(completed.name, "Read");
        assert_eq!(completed.input, json!({ "file_path": "src/main.rs" }));
        assert!(accumulator.completed_tool_use(1).is_none());
    }

    #[test]
    fn accumulates_server_tool_use_input_without_marking_local_tool_complete() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ServerToolUse {
                id: "srvu_1".to_string(),
                name: "web_search".to_string(),
                input: json!({}),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": "{\"query\":\"rust streaming"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": " accumulator\"}"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 0 });

        assert!(accumulator.completed_tool_use(0).is_none());

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "srvu_1");
                assert_eq!(name, "web_search");
                assert_eq!(input, &json!({ "query": "rust streaming accumulator" }));
            }
            other => panic!("expected server_tool_use block, got {other:?}"),
        }
    }

    #[test]
    fn accumulates_connector_text_and_signature() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ConnectorText {
                connector_text: String::new(),
                signature: None,
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "connector_text_delta",
                "connector_text": "first "
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "connector_text_delta",
                "connector_text": "second"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "signature_delta",
                "signature": "sig"
            }),
        });

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::ConnectorText {
                connector_text,
                signature,
            } => {
                assert_eq!(connector_text, "first second");
                assert_eq!(signature.as_deref(), Some("sig"));
            }
            other => panic!("expected connector_text block, got {other:?}"),
        }
    }

    #[test]
    fn accumulates_thinking_signature_delta() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "thinking_delta",
                "thinking": "first "
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "thinking_delta",
                "thinking": "second"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "signature_delta",
                "signature": "sig-a"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "signature_delta",
                "signature": "sig-b"
            }),
        });

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "first second");
                assert_eq!(signature.as_deref(), Some("sig-asig-b"));
            }
            other => panic!("expected thinking block, got {other:?}"),
        }
    }

    #[test]
    fn ignores_text_like_unsupported_delta() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: "prefix".to_string(),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "connector_text_delta",
                "text": " should not append"
            }),
        });

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "prefix"),
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn accepts_legacy_delta_without_type() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({ "text": "legacy" }),
        });

        let message = accumulator.build("claude-sonnet-4-20250514");
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "legacy"),
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn accumulates_mixed_anthropic_stream_sequence() {
        let mut accumulator = StreamAccumulator::new();

        accumulator.process_event(&StreamEvent::MessageStart {
            usage: Usage {
                input_tokens: 42,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 3,
                cache_creation_input_tokens: 2,
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "text_delta",
                "text": "hello"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 0 });
        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 1,
            delta: json!({
                "type": "thinking_delta",
                "thinking": "considering"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 1,
            delta: json!({
                "type": "signature_delta",
                "signature": "sig"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 1 });
        accumulator.process_event(&StreamEvent::ContentBlockStart {
            index: 2,
            content_block: ContentBlock::ToolUse {
                id: "toolu_2".to_string(),
                name: "Read".to_string(),
                input: json!({}),
            },
        });
        accumulator.process_event(&StreamEvent::ContentBlockDelta {
            index: 2,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": "{\"file_path\":\"Cargo.toml\"}"
            }),
        });
        accumulator.process_event(&StreamEvent::ContentBlockStop { index: 2 });
        accumulator.process_event(&StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some("tool_use".to_string()),
            },
            usage: Some(Usage {
                input_tokens: 0,
                output_tokens: 9,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
        });
        accumulator.process_event(&StreamEvent::MessageStop);

        let message = accumulator.build("claude-sonnet-4-20250514");
        assert_eq!(message.stop_reason.as_deref(), Some("tool_use"));
        let usage = message.usage.as_ref().expect("usage");
        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 9);
        assert_eq!(usage.cache_read_input_tokens, 3);
        assert_eq!(usage.cache_creation_input_tokens, 2);

        match &message.content[..] {
            [ContentBlock::Text { text }, ContentBlock::Thinking {
                thinking,
                signature,
            }, ContentBlock::ToolUse { id, name, input }] => {
                assert_eq!(text, "hello");
                assert_eq!(thinking, "considering");
                assert_eq!(signature.as_deref(), Some("sig"));
                assert_eq!(id, "toolu_2");
                assert_eq!(name, "Read");
                assert_eq!(input["file_path"], "Cargo.toml");
            }
            other => panic!("unexpected content blocks: {other:?}"),
        }
    }

    #[test]
    fn parse_errors_when_content_block_start_missing_required_index() {
        let error = parse_sse_event(
            "content_block_start",
            r#"{"content_block":{"type":"text","text":""}}"#,
        )
        .expect_err("missing index should fail");

        assert!(error
            .to_string()
            .contains("missing required SSE field `index`"));
    }

    #[test]
    fn parse_errors_when_content_block_start_has_malformed_content_block() {
        let error = parse_sse_event(
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"tool_use","id":"toolu_1"}}"#,
        )
        .expect_err("malformed content block should fail");

        assert!(
            error
                .to_string()
                .contains("malformed required SSE field `content_block`"),
            "{error:#}"
        );
    }

    #[test]
    fn parse_errors_when_content_block_delta_missing_required_delta() {
        let error = parse_sse_event("content_block_delta", r#"{"index":0}"#)
            .expect_err("missing delta should fail");

        assert!(error
            .to_string()
            .contains("missing required SSE field `delta`"));
    }

    #[test]
    fn parse_errors_when_message_start_missing_required_usage() {
        let error = parse_sse_event("message_start", r#"{"message":{"id":"msg_1"}}"#)
            .expect_err("missing usage should fail");

        assert!(error
            .to_string()
            .contains("missing required SSE field `message.usage`"));
    }

    #[test]
    fn parse_message_delta_allows_missing_optional_usage() {
        let event = parse_sse_event("message_delta", r#"{"delta":{"stop_reason":"end_turn"}}"#)
            .expect("valid event")
            .expect("stream event");

        match event {
            StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
                assert!(usage.is_none());
            }
            other => panic!("expected message_delta, got {other:?}"),
        }
    }

    #[test]
    fn build_with_uuid_calculates_cost() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(&StreamEvent::MessageStart {
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        });
        // claude-sonnet-4-20250514: input=$3/1M, output=$15/1M
        let expected = 1000.0 * 3.0 / 1_000_000.0;
        let uuid = uuid::Uuid::nil();
        let msg = acc.build_with_uuid("claude-sonnet-4-20250514", uuid);
        assert!(msg.cost_usd > 0.0);
        assert!((msg.cost_usd - expected).abs() < 1e-10);
    }

    #[test]
    fn build_with_uuid_unknown_model_cost_zero() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(&StreamEvent::MessageStart {
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 500,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        });
        let uuid = uuid::Uuid::nil();
        let msg = acc.build_with_uuid("unknown-model", uuid);
        assert_eq!(msg.cost_usd, 0.0);
    }

    #[test]
    fn build_with_uuid_cache_tokens_included() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(&StreamEvent::MessageStart {
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 500,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 500,
                cache_creation_input_tokens: 200,
            },
        });
        // claude-sonnet-4-20250514: input=$3/1M, output=$15/1M
        // cache_read = 500 * 3.0 * 0.1 / 1_000_000
        // cache_creation = 200 * 3.0 * 1.25 / 1_000_000
        let expected = 1000.0 * 3.0 / 1_000_000.0
            + 500.0 * 15.0 / 1_000_000.0
            + 500.0 * 3.0 * 0.1 / 1_000_000.0
            + 200.0 * 3.0 * 1.25 / 1_000_000.0;
        let uuid = uuid::Uuid::nil();
        let msg = acc.build_with_uuid("claude-sonnet-4-20250514", uuid);
        assert!(msg.cost_usd > 0.0);
        assert!((msg.cost_usd - expected).abs() < 1e-10);
    }
}
