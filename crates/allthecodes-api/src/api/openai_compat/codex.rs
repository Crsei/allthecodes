use anyhow::{Context, Result};
use futures::Stream;
use serde_json::{json, Value};

use allthecodes_types::message::{ContentBlock, MessageDelta, StreamEvent, Usage};

use super::format::reasoning_output_tokens_from_usage;

pub(super) fn parse_codex_sse_byte_stream<S>(
    byte_stream: S,
) -> impl Stream<Item = Result<StreamEvent>> + Send
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
        let mut codex_tool_call_emitted = false;

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

                match v.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
                    "response.output_text.delta" => {
                        if let Some(content) = v.get("delta").and_then(|c| c.as_str()) {
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
                    }
                    "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                        if let Some(reasoning_content) = v.get("delta").and_then(|c| c.as_str()) {
                            if !reasoning_content.is_empty() {
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
                                yield StreamEvent::ContentBlockDelta {
                                    index: block_index,
                                    delta: json!({
                                        "type": "thinking_delta",
                                        "thinking": reasoning_content,
                                    }),
                                };
                            }
                        }
                    }
                    "response.output_item.done" => {
                        if let Some(item) = v.get("item") {
                            match item.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
                                "function_call" => {
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
                                    if text_block_open {
                                        yield StreamEvent::ContentBlockStop { index: block_index };
                                        block_index += 1;
                                        text_block_open = false;
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
                        if text_block_open {
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
                    _ => {}
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
        let mut stream = std::pin::pin!(parse_codex_sse_byte_stream(byte_stream));
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
}
