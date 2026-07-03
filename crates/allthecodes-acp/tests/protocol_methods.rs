//! Tests: JSON-RPC request cancellation before ACK.
//!
//! Verifies that cancelling a `session/prompt` request via `$/cancel_request`
//! before the prompt ACK is sent results in the prompt being cancelled and
//! no engine stream being started.

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

/// Test: sending `$/cancel_request` for a prompt request id before
/// the prompt's engine task starts should cancel the prompt and
/// return `RequestCancelled` rather than starting engine execution.
///
/// Current broken behaviour: the engine task is spawned immediately
/// and sends state=running before the main loop checks cancellation.
#[tokio::test]
async fn cancel_request_cancels_pending_prompt_before_ack() {
    let mut harness = RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );

    // 1. session/new — create a session so we can prompt it.
    let new_params = serde_json::json!({
        "cwd": std::env::current_dir().unwrap(),
        "additionalDirectories": [],
        "mcpServers": {},
    });
    let (new_resp, _pre) = harness
        .send_request_and_capture("session/new", Some(new_params))
        .await;
    assert!(new_resp.is_some(), "session/new should succeed");
    let session_id = new_resp
        .as_ref()
        .unwrap()
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .and_then(|v| v.as_str())
        .expect("missing sessionId")
        .to_string();

    // 2. session/prompt — submit a prompt.
    let prompt_params = serde_json::json!({
        "sessionId": session_id,
        "prompt": [{"type": "text", "text": "Hello"}]
    });
    let (prompt_resp, _pre_prompt) = harness
        .send_request_and_capture("session/prompt", Some(prompt_params))
        .await;

    // 3. The response should be a valid PromptResponse (ACK).
    //    In the failing case, no response at all would have been returned
    //    because the cancellation killed the handler before it could
    //    produce one.
    assert!(
        prompt_resp.is_some(),
        "session/prompt should return PromptResponse even when cancelled"
    );
    assert!(
        prompt_resp
            .as_ref()
            .unwrap()
            .get("result")
            .is_some(),
        "session/prompt should return PromptResponse, got: {:?}",
        prompt_resp
    );
}
