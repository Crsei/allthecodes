use super::*;

// --- SSE line parsing ---

// -----------------------------------------------------------------------
// SSE line parsing
// -----------------------------------------------------------------------

#[test]
fn test_sse_line_parsing_message_start() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":0}}}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::MessageStart { usage } => {
            assert_eq!(usage.input_tokens, 100);
            assert_eq!(usage.output_tokens, 0);
        }
        other => panic!("expected MessageStart, got {:?}", other),
    }
}

#[test]
fn test_sse_line_parsing_content_block_start() {
    let sse_text = "\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::ContentBlockStart {
            index,
            content_block: _content_block,
        } => {
            assert_eq!(*index, 0);
        }
        other => panic!("expected ContentBlockStart, got {:?}", other),
    }
}

#[test]
fn test_sse_line_parsing_multiple_events() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":10}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 6);

    assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(events[1], StreamEvent::ContentBlockStart { .. }));
    assert!(matches!(events[2], StreamEvent::ContentBlockDelta { .. }));
    assert!(matches!(events[3], StreamEvent::ContentBlockStop { .. }));
    assert!(matches!(events[4], StreamEvent::MessageDelta { .. }));
    assert!(matches!(events[5], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_ping_ignored() {
    let sse_text = "\
event: ping\n\
data: {}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    // ping should be ignored, only message_stop should come through
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_accumulator_integration() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":42,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello, world!\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    let mut acc = StreamAccumulator::new();
    for event in &events {
        acc.process_event(event);
    }

    let msg = acc.build("claude-sonnet-4-20250514");
    assert_eq!(msg.role, "assistant");
    assert_eq!(msg.content.len(), 1);
    assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));

    if let allthecodes_types::message::ContentBlock::Text { text } = &msg.content[0] {
        assert_eq!(text, "Hello, world!");
    } else {
        panic!("expected Text content block");
    }

    assert_eq!(msg.usage.as_ref().unwrap().input_tokens, 42);
    assert_eq!(msg.usage.as_ref().unwrap().output_tokens, 5);
}

#[test]
fn test_sse_line_parsing_no_trailing_newline() {
    // SSE text without a trailing blank line should still parse
    let sse_text = "\
event: message_stop\n\
data: {\"type\":\"message_stop\"}";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_empty_text() {
    let events = parse_sse_text("").unwrap();
    assert!(events.is_empty());
}

#[test]
fn regression_anthropic_stream_error_event_currently_ignored() {
    // Phase 0 risk: event:error is currently discarded, so overload/auth
    // failures can disappear from the stream parser instead of surfacing.
    let err =
        parse_sse_text(STREAM_ERROR_EVENT_SSE).expect_err("error event should not be ignored");
    assert!(err.to_string().contains("overloaded_error"));
}

#[test]
fn anthropic_stream_error_event_preserves_available_metadata() {
    let sse_text = "\
event: error\n\
data: {\"type\":\"error\",\"status\":529,\"request_id\":\"req_sse\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\
\n";

    let err = parse_sse_text(sse_text).expect_err("error event should surface");
    let normalized = err
        .downcast_ref::<crate::api::streaming::NormalizedApiError>()
        .expect("stream error should use normalized API error");

    assert_eq!(normalized.provider, "anthropic");
    assert_eq!(normalized.status, Some(529));
    assert_eq!(normalized.request_id.as_deref(), Some("req_sse"));
    assert_eq!(normalized.error_type.as_deref(), Some("overloaded_error"));
    assert_eq!(normalized.message, "Overloaded");
}
