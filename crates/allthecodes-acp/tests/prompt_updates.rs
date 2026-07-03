//! Tests: prompt update ordering (ACK before first notification).
//!
//! Verifies that `session/prompt` always sends the JSON-RPC response
//! for the prompt request id before any `session/update` notifications
//! reach the sink.

mod support;

use std::sync::Arc;

use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::AcpEngineParams;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;

use support::RuntimeHarness;

/// A factory that creates a real QueryEngine for test prompting.
pub struct TestEngineFactory;

impl AcpEngineFactory for TestEngineFactory {
    fn create_engine(
        &self,
        params: AcpEngineParams,
    ) -> anyhow::Result<Arc<QueryEngine>> {
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
    let (prompt_resp, pre_prompt) = harness
        .send_request_and_capture("session/prompt", Some(prompt_params))
        .await;

    // 3. The response should be a valid PromptResponse.
    assert!(
        prompt_resp.is_some(),
        "session/prompt should return a response"
    );
    assert!(
        prompt_resp
            .as_ref()
            .unwrap()
            .get("result")
            .is_some(),
        "session/prompt should return a PromptResponse, got: {:?}",
        prompt_resp
    );

    // 4. NO session/update should arrive before the prompt ACK.  If we
    //    captured any session/update in `pre_prompt`, the ACK ordering is
    //    broken.
    let pre_updates: Vec<_> = pre_prompt
        .into_iter()
        .filter(|v| support::is_session_update(v))
        .collect();

    assert!(
        pre_updates.is_empty(),
        "session/update notification(s) arrived before prompt ACK: {pre_updates:?}"
    );
}
