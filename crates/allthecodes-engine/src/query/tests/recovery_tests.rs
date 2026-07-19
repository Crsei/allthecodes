use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;

use super::super::super::deps::CompactionResult;
use super::super::*;
use super::mocks::{
    has_api_error_containing, make_auto_compact_tracking, make_query_params, make_text_response,
    make_text_response_with_stop_and_output_tokens, make_user_message_for_test,
    request_start_count, MockDeps, MockStreamStep,
};
use crate::types::app_state::AppState;
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, QueryYield, StreamEvent, Usage,
};

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value.as_ref());
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[tokio::test]
async fn test_prompt_too_long_reactive_compact_retries_model_call() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("prompt_too_long: context window exceeded".to_string()),
        MockStreamStep::Response(make_text_response("Recovered after compact.")),
    ]));
    deps.set_reactive_compact_result(Some(CompactionResult {
        messages: initial_messages.clone(),
        tracking: make_auto_compact_tracking(),
    }));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "prompt_too_long recovery should retry the model call"
    );
    assert_eq!(
        deps.collapse_drain_calls.load(Ordering::SeqCst),
        1,
        "prompt_too_long should drain collapses before reactive compact"
    );
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        1,
        "prompt_too_long should attempt reactive compact once"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[1].max_output_tokens, None,
        "context recovery must not trigger max-output-token escalation"
    );

    let recovered = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.content.iter().any(|block| {
                matches!(block, ContentBlock::Text { text } if text == "Recovered after compact.")
            })
        } else {
            false
        }
    });
    assert!(recovered, "expected recovered assistant response");
}

#[tokio::test]
#[serial_test::serial]
async fn test_prompt_too_long_without_existing_state_does_not_activate_durable_proactive_state() {
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Error(
        "prompt_too_long: context window exceeded".to_string(),
    )]));

    let stream = query(make_query_params(initial_messages), deps);
    let _items: Vec<QueryYield> = stream.collect().await;

    let state_path = home.path().join("daemon").join("proactive-state.json");
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path).expect("durable proactive state"),
    )
    .unwrap();
    assert_eq!(state["active"], false);
    assert_eq!(state["context_blocked"], true);
    assert_eq!(state["blocked_reason"], "context_limit");
    assert!(state["next_tick_at"].is_null());
}

#[tokio::test]
async fn test_prompt_too_long_collapse_drain_retries_before_reactive_compact() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("prompt_too_long: context window exceeded".to_string()),
        MockStreamStep::Response(make_text_response("Recovered after collapse drain.")),
    ]));
    deps.set_collapse_drain_result(Some(CompactionResult {
        messages: initial_messages.clone(),
        tracking: make_auto_compact_tracking(),
    }));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "collapse drain recovery should retry the model call"
    );
    assert_eq!(deps.collapse_drain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        0,
        "reactive compact should not run when collapse drain succeeds"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[1].max_output_tokens, None,
        "collapse-drain retry must not trigger max-output-token escalation"
    );

    let recovered = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.content.iter().any(|block| {
                matches!(block, ContentBlock::Text { text } if text == "Recovered after collapse drain.")
            })
        } else {
            false
        }
    });
    assert!(recovered, "expected recovered assistant response");
}

#[tokio::test]
async fn test_prompt_too_long_terminals_after_collapse_and_reactive_fail() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Error(
        "prompt_too_long: context window exceeded".to_string(),
    )]));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert_eq!(deps.collapse_drain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(deps.reactive_compact_calls.load(Ordering::SeqCst), 1);
    assert!(items.iter().any(|item| {
        matches!(
            item,
            QueryYield::Message(Message::Assistant(message))
                if message.is_api_error_message
        )
    }));
}

#[tokio::test]
async fn test_fallback_model_retries_stream_start_capacity_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded: high demand".to_string()),
        MockStreamStep::Response(make_text_response("Recovered on fallback")),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use the fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "fallback should emit a second model request start"
    );

    let recorded = deps.recorded_params();
    let primary_model = AppState::default().main_loop_model;
    assert_eq!(
        recorded.len(),
        2,
        "primary failure should be retried once with fallback"
    );
    assert_eq!(recorded[0].model.as_deref(), Some(primary_model.as_str()));
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        !has_api_error_containing(&items, "529 overloaded"),
        "recoverable stream-start error should be withheld when fallback succeeds"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Recovered on fallback")
                ))
        )),
        "fallback response should be yielded as the assistant message"
    );
}

