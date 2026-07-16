#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Tests: prompt update ordering (ACK before first notification).
//!
//! Verifies that `session/prompt` always sends the JSON-RPC response
//! for the prompt request id before any `session/update` notifications
//! reach the sink.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_client_protocol_schema::v2::{self, SessionUpdate, StopReason};
use allthecodes_acp::capabilities::{AcpClientCapabilities, STRUCTURED_BRIEF_UPDATE};
use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::updates::AcpUpdateMapper;
use allthecodes_acp::AcpEngineParams;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_types::brief::{BriefMessageLevel, BriefMessagePayload, BriefMessageStatus};
use allthecodes_types::message::{
    AssistantMessage, ContentBlock as InternalContentBlock, StreamEvent, ToolResultContent, Usage,
};
use allthecodes_types::sdk::{
    ResultSubtype, SdkApiRetry, SdkAssistantMessage, SdkGoalUpdated, SdkMessage, SdkResult,
    SdkStreamEvent, SdkTombstone, SdkToolUseSummary, SdkUserReplay, UsageTracking,
};
use uuid::Uuid;

use support::{is_response, RuntimeHarness};

/// A factory that creates a real QueryEngine for test prompting.
pub struct TestEngineFactory;

impl AcpEngineFactory for TestEngineFactory {
    fn create_engine(&self, params: AcpEngineParams) -> anyhow::Result<Arc<QueryEngine>> {
        let cwd = params.cwd.to_string_lossy().to_string();
        let config = QueryEngineConfig {
            cwd,
            tools: vec![],
            max_turns: Some(1),
            initial_messages: params.initial_messages,
            verbose: false,
            resolved_model: Some("test-model".to_string()),
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            auto_save_session: false,
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_budget_usd: None,
            task_budget: None,
            verification_policy: None,
            agent_context: None,
        };
        Ok(Arc::new(QueryEngine::new(config)))
    }
}

/// Test: for a `session/prompt` request, the JSON-RPC response (the
/// `PromptResponse` / ACK) must be written before the first
/// `session/update` notification.
///
/// Current broken behavior: the spawned engine task sends
/// ``session/update`` (state=running) before the response is returned.
#[tokio::test]
async fn prompt_acks_before_first_update() {
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    // 1. session/new — create a session via the runtime dispatch.
    let new_params = serde_json::json!({
        "cwd": std::env::current_dir().unwrap(),
        "additionalDirectories": [],
        "mcpServers": {},
    });
    let (new_resp, _pre) = harness
        .send_request_and_capture("session/new", Some(new_params))
        .await;
    assert!(new_resp.is_some(), "session/new should return a response");
    assert!(
        new_resp.as_ref().unwrap().get("result").is_some(),
        "session/new response should have a result, got: {:?}",
        new_resp
    );
    let session_id = new_resp
        .as_ref()
        .unwrap()
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .and_then(|v| v.as_str())
        .expect("session/new response missing sessionId")
        .to_string();

    // 2. session/prompt — send a prompt.  The bug: the spawned task sends
    //    session/update(state=running) before dispatch_request returns.
    let prompt_params = serde_json::json!({
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": "Hello"}]
    });
    let messages = harness
        .send_request_with_response_gap("session/prompt", Some(prompt_params))
        .await;

    let first_response = messages
        .iter()
        .position(is_response)
        .expect("session/prompt should write a JSON-RPC response");
    let first_update = messages
        .iter()
        .position(support::is_session_update)
        .expect("session/prompt should eventually write a session/update");

    assert!(
        first_response < first_update,
        "session/update arrived before prompt ACK: {messages:?}"
    );
}

fn test_cwd() -> PathBuf {
    std::env::current_dir().unwrap()
}

fn mapper() -> AcpUpdateMapper {
    AcpUpdateMapper::new(v2::SessionId::new("session-1"), test_cwd())
}

fn structured_brief_mapper() -> AcpUpdateMapper {
    AcpUpdateMapper::new_with_client_capabilities(
        v2::SessionId::new("session-1"),
        test_cwd(),
        AcpClientCapabilities {
            structured_brief: true,
        },
    )
}

fn brief_msg() -> SdkMessage {
    SdkMessage::BriefMessage(BriefMessagePayload {
        message: "Build finished.".into(),
        status: BriefMessageStatus::Proactive,
        attachments: vec!["report.md".into()],
        level: Some(BriefMessageLevel::Warning),
        source_tool_name: Some("Brief".into()),
        tool_use_id: Some("toolu_brief".into()),
        session_id: Some("allthecodes-session-id".into()),
        timestamp: Some(1_783_273_215_000),
    })
}

