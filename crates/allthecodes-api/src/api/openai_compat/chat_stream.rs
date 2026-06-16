use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;
use serde_json::{json, Value};

use crate::api::client::{build_openai_compat_url, is_openai_codex_provider, MessagesRequest};
use crate::api::provider_runtime::{
    metadata_from_response, provider_error_from_response, ProviderEndpoint, ProviderStreamTransport,
};
use allthecodes_types::message::{ContentBlock, MessageDelta, StreamEvent, Usage};

use super::builder::build_openai_request;
use super::codex::parse_codex_sse_byte_stream;
use super::format::reasoning_output_tokens_from_usage;

/// Send a streaming request to an OpenAI-compatible provider and return
/// a stream of `StreamEvent` compatible with `StreamAccumulator`.
pub(crate) async fn openai_compat_stream(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    provider_name: &str,
    request: &MessagesRequest,
) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
    let endpoint = ProviderEndpoint::openai_compat(provider_name, base_url, api_key)?;
    let url = build_openai_compat_url(base_url, provider_name);
    let body = build_openai_request(request, provider_name);

    tracing::debug!(
        provider = provider_name,
        url = %url,
        body = %serde_json::to_string_pretty(&body).unwrap_or_default(),
        "OpenAI-compat request"
    );

    let response = match endpoint
        .apply_headers(http.post(&url))
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(
                provider = provider_name,
                url = %url,
                error = %e,
                error_debug = ?e,
                "HTTP request failed"
            );
            return Err(
                crate::api::provider_runtime::ProviderError::transport(provider_name, e).into(),
            );
        }
    };

    if !response.status().is_success() {
        return Err(provider_error_from_response(provider_name, response)
            .await
            .into());
    }

    let metadata =
        metadata_from_response(provider_name, ProviderStreamTransport::SseHttp, &response);
    tracing::debug!(?metadata, "provider stream established");
    let byte_stream = response.bytes_stream();
    if is_openai_codex_provider(provider_name) {
        Ok(Box::pin(parse_codex_sse_byte_stream(byte_stream)))
    } else {
        Ok(Box::pin(parse_chat_sse_byte_stream(byte_stream)))
    }
}

fn parse_chat_sse_byte_stream<S>(byte_stream: S) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = String::new();
        let mut header_emitted = false;
        let mut block_index: usize = 0;
        let mut text_block_open = false;
        let mut thinking_block_open = false;
        let mut tool_calls: std::collections::HashMap<u64, (String, String, String)> =
            std::collections::HashMap::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = chunk_result.context("error reading OpenAI response chunk")?;
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

                if data == "[DONE]" {
                    if text_block_open {
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
                    return;
                }

                let v: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if !header_emitted {
                    header_emitted = true;
                    yield StreamEvent::MessageStart { usage: Usage::default() };
                }

                if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
                    for choice in choices {
                        let delta = match choice.get("delta") {
                            Some(d) => d,
                            None => continue,
                        };

                        if let Some(reasoning_content) =
                            delta.get("reasoning_content").and_then(|c| c.as_str())
                        {
                            if text_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                text_block_open = false;
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

                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty() {
                                if thinking_block_open {
                                    yield StreamEvent::ContentBlockStop { index: block_index };
                                    block_index += 1;
                                    thinking_block_open = false;
                                }
                                if !text_block_open {
                                    yield StreamEvent::ContentBlockStart {
                                        index: block_index,
                                        content_block: ContentBlock::Text { text: String::new() },
                                    };
                                    text_block_open = true;
                                }
                                yield StreamEvent::ContentBlockDelta {
                                    index: block_index,
                                    delta: json!({"type": "text_delta", "text": content}),
                                };
                            }
                        }

                        if let Some(tc_arr) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                            for tc in tc_arr {
                                let tc_index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);

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

                        if let Some(reason) = choice
                            .get("finish_reason")
                            .and_then(|r| r.as_str())
                        {
                            if text_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                text_block_open = false;
                            }
                            if thinking_block_open {
                                yield StreamEvent::ContentBlockStop { index: block_index };
                                block_index += 1;
                                thinking_block_open = false;
                            }

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
                        }
                    }
                }

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

        if text_block_open {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut stream = std::pin::pin!(parse_chat_sse_byte_stream(byte_stream));
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