#[tokio::test]
async fn test_fallback_strips_signature_blocks_from_retry_messages() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded: high demand".to_string()),
        MockStreamStep::Response(make_text_response("Recovered without signed thinking")),
    ]));

    let signed_assistant = Message::Assistant(AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "assistant".to_string(),
        content: vec![
            ContentBlock::Text {
                text: "keep visible context".to_string(),
            },
            ContentBlock::Thinking {
                thinking: "private chain".to_string(),
                signature: Some("primary-model-signature".to_string()),
            },
            ContentBlock::RedactedThinking {
                data: "redacted-signature-payload".to_string(),
            },
            ContentBlock::ToolUse {
                id: "toolu_keep".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "Cargo.toml"}),
            },
        ],
        usage: Some(Usage::default()),
        stop_reason: Some("tool_use".to_string()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    });
    let mut params = make_query_params(vec![
        make_user_message_for_test("Use fallback with prior thinking"),
        signed_assistant,
    ]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let _items: Vec<QueryYield> = stream.collect().await;

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);

    let primary_assistant = recorded[0]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant(assistant) => Some(assistant),
            _ => None,
        })
        .expect("primary request should include assistant context");
    assert!(
        primary_assistant
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Thinking { .. })),
        "primary request keeps original signed thinking"
    );

    let fallback_assistant = recorded[1]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant(assistant) => Some(assistant),
            _ => None,
        })
        .expect("fallback request should include assistant context");
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        fallback_assistant.content.iter().all(|block| !matches!(
            block,
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
        )),
        "fallback request must not replay old model thinking signatures"
    );
    assert!(
        fallback_assistant.content.iter().any(|block| matches!(
            block,
            ContentBlock::Text { text } if text == "keep visible context"
        )),
        "fallback request should keep visible assistant context"
    );
    assert!(
        fallback_assistant.content.iter().any(|block| matches!(
            block,
            ContentBlock::ToolUse { id, .. } if id == "toolu_keep"
        )),
        "fallback request should keep tool_use context"
    );
}

#[tokio::test]
async fn test_fallback_tombstones_partial_assistant_after_stream_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Events(vec![
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({
                    "type": "text_delta",
                    "text": "orphaned partial"
                }),
            }),
            Err("529 overloaded during stream".to_string()),
        ]),
        MockStreamStep::Response(make_text_response("Recovered after tombstone")),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "partial stream failure should retry with fallback"
    );
    let tombstone = items.iter().find_map(|item| {
        if let QueryYield::Tombstone(tombstone) = item {
            Some(tombstone)
        } else {
            None
        }
    });
    let tombstone = tombstone.expect("expected tombstone for orphaned partial assistant");
    assert!(
        tombstone.message.content.iter().any(|block| matches!(
            block,
            ContentBlock::Text { text } if text == "orphaned partial"
        )),
        "tombstone should carry the orphaned partial assistant content"
    );

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        !has_api_error_containing(&items, "529 overloaded during stream"),
        "recoverable mid-stream error should be withheld when fallback succeeds"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Recovered after tombstone")
                ))
        )),
        "fallback response should be yielded after tombstone"
    );
}

#[tokio::test]
async fn test_chunk_read_error_after_text_accepts_partial_assistant() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Events(vec![
        Ok(StreamEvent::MessageStart {
            usage: Usage::default(),
        }),
        Ok(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        }),
        Ok(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: serde_json::json!({
                "type": "text_delta",
                "text": "partial but usable"
            }),
        }),
        Err("error reading OpenAI response chunk: connection closed".to_string()),
    ])]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Use compatible gateway")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        !has_api_error_containing(&items, "error reading OpenAI response chunk"),
        "chunk read errors after text should not replace the response with an API error"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text == "partial but usable"
                ))
        )),
        "partial text should be finalized as the assistant response"
    );
}

#[tokio::test]
async fn test_empty_chunk_read_error_retries_same_model_once() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Events(vec![
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
            Err("error reading OpenAI response chunk: connection reset".to_string()),
        ]),
        MockStreamStep::Response(make_text_response("Recovered on the same model")),
    ]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Retry empty stream")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 2);
    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].model, recorded[1].model);
    assert!(
        !has_api_error_containing(&items, "error reading OpenAI response chunk"),
        "the recovered interruption should not become a terminal API error"
    );
    assert!(items.iter().any(|item| matches!(
        item,
        QueryYield::Message(Message::Assistant(message))
            if message.content.iter().any(|block| matches!(
                block,
                ContentBlock::Text { text } if text == "Recovered on the same model"
            ))
    )));
}