fn stream_msg(event: StreamEvent) -> SdkMessage {
    SdkMessage::StreamEvent(SdkStreamEvent {
        event,
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
    })
}

fn assistant_msg(blocks: Vec<InternalContentBlock>) -> SdkMessage {
    SdkMessage::Assistant(SdkAssistantMessage {
        message: AssistantMessage {
            uuid: Uuid::nil(),
            timestamp: 0,
            role: "assistant".into(),
            content: blocks,
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        session_id: "session-1".into(),
        parent_tool_use_id: None,
    })
}

fn tool_result_msg(tool_use_id: &str, text: &str, is_error: bool) -> SdkMessage {
    SdkMessage::UserReplay(SdkUserReplay {
        content: text.into(),
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
        timestamp: 0,
        is_replay: false,
        is_synthetic: false,
        tool_use_result: Some(text.into()),
        source_tool_assistant_uuid: None,
        content_blocks: Some(vec![InternalContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: ToolResultContent::Text(text.into()),
            is_error,
        }]),
    })
}

fn sdk_result(subtype: ResultSubtype) -> SdkResult {
    SdkResult {
        subtype,
        is_error: false,
        duration_ms: 10,
        duration_api_ms: 7,
        num_turns: 1,
        result: "done".into(),
        stop_reason: None,
        session_id: "session-1".into(),
        total_cost_usd: 0.0,
        usage: UsageTracking {
            total_input_tokens: 12,
            total_output_tokens: 5,
            total_cache_read_tokens: 0,
            total_cache_creation_tokens: 0,
            total_reasoning_output_tokens: 0,
            total_cost_usd: 0.0,
            api_call_count: 1,
        },
        permission_denials: Vec::new(),
        structured_output: None,
        uuid: Uuid::nil(),
        errors: Vec::new(),
    }
}

fn json(update: &SessionUpdate) -> serde_json::Value {
    serde_json::to_value(update).unwrap()
}

fn update_kind(update: &SessionUpdate) -> String {
    json(update)["sessionUpdate"].as_str().unwrap().to_string()
}

fn text_delta(index: usize, text: &str) -> SdkMessage {
    stream_msg(StreamEvent::ContentBlockDelta {
        index,
        delta: serde_json::json!({ "type": "text_delta", "text": text }),
    })
}

fn thinking_delta(index: usize, thinking: &str) -> SdkMessage {
    stream_msg(StreamEvent::ContentBlockDelta {
        index,
        delta: serde_json::json!({ "type": "thinking_delta", "thinking": thinking }),
    })
}

fn tool_use_msg(tool_use_id: &str, name: &str, input: serde_json::Value) -> SdkMessage {
    assistant_msg(vec![InternalContentBlock::ToolUse {
        id: tool_use_id.into(),
        name: name.into(),
        input,
    }])
}

#[test]
fn state_running_precedes_agent_content() {
    let mut mapper = mapper();
    let mut updates = mapper.map_message(&stream_msg(StreamEvent::MessageStart {
        usage: Usage::default(),
    }));
    updates.extend(mapper.map_message(&text_delta(0, "Hello")));

    let kinds: Vec<_> = updates.iter().map(update_kind).collect();
    assert_eq!(kinds, vec!["state_update", "agent_message_chunk"]);
}

#[test]
fn stream_text_delta_becomes_agent_message_chunk() {
    let mut mapper = mapper();
    let first = mapper.map_message(&text_delta(0, "Hello"));
    let second = mapper.map_message(&text_delta(0, " world"));

    let first_json = json(&first[0]);
    let second_json = json(&second[0]);

    assert_eq!(first_json["sessionUpdate"], "agent_message_chunk");
    assert_eq!(first_json["messageId"], "agent-msg-1");
    assert_eq!(first_json["content"]["type"], "text");
    assert_eq!(first_json["content"]["text"], "Hello");
    assert_eq!(second_json["messageId"], "agent-msg-1");
    assert_eq!(second_json["content"]["text"], " world");
}

#[test]
fn thinking_delta_becomes_agent_thought_chunk() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&thinking_delta(0, "Reasoning"));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "agent_thought_chunk");
    assert_eq!(value["messageId"], "thought-1");
    assert_eq!(value["content"]["type"], "text");
    assert_eq!(value["content"]["text"], "Reasoning");
}

