use super::*;

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
async fn test_stream_idle_watchdog_errors_when_first_event_never_arrives() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::DelayedEvents(
        vec![(
            Duration::from_millis(75),
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        )],
    )]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Wait for stream")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert!(
        has_api_error_containing(&items, "stream idle timeout"),
        "idle watchdog should surface a stream timeout error: {:?}",
        items
    );
}

#[tokio::test]
async fn test_stream_stall_detection_errors_after_handshake_without_progress() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::DelayedEvents(
        vec![
            (
                Duration::from_millis(0),
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
        ],
    )]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Detect stall")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        items
            .iter()
            .any(|item| matches!(item, QueryYield::Stream(StreamEvent::MessageStart { .. }))),
        "stream handshake should be forwarded before the stall is detected"
    );
    assert!(
        has_api_error_containing(&items, "stream stalled"),
        "passive stall detection should surface a stream stalled error: {:?}",
        items
    );
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

#[tokio::test]
async fn test_image_tool_result_flows_as_blocks() {
    // Turn 1: model calls a tool (screenshot)
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me take a screenshot.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_screenshot".to_string(),
                    name: "mcp__computer-use__screenshot".to_string(),
                    input: serde_json::json!({}),
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
    };

    // Turn 2: model sees image and responds
    let text_response = make_text_response("I can see the desktop.");

    let deps = Arc::new(ImageMockDeps::new(vec![tool_response, text_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Take a screenshot".to_string()),
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

    // Find the tool result user message
    let tool_result_msg = items.iter().find_map(|item| {
        if let QueryYield::Message(Message::User(user_msg)) = item {
            if user_msg.is_meta && user_msg.source_tool_assistant_uuid.is_some() {
                return Some(user_msg);
            }
        }
        None
    });

    let user_msg = tool_result_msg.expect("expected a tool result user message");

    // Verify the tool result contains structured Blocks (not plain Text)
    match &user_msg.content {
        MessageContent::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1, "expected 1 tool_result block");
            match &blocks[0] {
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    assert!(!is_error);
                    match content {
                        ToolResultContent::Blocks(inner_blocks) => {
                            assert_eq!(
                                inner_blocks.len(),
                                2,
                                "expected 2 inner blocks (text + image)"
                            );
                            assert!(
                                matches!(&inner_blocks[0], ContentBlock::Text { .. }),
                                "first block should be text"
                            );
                            match &inner_blocks[1] {
                                ContentBlock::Image { source } => {
                                    assert_eq!(source.source_type, "base64");
                                    assert_eq!(source.media_type, "image/png");
                                    assert_eq!(source.data, "iVBORw0KGgoAAAANSUhEUg==");
                                }
                                other => panic!("second block should be Image, got {:?}", other),
                            }
                        }
                        ToolResultContent::Text(t) => {
                            panic!("expected Blocks in tool result, got Text: {}", t);
                        }
                    }
                }
                other => panic!("expected ToolResult block, got {:?}", other),
            }
        }
        _ => panic!("expected Blocks content"),
    }

    // Verify display_preview is used for tool_use_result (not raw base64)
    assert_eq!(
        user_msg.tool_use_result.as_deref(),
        Some("[Image: image/png]"),
        "tool_use_result should contain display preview, not base64 data"
    );

    // Verify there are 2 request starts (two turns = tool call + final response)
    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 2, "expected 2 API turns");
}

/// Full Computer Use smoke test:
///   Turn 1: model calls screenshot 鈫?receives image
///   Turn 2: model calls left_click 鈫?receives text confirmation
///   Turn 3: model responds with final text
#[tokio::test]
async fn test_computer_use_screenshot_click_round_trip() {
    // Turn 1: model takes a screenshot
    let screenshot_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me take a screenshot to see the desktop.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_screenshot".to_string(),
                    name: "mcp__computer-use__screenshot".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
    };

    // Turn 2: model sees image, decides to click
    let click_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I can see a button at (500, 300). Clicking it.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_click".to_string(),
                    name: "mcp__computer-use__left_click".to_string(),
                    input: serde_json::json!({"x": 500, "y": 300}),
                },
            ],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
    };

    // Turn 3: model confirms result
    let final_response = make_text_response("I clicked the button successfully.");

    let deps = Arc::new(CuMockDeps::new(vec![
        screenshot_response,
        click_response,
        final_response,
    ]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Click the button on screen".to_string()),
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

    // Verify 3 API turns (screenshot, click, final)
    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 3, "expected 3 API turns");

    // Verify 3 assistant messages
    let assistant_msgs: Vec<_> = items
        .iter()
        .filter_map(|item| {
            if let QueryYield::Message(Message::Assistant(msg)) = item {
                Some(msg)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(assistant_msgs.len(), 3, "expected 3 assistant messages");

    // Verify tool result messages
    let tool_result_msgs: Vec<_> = items
        .iter()
        .filter_map(|item| {
            if let QueryYield::Message(Message::User(msg)) = item {
                if msg.is_meta && msg.source_tool_assistant_uuid.is_some() {
                    return Some(msg);
                }
            }
            None
        })
        .collect();
    assert_eq!(tool_result_msgs.len(), 2, "expected 2 tool result messages");

    // First tool result (screenshot) should have Blocks content with Image
    match &tool_result_msgs[0].content {
        MessageContent::Blocks(blocks) => match &blocks[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert!(
                    matches!(content, ToolResultContent::Blocks(_)),
                    "screenshot result should be Blocks (image), got Text"
                );
            }
            other => panic!("expected ToolResult, got {:?}", other),
        },
        _ => panic!("expected Blocks content"),
    }

    // Second tool result (click) should have Text content
    match &tool_result_msgs[1].content {
        MessageContent::Blocks(blocks) => match &blocks[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert!(
                    matches!(content, ToolResultContent::Text(_)),
                    "click result should be Text"
                );
            }
            other => panic!("expected ToolResult, got {:?}", other),
        },
        _ => panic!("expected Blocks content"),
    }

    // Final message should be text
    let final_msg = assistant_msgs.last().unwrap();
    assert!(
        final_msg
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { text } if text.contains("clicked"))),
        "final message should mention clicking"
    );
}
