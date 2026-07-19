use std::time::Duration;

use anyhow::Result;
use futures::Stream;
use serde_json::{json, Value};

use crate::api::provider_runtime::{ProviderStreamFailure, ProviderStreamFailureCategory};
use allthecodes_types::message::{ContentBlock, MessageDelta, StreamEvent, Usage};

use super::format::reasoning_output_tokens_from_usage;

#[cfg(test)]
pub(super) fn parse_codex_sse_byte_stream<S>(
    byte_stream: S,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    parse_codex_sse_byte_stream_with_timeout(byte_stream, Duration::from_secs(300))
}

pub(super) fn parse_codex_sse_byte_stream_with_timeout<S>(
    byte_stream: S,
    idle_timeout: Duration,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = Vec::new();
        let mut header_emitted = false;
        let mut block_index: usize = 0;
        let mut text_block_open = false;
        let mut thinking_block_open = false;
        let mut codex_tool_call_emitted = false;

        loop {
            let deadline = tokio::time::Instant::now() + idle_timeout;
            let frame = loop {
                if let Some((frame_end, delimiter_len)) = sse_frame_boundary(&buffer) {
                    let frame = buffer.drain(..frame_end).collect::<Vec<_>>();
                    buffer.drain(..delimiter_len);
                    break std::str::from_utf8(&frame).map(str::to_owned).map_err(|_| {
                        anyhow::Error::new(ProviderStreamFailure::new(
                            ProviderStreamFailureCategory::Decode,
                            "openai-codex",
                            "response stream contained invalid UTF-8",
                        ))
                    })?;
                }
                let chunk_result = match tokio::time::timeout_at(deadline, byte_stream.next()).await {
                    Ok(Some(chunk_result)) => chunk_result,
                    Ok(None) => Err(anyhow::Error::new(ProviderStreamFailure::new(
                        ProviderStreamFailureCategory::IncompleteResponse,
                        "openai-codex",
                        "stream closed before response.completed",
                    )))?,
                    Err(_) => Err(anyhow::Error::new(ProviderStreamFailure::new(
                        ProviderStreamFailureCategory::IdleTimeout,
                        "openai-codex",
                        format!("idle timeout waiting for a complete SSE frame after {}ms", idle_timeout.as_millis()),
                    )))?,
                };
                let chunk = chunk_result.map_err(|error| {
                    anyhow::Error::new(ProviderStreamFailure::new(
                        ProviderStreamFailureCategory::Transport,
                        "openai-codex",
                        format!("error reading response stream: {error}"),
                    ))
                })?;
                buffer.extend_from_slice(&chunk);
            };

            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                // Comments and event-only frames are valid transport activity.
                continue;
            }
            if data == "[DONE]" {
                Err(anyhow::Error::new(ProviderStreamFailure::new(
                    ProviderStreamFailureCategory::IncompleteResponse,
                    "openai-codex",
                    "received [DONE] before response.completed",
                )))?;
            }

            let v: Value = serde_json::from_str(&data).map_err(|_| {
                anyhow::Error::new(ProviderStreamFailure::new(
                    ProviderStreamFailureCategory::Decode,
                    "openai-codex",
                    "failed to decode Responses SSE JSON",
                ))
            })?;

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
                        let error = v
                            .get("response")
                            .and_then(|r| r.get("error"))
                            .or_else(|| v.get("error"));
                        let message = error
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("response.failed event received");
                        let mut failure = ProviderStreamFailure::new(
                            ProviderStreamFailureCategory::ProviderFailed,
                            "openai-codex",
                            message,
                        );
                        failure.status = error
                            .and_then(|e| e.get("status"))
                            .and_then(|status| status.as_u64())
                            .and_then(|status| u16::try_from(status).ok());
                        failure.request_id = v
                            .get("response")
                            .and_then(|response| response.get("id"))
                            .and_then(|id| id.as_str())
                            .map(str::to_string);
                        failure.error_type = error
                            .and_then(|e| e.get("code").or_else(|| e.get("type")))
                            .and_then(|code| code.as_str())
                            .map(str::to_string);
                        failure.retry_after_ms = error
                            .and_then(|e| e.get("retry_after_ms"))
                            .and_then(|delay| delay.as_u64());
                        Err(anyhow::Error::new(failure))?;
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
}

fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    (0..buffer.len()).find_map(|index| {
        if buffer[index..].starts_with(b"\r\n\r\n") {
            Some((index, 4))
        } else if buffer[index..].starts_with(b"\n\n") || buffer[index..].starts_with(b"\r\r") {
            Some((index, 2))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn terminal_failure_category<S>(stream: S) -> ProviderStreamFailureCategory
    where
        S: Stream<Item = Result<StreamEvent>>,
    {
        use futures::StreamExt;
        let mut stream = std::pin::pin!(stream);
        while let Some(item) = stream.next().await {
            if let Err(error) = item {
                return error
                    .downcast_ref::<ProviderStreamFailure>()
                    .expect("typed provider stream failure")
                    .category;
            }
        }
        panic!("expected terminal stream failure")
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

    #[tokio::test]
    async fn test_parse_codex_custom_tool_call_stream() {
        use futures::StreamExt;

        let patch = "*** Begin Patch\n*** End Patch";
        let chunks = vec![
            format!(
                "event: response.created\ndata: {}\n\n",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_1"}
                })
            ),
            format!(
                "event: response.output_item.done\ndata: {}\n\n",
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "custom_tool_call",
                        "call_id": "call_patch",
                        "name": "apply_patch",
                        "input": patch,
                    }
                })
            ),
            format!(
                "event: response.completed\ndata: {}\n\n",
                json!({
                    "type": "response.completed",
                    "response": {"id": "resp_1"}
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
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_patch");
                assert_eq!(name, "apply_patch");
                assert_eq!(input["input"], patch);
            }
            other => panic!("expected tool use block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn codex_eof_and_early_done_are_incomplete() {
        let created = bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({"type": "response.created", "response": {"id": "resp_eof"}})
        ));
        let eof_stream = futures::stream::iter(vec![Ok::<_, reqwest::Error>(created)]);
        assert_eq!(
            terminal_failure_category(parse_codex_sse_byte_stream_with_timeout(
                eof_stream,
                Duration::from_millis(100)
            ))
            .await,
            ProviderStreamFailureCategory::IncompleteResponse
        );

        let done_stream = futures::stream::iter(vec![Ok::<_, reqwest::Error>(
            bytes::Bytes::from_static(b"data: [DONE]\n\n"),
        )]);
        assert_eq!(
            terminal_failure_category(parse_codex_sse_byte_stream_with_timeout(
                done_stream,
                Duration::from_millis(100)
            ))
            .await,
            ProviderStreamFailureCategory::IncompleteResponse
        );
    }

    #[tokio::test]
    async fn codex_decode_failed_and_idle_are_typed() {
        let malformed = futures::stream::iter(vec![Ok::<_, reqwest::Error>(
            bytes::Bytes::from_static(b"data: {not-json}\n\n"),
        )]);
        assert_eq!(
            terminal_failure_category(parse_codex_sse_byte_stream_with_timeout(
                malformed,
                Duration::from_millis(100)
            ))
            .await,
            ProviderStreamFailureCategory::Decode
        );

        let failed = futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(
            format!(
                "data: {}\n\n",
                json!({
                    "type": "response.failed",
                    "response": {"id": "resp_failed", "error": {"message": "overloaded", "status": 503, "retry_after_ms": 25}}
                })
            ),
        ))]);
        assert_eq!(
            terminal_failure_category(parse_codex_sse_byte_stream_with_timeout(
                failed,
                Duration::from_millis(100)
            ))
            .await,
            ProviderStreamFailureCategory::ProviderFailed
        );

        let pending = futures::stream::pending::<Result<bytes::Bytes, reqwest::Error>>();
        assert_eq!(
            terminal_failure_category(parse_codex_sse_byte_stream_with_timeout(
                pending,
                Duration::from_millis(5)
            ))
            .await,
            ProviderStreamFailureCategory::IdleTimeout
        );
    }

    #[tokio::test]
    async fn heartbeat_and_unknown_frames_refresh_idle_until_completed() {
        use futures::StreamExt;
        let byte_stream = async_stream::stream! {
            yield Ok::<_, reqwest::Error>(bytes::Bytes::from_static(b": heartbeat\n\n"));
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_millis(8)).await;
                yield Ok(bytes::Bytes::from_static(b"data: {\"type\":\"response.future_event\"}\n\n"));
            }
            tokio::time::sleep(Duration::from_millis(8)).await;
            yield Ok(bytes::Bytes::from_static(b"data: {\"type\":\"response.completed\",\"response\":{}}\n\n"));
        };
        let events =
            parse_codex_sse_byte_stream_with_timeout(byte_stream, Duration::from_millis(20))
                .collect::<Vec<_>>()
                .await;
        assert!(events.iter().all(Result::is_ok));
        assert!(events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::MessageStop))));
    }

    #[tokio::test]
    async fn response_created_can_be_followed_by_reasoning_silence_below_idle_limit() {
        use futures::StreamExt;
        let byte_stream = async_stream::stream! {
            yield Ok::<_, reqwest::Error>(bytes::Bytes::from_static(
                b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_slow\"}}\n\n",
            ));
            tokio::time::sleep(Duration::from_millis(15)).await;
            yield Ok(bytes::Bytes::from_static(
                b"data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
            ));
        };
        let events =
            parse_codex_sse_byte_stream_with_timeout(byte_stream, Duration::from_millis(25))
                .collect::<Vec<_>>()
                .await;
        assert!(events.iter().all(Result::is_ok));
        assert!(events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::MessageStop))));
    }

    #[tokio::test]
    async fn codex_parser_accepts_crlf_and_utf8_split_across_transport_chunks() {
        use futures::StreamExt;
        let payload = format!(
            "data: {}\r\n\r\ndata: {}\r\n\r\n",
            json!({"type": "response.output_text.delta", "delta": "银河"}),
            json!({"type": "response.completed", "response": {}}),
        );
        let bytes = payload.as_bytes();
        let split = payload.find('河').expect("multibyte test marker") + 1;
        let byte_stream = futures::stream::iter(vec![
            Ok::<_, reqwest::Error>(bytes::Bytes::copy_from_slice(&bytes[..split])),
            Ok(bytes::Bytes::copy_from_slice(&bytes[split..])),
        ]);
        let events =
            parse_codex_sse_byte_stream_with_timeout(byte_stream, Duration::from_millis(100))
                .collect::<Vec<_>>()
                .await;
        assert!(events.iter().all(Result::is_ok));
        assert!(events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::MessageStop))));
    }
}
