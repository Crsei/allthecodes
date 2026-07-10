#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Tests: ACP session lifecycle parity.

mod support;

use std::sync::Arc;

use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::session::AcpTurnHandle;
use allthecodes_acp::AcpEngineParams;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_session::storage::{get_archived_session_file, get_session_file, save_session};
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, UserMessage,
};
use serial_test::serial;
use support::RuntimeHarness;
use uuid::Uuid;

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
            agent_context: None,
        };
        Ok(Arc::new(QueryEngine::new(config)))
    }
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
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

fn user_message(text: &str, timestamp: i64) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp,
        role: "user".into(),
        content: MessageContent::Text(text.into()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn assistant_message(text: &str, timestamp: i64) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: Uuid::new_v4(),
        timestamp,
        role: "assistant".into(),
        content: vec![ContentBlock::Text { text: text.into() }],
        usage: None,
        stop_reason: Some("end_turn".into()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    })
}

fn update_payload(value: &serde_json::Value) -> Option<&serde_json::Value> {
    if value.get("method").and_then(|method| method.as_str()) != Some("session/update") {
        return None;
    }
    value.pointer("/params/update")
}

fn update_kinds(messages: &[serde_json::Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(update_payload)
        .filter_map(|update| update.get("sessionUpdate"))
        .filter_map(|kind| kind.as_str())
        .collect()
}

fn update_texts(messages: &[serde_json::Value], kind: &str) -> Vec<String> {
    messages
        .iter()
        .filter_map(update_payload)
        .filter(|update| update.get("sessionUpdate").and_then(|value| value.as_str()) == Some(kind))
        .filter_map(|update| update.pointer("/content/0/text"))
        .filter_map(|text| text.as_str())
        .map(str::to_string)
        .collect()
}

fn result_sessions(response: &serde_json::Value) -> Vec<serde_json::Value> {
    response
        .pointer("/result/sessions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
}

fn result_session_ids(response: &serde_json::Value) -> Vec<String> {
    result_sessions(response)
        .into_iter()
        .filter_map(|session| {
            session
                .get("sessionId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect()
}

async fn send_list(
    harness: &mut RuntimeHarness,
    params: Option<serde_json::Value>,
) -> serde_json::Value {
    let (response, _pre_response) = harness
        .send_request_and_capture("session/list", params)
        .await;
    let response = response.expect("session/list should write a response");
    assert!(
        response.get("error").is_none(),
        "session/list returned error: {response:?}"
    );
    response
}

#[tokio::test]
#[serial]
async fn load_replays_before_response() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    let session_id = "load-replay-session";
    save_session(
        session_id,
        &[
            user_message("visible user", 100),
            assistant_message("visible assistant", 101),
        ],
        project.to_str().unwrap(),
    )
    .unwrap();

    let mut harness =
        RuntimeHarness::new_with_factory(project.clone(), Arc::new(TestEngineFactory));
    let params = serde_json::json!({
        "sessionId": session_id,
        "cwd": project,
        "additionalDirectories": [],
        "mcpServers": [],
    });

    let (response, pre_response) = harness
        .send_request_and_capture("session/load", Some(params))
        .await;

    assert!(
        response
            .as_ref()
            .and_then(|value| value.get("result"))
            .is_some(),
        "session/load should return LoadSessionResponse, got: {response:?}"
    );

    let kinds = update_kinds(&pre_response);
    assert!(
        kinds.contains(&"user_message"),
        "loaded user message should be replayed before response: {pre_response:?}"
    );
    assert!(
        kinds.contains(&"agent_message"),
        "loaded assistant message should be replayed before response: {pre_response:?}"
    );
    assert!(
        !matches!(kinds.as_slice(), ["state_update", "state_update", ..]),
        "placeholder running/idle updates must not substitute for replay: {pre_response:?}"
    );

    assert_eq!(
        update_texts(&pre_response, "user_message"),
        vec!["visible user".to_string()]
    );
    assert_eq!(
        update_texts(&pre_response, "agent_message"),
        vec!["visible assistant".to_string()]
    );
}

#[tokio::test]
#[serial]
async fn list_uses_workspace_filter_when_cwd_present() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let workspace_a = temp.path().join("workspace-a");
    let workspace_b = temp.path().join("workspace-b");
    std::fs::create_dir_all(&workspace_a).unwrap();
    std::fs::create_dir_all(&workspace_b).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    save_session(
        "workspace-a-session",
        &[user_message("a", 100)],
        workspace_a.to_str().unwrap(),
    )
    .unwrap();
    save_session(
        "workspace-b-session",
        &[user_message("b", 100)],
        workspace_b.to_str().unwrap(),
    )
    .unwrap();

    let mut harness =
        RuntimeHarness::new_with_factory(workspace_a.clone(), Arc::new(TestEngineFactory));

    let global = send_list(&mut harness, None).await;
    assert_eq!(
        result_session_ids(&global),
        vec![
            "workspace-a-session".to_string(),
            "workspace-b-session".to_string()
        ]
    );

    let filtered = send_list(
        &mut harness,
        Some(serde_json::json!({
            "cwd": workspace_a,
        })),
    )
    .await;
    assert_eq!(
        result_session_ids(&filtered),
        vec!["workspace-a-session".to_string()]
    );
    assert_eq!(
        filtered
            .pointer("/result/sessions/0/cwd")
            .and_then(|value| value.as_str()),
        Some(workspace_a.to_str().unwrap())
    );
}

#[tokio::test]
#[serial]
async fn list_serializes_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    for index in 0..101 {
        save_session(
            &format!("cursor-session-{index:03}"),
            &[user_message("cursor", index)],
            project.to_str().unwrap(),
        )
        .unwrap();
    }

    let mut harness =
        RuntimeHarness::new_with_factory(project.clone(), Arc::new(TestEngineFactory));
    let first_page = send_list(&mut harness, None).await;
    let next_cursor = first_page
        .pointer("/result/nextCursor")
        .and_then(|value| value.as_str())
        .expect("first page should include serialized nextCursor");
    assert_eq!(result_sessions(&first_page).len(), 100);

    let second_page = send_list(
        &mut harness,
        Some(serde_json::json!({
            "cursor": next_cursor,
        })),
    )
    .await;
    assert_eq!(result_sessions(&second_page).len(), 1);
    assert!(
        second_page.pointer("/result/nextCursor").is_none(),
        "last page should not include nextCursor: {second_page:?}"
    );
}

#[tokio::test]
#[serial]
async fn list_includes_workspace_meta() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    save_session(
        "meta-session",
        &[user_message("meta", 100), assistant_message("reply", 101)],
        project.to_str().unwrap(),
    )
    .unwrap();

    let mut harness =
        RuntimeHarness::new_with_factory(project.clone(), Arc::new(TestEngineFactory));
    let response = send_list(
        &mut harness,
        Some(serde_json::json!({
            "cwd": project,
        })),
    )
    .await;

    let meta = response
        .pointer("/result/sessions/0/_meta")
        .expect("session/list entries should include _meta");
    assert_eq!(
        meta.get("messageCount").and_then(|value| value.as_u64()),
        Some(2)
    );
    assert!(
        meta.get("workspaceKey")
            .and_then(|value| value.as_str())
            .is_some(),
        "workspaceKey missing from _meta: {response:?}"
    );
    assert_eq!(
        meta.get("workspaceRoot").and_then(|value| value.as_str()),
        Some(project.to_str().unwrap())
    );
}

#[tokio::test]
#[serial]
async fn close_waits_for_cancelled_idle_before_removal() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let mut harness =
        RuntimeHarness::new_with_factory(project.clone(), Arc::new(TestEngineFactory));
    let (new_response, _pre_response) = harness
        .send_request_and_capture(
            "session/new",
            Some(serde_json::json!({
                "cwd": project,
                "additionalDirectories": [],
                "mcpServers": {},
            })),
        )
        .await;
    let session_id = new_response
        .as_ref()
        .and_then(|value| value.pointer("/result/sessionId"))
        .and_then(|value| value.as_str())
        .expect("session/new response should include sessionId")
        .to_string();

    let session = harness
        .session_manager
        .get_session(&session_id)
        .await
        .expect("session should exist");
    *session.active_turn.lock().await = Some(AcpTurnHandle {
        cancel_requested: false,
    });

    let (close_response, pre_response) = harness
        .send_request_and_capture(
            "session/close",
            Some(serde_json::json!({
                "sessionId": session_id,
            })),
        )
        .await;
    assert!(
        close_response
            .as_ref()
            .and_then(|value| value.get("result"))
            .is_some(),
        "session/close should return CloseSessionResponse: {close_response:?}"
    );

    let cancelled_idle = pre_response.iter().any(|message| {
        update_payload(message).is_some_and(|update| {
            update.get("sessionUpdate").and_then(|value| value.as_str()) == Some("state_update")
                && update.get("state").and_then(|value| value.as_str()) == Some("idle")
                && update.get("stopReason").and_then(|value| value.as_str()) == Some("cancelled")
        })
    });
    assert!(
        cancelled_idle,
        "session/close should send cancelled idle before response: {pre_response:?}"
    );
    assert!(
        harness
            .session_manager
            .get_session(&session_id)
            .await
            .is_none(),
        "session should be removed after close"
    );
}