#[test]
fn result_sends_usage_and_idle() {
    let mut mapper = mapper();
    let updates = mapper.map_result(&sdk_result(ResultSubtype::Success), None);
    let values: Vec<_> = updates.iter().map(json).collect();

    assert_eq!(values[0]["sessionUpdate"], "usage_update");
    assert_eq!(values[0]["used"], 12);
    assert_eq!(values[0]["size"], 5);
    assert_eq!(values[1]["sessionUpdate"], "state_update");
    assert_eq!(values[1]["state"], "idle");
    assert_eq!(values[1]["stopReason"], "end_turn");
}

#[test]
fn max_turns_maps_to_max_turn_requests() {
    let mut mapper = mapper();
    let updates = mapper.map_result(&sdk_result(ResultSubtype::ErrorMaxTurns), None);
    let value = json(updates.last().unwrap());

    assert_eq!(value["sessionUpdate"], "state_update");
    assert_eq!(value["state"], "idle");
    assert_eq!(value["stopReason"], "max_turn_requests");
}

#[test]
fn cancel_sends_idle_cancelled() {
    let mut mapper = mapper();
    let updates = mapper.map_result(
        &sdk_result(ResultSubtype::Success),
        Some(StopReason::Cancelled),
    );
    let value = json(updates.last().unwrap());

    assert_eq!(value["sessionUpdate"], "state_update");
    assert_eq!(value["state"], "idle");
    assert_eq!(value["stopReason"], "cancelled");
}

#[test]
fn assistant_tool_use_starts_tool_call() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&tool_use_msg(
        "tool-1",
        "Read",
        serde_json::json!({ "file_path": "src/lib.rs" }),
    ));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "tool_call_update");
    assert_eq!(value["toolCallId"], "tool-1");
    assert_eq!(value["status"], "in_progress");
    assert_eq!(value["kind"], "read");
    assert_eq!(value["title"], "Read src/lib.rs");
    assert_eq!(value["rawInput"]["file_path"], "src/lib.rs");
}

#[test]
fn tool_result_completes_tool_call() {
    let mut mapper = mapper();
    mapper.map_message(&tool_use_msg(
        "tool-1",
        "Read",
        serde_json::json!({ "file_path": "src/lib.rs" }),
    ));

    let updates = mapper.map_message(&tool_result_msg("tool-1", "file contents", false));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "tool_call_update");
    assert_eq!(value["toolCallId"], "tool-1");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["rawOutput"], "file contents");
    assert_eq!(value["content"][0]["type"], "content");
    assert_eq!(value["content"][0]["content"]["text"], "file contents");
}

#[test]
fn tool_error_fails_tool_call() {
    let mut mapper = mapper();
    mapper.map_message(&tool_use_msg(
        "tool-1",
        "Bash",
        serde_json::json!({ "command": "false" }),
    ));

    let updates = mapper.map_message(&tool_result_msg("tool-1", "exit 1", true));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "tool_call_update");
    assert_eq!(value["toolCallId"], "tool-1");
    assert_eq!(value["status"], "failed");
    assert_eq!(value["rawOutput"], "exit 1");
}

#[test]
fn read_tool_location_is_absolute() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&tool_use_msg(
        "tool-1",
        "Read",
        serde_json::json!({ "file_path": "src/lib.rs" }),
    ));
    let value = json(&updates[0]);
    let location = value["locations"][0]["path"].as_str().unwrap();

    assert!(
        Path::new(location).is_absolute(),
        "{location} is not absolute"
    );
    assert!(
        location.ends_with("src/lib.rs"),
        "unexpected location path: {location}"
    );
}

#[test]
fn edit_tool_without_old_new_does_not_fabricate_diff() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&tool_use_msg(
        "tool-1",
        "Edit",
        serde_json::json!({ "file_path": "src/lib.rs" }),
    ));
    let value = json(&updates[0]);
    let contents = value
        .get("content")
        .and_then(|content| content.as_array())
        .cloned()
        .unwrap_or_default();

    assert!(!contents.iter().any(|item| item["type"] == "diff"));
}

#[test]
fn tool_progress_appends_content_chunk() {
    let mut mapper = mapper();
    mapper.map_message(&tool_use_msg(
        "tool-1",
        "Bash",
        serde_json::json!({ "command": "cargo test" }),
    ));

    let update = mapper
        .map_tool_progress("tool-1", "running tests")
        .expect("progress should map for known tool");
    let value = json(&update);

    assert_eq!(value["sessionUpdate"], "tool_call_content_chunk");
    assert_eq!(value["toolCallId"], "tool-1");
    assert_eq!(value["content"]["type"], "content");
    assert_eq!(value["content"]["content"]["text"], "running tests");
}

