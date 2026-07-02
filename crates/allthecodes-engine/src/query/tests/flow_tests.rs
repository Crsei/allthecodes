use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use uuid::Uuid;

use super::super::super::deps::ModelResponse;
use super::super::*;
use super::mocks::{
    has_api_error_containing, make_query_params, make_text_response, make_user_message_for_test,
    request_start_count, request_tool_input, run_observable_input_backfill_case,
    tool_use_summary_gate_case, yielded_tool_input, LoopTestTool, MockDeps, MockStreamStep,
};
use crate::types::config::{QueryGates, QueryParams, QuerySource};
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, QueryYield, StreamEvent, Usage,
    UserMessage,
};
use crate::types::tool::Tools;

#[tokio::test]
async fn test_simple_text_response_terminates() {
    let deps = Arc::new(MockDeps::new(vec![make_text_response("Hello, world!")]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Hi".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec!["You are a helpful assistant.".to_string()],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        items.len() >= 2,
        "expected at least 2 items, got {}",
        items.len()
    );
    assert!(matches!(items[0], QueryYield::RequestStart(_)));

    let has_assistant = items
        .iter()
        .any(|item| matches!(item, QueryYield::Message(Message::Assistant(_))));
    assert!(has_assistant, "expected an assistant message in output");
}

#[tokio::test]
async fn test_tool_use_then_text_response() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "echo hello"}),
                },
            ],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 80,
                ..Default::default()
            }),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let text_response = make_text_response("Done! The output was hello.");

    let deps = Arc::new(MockDeps::new(vec![tool_response, text_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Run echo hello".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 2, "expected 2 request starts (two turns)");

    let assistant_msgs = items
        .iter()
        .filter(|i| matches!(i, QueryYield::Message(Message::Assistant(_))))
        .count();
    assert_eq!(assistant_msgs, 2, "expected 2 assistant messages");

    assert!(
        !deps
            .tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "tool execution should not start before the model stream reaches message_stop"
    );
}

#[tokio::test]
async fn tool_use_summary_gate_defaults_off() {
    let items = tool_use_summary_gate_case(false).await;

    assert!(
        !items
            .iter()
            .any(|item| matches!(item, QueryYield::ToolUseSummary(_))),
        "default gates should not emit tool use summaries"
    );
}

#[tokio::test]
async fn tool_use_summary_gate_yields_summary_when_enabled() {
    let items = tool_use_summary_gate_case(true).await;
    let summary = items
        .iter()
        .find_map(|item| {
            if let QueryYield::ToolUseSummary(summary) = item {
                Some(summary)
            } else {
                None
            }
        })
        .expect("tool use summary should be emitted");

    assert!(summary.summary.contains("Bash"));
    assert!(summary.summary.contains("mock tool output"));
    assert_eq!(summary.preceding_tool_use_ids, vec!["tu_summary"]);
}

#[tokio::test]
async fn observable_input_backfill_clones_yield_without_changing_next_request_gate_off() {
    let (items, params) = run_observable_input_backfill_case(false).await;

    let yielded_input = yielded_tool_input(&items, "ObservableMessage");
    assert_eq!(
        yielded_input.get("type"),
        Some(&serde_json::json!("message"))
    );
    assert_eq!(
        yielded_input.get("recipient"),
        Some(&serde_json::json!("worker"))
    );
    assert_eq!(
        yielded_input.get("content"),
        Some(&serde_json::json!("hello"))
    );

    let request_input = request_tool_input(&params, 1, "ObservableMessage");
    assert!(request_input.get("type").is_none());
    assert!(request_input.get("recipient").is_none());
    assert!(request_input.get("content").is_none());
}

#[tokio::test]
async fn observable_input_backfill_matches_with_streaming_tool_gate_on() {
    let (items, params) = run_observable_input_backfill_case(true).await;

    let yielded_input = yielded_tool_input(&items, "ObservableMessage");
    assert_eq!(
        yielded_input.get("type"),
        Some(&serde_json::json!("message"))
    );
    assert_eq!(
        yielded_input.get("recipient"),
        Some(&serde_json::json!("worker"))
    );
    assert_eq!(
        yielded_input.get("content"),
        Some(&serde_json::json!("hello"))
    );

    let request_input = request_tool_input(&params, 1, "ObservableMessage");
    assert!(request_input.get("type").is_none());
    assert!(request_input.get("recipient").is_none());
    assert!(request_input.get("content").is_none());
}

#[tokio::test]
async fn streaming_tool_execution_gate_starts_safe_tools_before_message_stop() {
    let events = vec![
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::ToolUse {
                    id: "tu_safe_1".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
        ),
        (
            Duration::from_millis(5),
            Ok(StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlock::ToolUse {
                    id: "tu_safe_2".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 1 }),
        ),
        (
            Duration::from_millis(5),
            Ok(StreamEvent::ContentBlockStart {
                index: 2,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockDelta {
                index: 2,
                delta: serde_json::json!({
                    "type": "text_delta",
                    "text": "still streaming"
                }),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 2 }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageDelta {
                delta: crate::types::message::MessageDelta {
                    stop_reason: Some("tool_use".to_string()),
                },
                usage: Some(Usage::default()),
            }),
        ),
        (Duration::ZERO, Ok(StreamEvent::MessageStop)),
    ];
    let tools: Tools = vec![Arc::new(LoopTestTool {
        name: "SafeTool",
        concurrency_safe: true,
    })];
    let deps = Arc::new(
        MockDeps::from_steps(vec![
            MockStreamStep::DelayedEvents(events),
            MockStreamStep::Response(make_text_response("streamed tools complete")),
        ])
        .with_tools(tools)
        .with_tool_delay(Duration::from_millis(20)),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("run safe tools")]);
    params.gates.streaming_tool_execution = true;
    params.gates.deferred_tool_loading = false;

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;

    assert!(
        deps.tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "safe tools should start while the assistant stream is still open"
    );
    assert_eq!(
        deps.tool_execution_count.load(Ordering::SeqCst),
        2,
        "started streaming tools must not be re-executed after message_stop"
    );
    assert_eq!(
        deps.tool_completed_count.load(Ordering::SeqCst),
        2,
        "streaming tool tasks should be awaited before continuation"
    );
    assert_eq!(
        deps.max_active_tools.load(Ordering::SeqCst),
        2,
        "consecutive safe tools should run concurrently"
    );
    let tool_result_messages = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                QueryYield::Message(Message::User(user))
                    if user.is_meta && user.source_tool_assistant_uuid.is_some()
            )
        })
        .count();
    assert_eq!(tool_result_messages, 2);
    assert_eq!(request_start_count(&items), 2);
}