async fn send_delete(harness: &mut RuntimeHarness, session_id: &str) -> serde_json::Value {
    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/delete",
            Some(serde_json::json!({
                "sessionId": session_id,
            })),
        )
        .await;
    response.expect("session/delete should write a response")
}

#[tokio::test]
#[serial]
async fn delete_existing_session_archives_it() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    save_session(
        "delete-existing-session",
        &[user_message("delete me", 100)],
        project.to_str().unwrap(),
    )
    .unwrap();
    assert!(get_session_file("delete-existing-session").exists());

    let mut harness = RuntimeHarness::new_with_factory(project, Arc::new(TestEngineFactory));
    let response = send_delete(&mut harness, "delete-existing-session").await;

    assert!(
        response.get("error").is_none(),
        "session/delete should succeed: {response:?}"
    );
    assert!(!get_session_file("delete-existing-session").exists());
    assert!(get_archived_session_file("delete-existing-session").exists());
}

#[tokio::test]
#[serial]
async fn delete_nonexistent_session_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    let mut harness = RuntimeHarness::new_with_factory(project, Arc::new(TestEngineFactory));
    let response = send_delete(&mut harness, "missing-delete-session").await;

    assert!(
        response.get("error").is_none(),
        "deleting a missing session should be idempotent: {response:?}"
    );
}