#[test]
fn api_retry_maps_to_kinded_thought() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&SdkMessage::ApiRetry(SdkApiRetry {
        attempt: 2,
        max_retries: 3,
        retry_delay_ms: 500,
        error_status: Some(429),
        error: "rate limited".into(),
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
    }));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "agent_thought");
    assert_eq!(value["_meta"]["kind"], "api_retry");
    assert!(value["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("rate limited"));
}

#[test]
fn tool_use_summary_maps_to_kinded_thought() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&SdkMessage::ToolUseSummary(SdkToolUseSummary {
        summary: "Read two files".into(),
        preceding_tool_use_ids: vec!["tool-1".into(), "tool-2".into()],
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
    }));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "agent_thought");
    assert_eq!(value["_meta"]["kind"], "tool_use_summary");
    assert_eq!(value["_meta"]["precedingToolUseIds"][0], "tool-1");
    assert_eq!(value["content"][0]["text"], "Read two files");
}

#[test]
fn goal_updated_parseable_items_becomes_plan_update() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&SdkMessage::GoalUpdated(SdkGoalUpdated {
        event: "updated".into(),
        goal: serde_json::json!({
            "entries": [
                { "content": "Inspect mapper", "status": "completed", "priority": "high" },
                { "content": "Run tests", "status": "in_progress" }
            ]
        }),
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
    }));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "plan_update");
    assert_eq!(value["plan"]["type"], "items");
    assert_eq!(value["plan"]["id"], "goal-session-1");
    assert_eq!(value["plan"]["entries"][0]["content"], "Inspect mapper");
    assert_eq!(value["plan"]["entries"][0]["status"], "completed");
}

#[test]
fn tombstone_maps_to_kinded_thought() {
    let mut mapper = mapper();
    let message_uuid = Uuid::new_v4();
    let updates = mapper.map_message(&SdkMessage::Tombstone(SdkTombstone {
        message: AssistantMessage {
            uuid: message_uuid,
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        session_id: "session-1".into(),
        uuid: Uuid::nil(),
    }));
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "agent_thought");
    assert_eq!(value["_meta"]["kind"], "tombstone");
    assert_eq!(
        value["_meta"]["abandonedMessageId"],
        message_uuid.to_string()
    );
}

#[test]
fn brief_message_without_client_opt_in_keeps_text_fallback() {
    let mut mapper = mapper();
    let updates = mapper.map_message(&brief_msg());
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], "agent_message");
    assert_eq!(value["messageId"], "agent-msg-1");
    assert_eq!(value["content"][0]["type"], "text");
    assert_eq!(value["content"][0]["text"], "Build finished.");
    assert!(
        value.get("_meta").is_none(),
        "legacy fallback must not send structured Brief metadata: {value:?}"
    );
}

#[test]
fn brief_message_with_client_opt_in_preserves_structured_payload() {
    let mut mapper = structured_brief_mapper();
    let updates = mapper.map_message(&brief_msg());
    let value = json(&updates[0]);

    assert_eq!(value["sessionUpdate"], STRUCTURED_BRIEF_UPDATE);
    assert_eq!(value["messageId"], "brief-msg-1");
    assert_eq!(value["sessionId"], "session-1");
    assert_eq!(value["message"], "Build finished.");
    assert_eq!(value["status"], "proactive");
    assert_eq!(value["attachments"][0], "report.md");
    assert_eq!(value["level"], "warning");
    assert_eq!(value["sourceToolName"], "Brief");
    assert_eq!(value["toolUseId"], "toolu_brief");
    assert_eq!(value["sourceSessionId"], "allthecodes-session-id");
    assert_eq!(value["timestamp"], 1_783_273_215_000_i64);
    assert!(
        value.get("content").is_none(),
        "structured Brief update must not degrade to text content: {value:?}"
    );
}

#[test]
fn structured_brief_message_ids_do_not_consume_agent_message_ids() {
    let mut mapper = structured_brief_mapper();
    let brief = mapper.map_message(&brief_msg());
    let agent = mapper.map_message(&assistant_msg(vec![InternalContentBlock::Text {
        text: "normal text".into(),
    }]));

    assert_eq!(json(&brief[0])["messageId"], "brief-msg-1");
    assert_eq!(json(&agent[0])["messageId"], "agent-msg-1");
}