#[tokio::test]
async fn streaming_tool_execution_aborts_started_tools_on_stream_fallback() {
    let events = vec![
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::ToolUse {
                    id: "tu_orphan".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
        ),
        (
            Duration::from_millis(5),
            Err("529 overloaded during stream".to_string()),
        ),
    ];
    let tools: Tools = vec![Arc::new(LoopTestTool {
        name: "SafeTool",
        concurrency_safe: true,
    })];
    let deps = Arc::new(
        MockDeps::from_steps(vec![
            MockStreamStep::DelayedEvents(events),
            MockStreamStep::Response(make_text_response("fallback recovered")),
        ])
        .with_tools(tools)
        .with_tool_delay(Duration::from_millis(50)),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("run then fallback")]);
    params.gates.streaming_tool_execution = true;
    params.gates.deferred_tool_loading = false;
    params.fallback_model = Some("fallback-model".to_string());

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "stream interruption should retry on fallback model"
    );
    assert!(
        items
            .iter()
            .any(|item| matches!(item, QueryYield::Tombstone(_))),
        "partial primary assistant should be tombstoned before fallback"
    );
    assert!(
        deps.tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "the primary attempt should have started the safe tool before failing"
    );
    assert_eq!(
        deps.tool_completed_count.load(Ordering::SeqCst),
        0,
        "started primary-attempt tools should be aborted instead of awaited into fallback"
    );
    let tool_result_messages = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                QueryYield::Message(Message::User(user))
                    if user.is_meta && user.source_tool_assistant_uuid.is_some()
            )
        })
        .count();
    assert_eq!(
        tool_result_messages, 0,
        "orphaned primary-attempt tool results must not enter fallback transcript"
    );
    assert!(!has_api_error_containing(&items, "529 overloaded"));
}

#[tokio::test]
async fn test_abort_before_api_call() {
    let deps = Arc::new(MockDeps::new(vec![]));
    deps.aborted
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Hi".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    let has_assistant = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.stop_reason.as_deref() == Some("AbortedStreaming")
        } else {
            false
        }
    });
    assert!(has_assistant, "expected aborted assistant message");
}

#[test]
fn snip_projection_removes_requested_messages_only() {
    let keep = Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1,
        role: "user".to_string(),
        content: MessageContent::Text("keep".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    });
    let remove = Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 2,
        role: "user".to_string(),
        content: MessageContent::Text("remove".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    });
    let remove_id = remove.uuid().to_string();
    let keep_id = keep.uuid();
    let mut messages = vec![keep, remove];

    let removed = apply_snip_projection(
        &mut messages,
        &serde_json::json!({ "message_ids": [remove_id] }),
    );

    assert_eq!(removed, 1);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].uuid(), keep_id);
}
