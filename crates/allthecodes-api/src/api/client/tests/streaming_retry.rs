use super::*;

// --- Streaming / retry ---

struct FlakyStreamProvider {
    calls: Arc<AtomicUsize>,
    fail_times: usize,
    error: &'static str,
}

struct StaticStreamProvider {
    events: Vec<StreamEvent>,
}

struct PartialThenErrorStreamProvider;

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for StaticStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Ok(Box::pin(futures::stream::iter(
            self.events.clone().into_iter().map(Ok),
        )))
    }
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for PartialThenErrorStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let events = vec![
            Ok(StreamEvent::MessageStart {
                usage: allthecodes_types::message::Usage {
                    input_tokens: 11,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: allthecodes_types::message::ContentBlock::Text {
                    text: String::new(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({"type": "text_delta", "text": "partial"}),
            }),
            Err(anyhow::anyhow!(crate::api::streaming::NormalizedApiError {
                provider: "anthropic".to_string(),
                status: Some(529),
                request_id: Some("req_partial".to_string()),
                error_type: Some("overloaded_error".to_string()),
                message: "Overloaded".to_string(),
            })),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for FlakyStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index < self.fail_times {
            anyhow::bail!("{}", self.error);
        }

        Ok(Box::pin(futures::stream::empty::<Result<StreamEvent>>()))
    }
}

fn minimal_stream_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
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
    }
}

fn retry_test_config(max_retries: usize) -> crate::api::retry::RetryConfig {
    crate::api::retry::RetryConfig {
        max_retries,
        initial_delay_ms: 0,
        max_delay_ms: 0,
        backoff_multiplier: 1.0,
        retryable_status_codes: vec![429, 500, 502, 503, 504, 529],
    }
}

#[tokio::test]
async fn messages_stream_retries_retryable_stream_start_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(FlakyStreamProvider {
            calls: calls.clone(),
            fail_times: 1,
            error: "Provider qwen error (HTTP 500): upstream unavailable",
        }),
    };
    let observed_delays = Arc::new(Mutex::new(Vec::new()));
    let observed_delays_for_sleep = observed_delays.clone();

    let result = client
        .messages_stream_with_backoff(
            minimal_stream_request(),
            retry_test_config(2),
            move |delay| {
                observed_delays_for_sleep.lock().unwrap().push(delay);
                std::future::ready(())
            },
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *observed_delays.lock().unwrap(),
        vec![Duration::from_millis(0)]
    );
}

#[tokio::test]
async fn messages_stream_does_not_retry_nonretryable_stream_start_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(FlakyStreamProvider {
            calls: calls.clone(),
            fail_times: 1,
            error: "API error (HTTP 400): prompt is too long",
        }),
    };
    let observed_delays = Arc::new(Mutex::new(Vec::new()));
    let observed_delays_for_sleep = observed_delays.clone();

    let result = client
        .messages_stream_with_backoff(
            minimal_stream_request(),
            retry_test_config(2),
            move |delay| {
                observed_delays_for_sleep.lock().unwrap().push(delay);
                std::future::ready(())
            },
        )
        .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(observed_delays.lock().unwrap().is_empty());
}

#[tokio::test]
async fn messages_collects_stream_events_into_assistant_message() {
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(StaticStreamProvider {
            events: vec![
                StreamEvent::MessageStart {
                    usage: allthecodes_types::message::Usage {
                        input_tokens: 11,
                        output_tokens: 0,
                        reasoning_output_tokens: 0,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                },
                StreamEvent::ContentBlockStart {
                    index: 0,
                    content_block: allthecodes_types::message::ContentBlock::Text {
                        text: String::new(),
                    },
                },
                StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({"type": "text_delta", "text": "Hello"}),
                },
                StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({"type": "text_delta", "text": ", world"}),
                },
                StreamEvent::ContentBlockStop { index: 0 },
                StreamEvent::MessageDelta {
                    delta: allthecodes_types::message::MessageDelta {
                        stop_reason: Some("end_turn".to_string()),
                    },
                    usage: Some(allthecodes_types::message::Usage {
                        input_tokens: 0,
                        output_tokens: 7,
                        reasoning_output_tokens: 0,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    }),
                },
                StreamEvent::MessageStop,
            ],
        }),
    };

    let message = client.messages(minimal_stream_request()).await.unwrap();

    assert_eq!(message.role, "assistant");
    assert_eq!(message.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(message.usage.as_ref().unwrap().input_tokens, 11);
    assert_eq!(message.usage.as_ref().unwrap().output_tokens, 7);
    match &message.content[0] {
        allthecodes_types::message::ContentBlock::Text { text } => assert_eq!(text, "Hello, world"),
        other => panic!("expected text content, got {:?}", other),
    }
}

#[tokio::test]
async fn messages_propagates_partial_stream_error_instead_of_fake_success() {
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(PartialThenErrorStreamProvider),
    };

    let err = client
        .messages(minimal_stream_request())
        .await
        .expect_err("partial stream error must not become an assistant message");
    let msg = err.to_string();

    assert!(msg.contains("provider=anthropic"));
    assert!(msg.contains("status=529"));
    assert!(msg.contains("request_id=req_partial"));
    assert!(msg.contains("type=overloaded_error"));
}

#[test]
fn normalizes_anthropic_error_body_with_request_metadata() {
    let err = crate::api::streaming::normalize_api_error_body(
        "anthropic",
        Some(400),
        r#"{"type":"error","request_id":"req_123","error":{"type":"invalid_request_error","message":"bad request"}}"#,
        None,
    );

    assert_eq!(err.provider, "anthropic");
    assert_eq!(err.status, Some(400));
    assert_eq!(err.request_id.as_deref(), Some("req_123"));
    assert_eq!(err.error_type.as_deref(), Some("invalid_request_error"));
    assert_eq!(err.message, "bad request");
    assert_eq!(
        err.to_string(),
        "API error provider=anthropic status=400 request_id=req_123 type=invalid_request_error: bad request"
    );
}
