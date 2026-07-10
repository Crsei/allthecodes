#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::sync::Arc;

use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v2::{
    self, PermissionOptionId, RequestPermissionOutcome, SelectedPermissionOutcome,
};
use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::permissions::map_permission_outcome;
use allthecodes_acp::{AcpEngineParams, PermissionRequestPayload};
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;

struct TestEngineFactory;

impl AcpEngineFactory for TestEngineFactory {
    fn create_engine(&self, params: AcpEngineParams) -> anyhow::Result<Arc<QueryEngine>> {
        let config = QueryEngineConfig {
            cwd: params.cwd.to_string_lossy().to_string(),
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

fn permission_payload() -> PermissionRequestPayload {
    PermissionRequestPayload {
        tool_use_id: "tool-1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({ "command": "cargo test" }),
        message: "run cargo test".to_string(),
        options: vec![
            "allow".to_string(),
            "always_allow".to_string(),
            "deny".to_string(),
        ],
        operation: None,
    }
}

async fn test_session(
    harness: &support::RuntimeHarness,
    session_id: &str,
) -> Arc<allthecodes_acp::session::AcpSession> {
    harness
        .session_manager
        .create_session(
            v2::SessionId::new(session_id),
            std::env::current_dir().unwrap(),
            Vec::new(),
            None,
        )
        .await
        .unwrap()
}

fn update_state(value: &serde_json::Value) -> Option<&str> {
    value
        .get("params")
        .and_then(|params| params.get("update"))
        .and_then(|update| update.get("state"))
        .and_then(|state| state.as_str())
}

async fn wait_for_messages(
    harness: &mut support::RuntimeHarness,
    count: usize,
) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    for _ in 0..100 {
        messages.extend(harness.drain_all());
        if messages.len() >= count {
            return messages;
        }
        tokio::task::yield_now().await;
    }
    panic!("timed out waiting for {count} messages, got {messages:?}");
}

#[tokio::test]
async fn permission_request_uses_client_request() {
    let mut harness = support::RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );
    let session = test_session(&harness, "session-1").await;
    let manager = harness.permission_manager.clone();

    let task = tokio::spawn({
        let session = session.clone();
        let sink = harness.sink.clone();
        let manager = manager.clone();
        async move {
            manager
                .request_permission(&session, &sink, permission_payload())
                .await
        }
    });

    let messages = wait_for_messages(&mut harness, 2).await;
    assert_eq!(update_state(&messages[0]), Some("requires_action"));
    assert_eq!(
        messages[1].get("method").and_then(|value| value.as_str()),
        Some("session/request_permission")
    );
    let request_id = messages[1]
        .get("id")
        .and_then(|value| value.as_str())
        .expect("permission request id");
    assert_eq!(request_id, "allthecodes-permission-session-1-1");
    assert_eq!(
        messages[1]["params"]["toolCall"]["toolCallId"],
        serde_json::json!("tool-1")
    );
    assert_eq!(
        messages[1]["params"]["options"][0]["optionId"],
        "allow_once"
    );

    let response_frame = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": serde_json::to_value(v2::RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                PermissionOptionId::new("allow_once"),
            )),
        ))
        .unwrap(),
    })
    .to_string();
    match allthecodes_acp::jsonrpc::parse_frame(&response_frame)
        .unwrap()
        .unwrap()
    {
        allthecodes_acp::jsonrpc::InboundMessage::Response { id, result, error } => {
            allthecodes_acp::runtime::handle_response(
                id,
                result,
                error,
                &harness.permission_manager,
            )
            .await;
        }
        other => panic!("expected JSON-RPC response, got {other:?}"),
    }

    let response = task.await.unwrap();
    assert_eq!(response.decision, "allow");
    let trailing = wait_for_messages(&mut harness, 1).await;
    assert_eq!(update_state(&trailing[0]), Some("running"));
}

#[test]
fn allow_once_maps_to_allow() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::new("allow_once"),
    ));
    assert_eq!(map_permission_outcome(&outcome).decision, "allow");
}

#[test]
fn always_allow_maps_to_always_allow() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::new("allow_always"),
    ));
    assert_eq!(map_permission_outcome(&outcome).decision, "always_allow");
}

#[test]
fn cancelled_maps_to_deny() {
    assert_eq!(
        map_permission_outcome(&RequestPermissionOutcome::Cancelled).decision,
        "deny"
    );
}

#[tokio::test]
async fn disconnect_denies_pending_permission() {
    let (sink_tx, sink_rx) = tokio::sync::mpsc::unbounded_channel();
    drop(sink_rx);
    let sink = allthecodes_acp::transport::AcpSink::new(sink_tx);
    let harness = support::RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );
    let session = test_session(&harness, "session-disconnect").await;

    let response = harness
        .permission_manager
        .request_permission(&session, &sink, permission_payload())
        .await;

    assert_eq!(response.decision, "deny");
    assert!(!harness.permission_manager.has_pending().await);
}

#[tokio::test]
async fn cancel_request_denies_pending_permission() {
    let mut harness = support::RuntimeHarness::new_with_factory(
        std::env::current_dir().unwrap(),
        Arc::new(TestEngineFactory),
    );
    let session = test_session(&harness, "session-cancel").await;

    let task = tokio::spawn({
        let session = session.clone();
        let sink = harness.sink.clone();
        let manager = harness.permission_manager.clone();
        async move {
            manager
                .request_permission(&session, &sink, permission_payload())
                .await
        }
    });

    let messages = wait_for_messages(&mut harness, 2).await;
    let request_id = messages[1]
        .get("id")
        .and_then(|value| value.as_str())
        .expect("permission request id");

    harness
        .permission_manager
        .cancel_request_id(&RequestId::Str(request_id.to_string()))
        .await;

    let response = task.await.unwrap();
    assert_eq!(response.decision, "deny");
    assert!(!harness.permission_manager.has_pending().await);
}