#[tokio::test]
async fn test_chunk_error_after_partial_tool_use_is_not_retried() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Events(vec![
        Ok(StreamEvent::MessageStart {
            usage: Usage::default(),
        }),
        Ok(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "toolu_partial".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({}),
            },
        }),
        Err("error reading OpenAI response chunk: connection reset".to_string()),
    ])]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Do not duplicate tools")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert_eq!(deps.recorded_params().len(), 1);
    assert!(has_api_error_containing(
        &items,
        "error reading OpenAI response chunk"
    ));
}

#[tokio::test]
async fn test_fallback_exhaustion_releases_terminal_stream_start_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded primary".to_string()),
        MockStreamStep::Error("529 overloaded fallback".to_string()),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "primary should be retried once on the fallback model"
    );
    assert!(
        !has_api_error_containing(&items, "529 overloaded primary"),
        "recoverable primary failure should remain withheld"
    );
    assert!(
        has_api_error_containing(&items, "529 overloaded fallback"),
        "fallback exhaustion should release the final visible error"
    );
    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
}

#[tokio::test]
async fn test_stream_idle_watchdog_errors_when_first_event_never_arrives() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::DelayedEvents(vec![(
            Duration::from_millis(75),
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        )]),
        MockStreamStep::DelayedEvents(vec![(
            Duration::from_millis(75),
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        )]),
    ]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Wait for stream")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 2);
    assert!(
        has_api_error_containing(&items, "stream idle timeout"),
        "idle watchdog should surface a stream timeout error: {:?}",
        items
    );
}

#[tokio::test]
async fn test_delayed_progress_after_stall_threshold_is_accepted() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::DelayedEvents(
        vec![
            (
                Duration::from_millis(40),
                Ok(StreamEvent::MessageStart {
                    usage: Usage::default(),
                }),
            ),
            (
                Duration::from_millis(40),
                Ok(StreamEvent::ContentBlockStart {
                    index: 0,
                    content_block: ContentBlock::Text {
                        text: String::new(),
                    },
                }),
            ),
            (
                Duration::ZERO,
                Ok(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({"type": "text_delta", "text": "Recovered"}),
                }),
            ),
            (
                Duration::ZERO,
                Ok(StreamEvent::ContentBlockStop { index: 0 }),
            ),
            (
                Duration::ZERO,
                Ok(StreamEvent::MessageDelta {
                    delta: crate::types::message::MessageDelta {
                        stop_reason: Some("end_turn".to_string()),
                    },
                    usage: Some(Usage::default()),
                }),
            ),
            (Duration::ZERO, Ok(StreamEvent::MessageStop)),
        ],
    )]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Wait for useful progress")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert!(
        !has_api_error_containing(&items, "stream stalled"),
        "newly arrived progress must not be rejected retroactively: {:?}",
        items
    );
    assert!(items.iter().any(|item| {
        matches!(
            item,
            QueryYield::Message(Message::Assistant(message))
                if message.content.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text == "Recovered")
                })
        )
    }));
}

#[test]
fn test_message_start_counts_as_stream_progress() {
    assert!(super::super::super::recovery::is_stream_progress_event(
        &StreamEvent::MessageStart {
            usage: Usage::default(),
        }
    ));
}

#[tokio::test]
async fn test_max_tokens_recovery_escalates_next_request_limit() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response_with_stop_and_output_tokens("Partial answer", "max_tokens", 50),
        make_text_response("Continuation after larger output limit."),
    ]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Write a long answer")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "max_tokens should trigger one retry"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected two model calls");
    assert_eq!(params[0].max_output_tokens, None);
    assert_eq!(
        params[1].max_output_tokens,
        Some(super::super::super::recovery::ESCALATED_MAX_TOKENS)
    );
    assert_eq!(
        deps.collapse_drain_calls.load(Ordering::SeqCst),
        0,
        "max_tokens recovery must not invoke context collapse drain"
    );
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        0,
        "max_tokens recovery must not invoke reactive compact"
    );
}