#[tokio::test]
#[serial]
async fn delete_active_session_closes_first() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    let mut harness =
        RuntimeHarness::new_with_factory(project.clone(), Arc::new(TestEngineFactory));
    let (new_response, _pre_response) = harness
        .send_request_and_capture(
            "session/new",
            Some(serde_json::json!({
                "cwd": project,
                "additionalDirectories": [],
                "mcpServers": {},
            })),
        )
        .await;
    let session_id = new_response
        .as_ref()
        .and_then(|value| value.pointer("/result/sessionId"))
        .and_then(|value| value.as_str())
        .expect("session/new response should include sessionId")
        .to_string();
    let session = harness
        .session_manager
        .get_session(&session_id)
        .await
        .expect("session should exist");
    *session.active_turn.lock().await = Some(AcpTurnHandle {
        cancel_requested: false,
    });

    let (response, pre_response) = harness
        .send_request_and_capture(
            "session/delete",
            Some(serde_json::json!({
                "sessionId": session_id,
            })),
        )
        .await;
    let response = response.expect("session/delete should write a response");

    assert!(
        response.get("error").is_none(),
        "session/delete should close active session first: {response:?}"
    );
    assert!(
        harness
            .session_manager
            .get_session(&session_id)
            .await
            .is_none(),
        "active session should be removed after delete"
    );
    let cancelled_idle = pre_response.iter().any(|message| {
        update_payload(message).is_some_and(|update| {
            update.get("sessionUpdate").and_then(|value| value.as_str()) == Some("state_update")
                && update.get("state").and_then(|value| value.as_str()) == Some("idle")
                && update.get("stopReason").and_then(|value| value.as_str()) == Some("cancelled")
        })
    });
    assert!(
        cancelled_idle,
        "delete should emit cancelled idle while closing active session: {pre_response:?}"
    );
}

#[tokio::test]
#[serial]
async fn deleted_session_no_longer_lists() {
    let temp = tempfile::tempdir().unwrap();
    let data_home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", &data_home);

    save_session(
        "delete-list-removed",
        &[user_message("removed", 100)],
        project.to_str().unwrap(),
    )
    .unwrap();
    save_session(
        "delete-list-kept",
        &[user_message("kept", 100)],
        project.to_str().unwrap(),
    )
    .unwrap();

    let mut harness = RuntimeHarness::new_with_factory(project, Arc::new(TestEngineFactory));
    let delete_response = send_delete(&mut harness, "delete-list-removed").await;
    assert!(
        delete_response.get("error").is_none(),
        "session/delete should succeed: {delete_response:?}"
    );

    let list_response = send_list(&mut harness, None).await;
    assert_eq!(
        result_session_ids(&list_response),
        vec!["delete-list-kept".to_string()]
    );
}
