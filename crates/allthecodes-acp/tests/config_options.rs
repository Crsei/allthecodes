#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Tests: ACP session configuration options.

mod support;

use std::sync::Arc;

use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::AcpEngineParams;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_types::permissions::PermissionMode;
use support::RuntimeHarness;

#[derive(Clone)]
struct ConfigEngineFactory {
    model: String,
    available_models: Vec<String>,
    effort: Option<String>,
    mode: PermissionMode,
}

impl Default for ConfigEngineFactory {
    fn default() -> Self {
        Self {
            model: "model-a".to_string(),
            available_models: vec!["model-a".to_string(), "model-b".to_string()],
            effort: Some("medium".to_string()),
            mode: PermissionMode::Default,
        }
    }
}

impl AcpEngineFactory for ConfigEngineFactory {
    fn create_engine(&self, params: AcpEngineParams) -> anyhow::Result<Arc<QueryEngine>> {
        let config = QueryEngineConfig {
            cwd: params.cwd.to_string_lossy().to_string(),
            tools: vec![],
            max_turns: Some(1),
            initial_messages: params.initial_messages,
            verbose: false,
            resolved_model: Some(self.model.clone()),
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
        let engine = Arc::new(QueryEngine::new(config));
        engine.update_app_state(|state| {
            state.main_loop_model = self.model.clone();
            state.settings.available_models = self.available_models.clone();
            state.effort_value = self.effort.clone();
            state.tool_permission_context.mode = self.mode.clone();
        });
        Ok(engine)
    }
}

fn option_ids(options: &[serde_json::Value]) -> Vec<&str> {
    options
        .iter()
        .filter_map(|option| option.get("id"))
        .filter_map(|id| id.as_str())
        .collect()
}

fn find_option<'a>(options: &'a [serde_json::Value], id: &str) -> &'a serde_json::Value {
    options
        .iter()
        .find(|option| option.get("id").and_then(|value| value.as_str()) == Some(id))
        .unwrap_or_else(|| panic!("missing config option {id}: {options:?}"))
}

fn option_values(option: &serde_json::Value) -> Vec<&str> {
    option
        .get("options")
        .and_then(|options| options.as_array())
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("value"))
        .filter_map(|value| value.as_str())
        .collect()
}

fn result_config_options(response: &serde_json::Value) -> Vec<serde_json::Value> {
    response
        .pointer("/result/configOptions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
}

async fn new_session(harness: &mut RuntimeHarness, cwd: &std::path::Path) -> String {
    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/new",
            Some(serde_json::json!({
                "cwd": cwd,
                "additionalDirectories": [],
                "mcpServers": {},
            })),
        )
        .await;
    let response = response.expect("session/new should write a response");
    assert!(
        response.get("error").is_none(),
        "session/new should succeed: {response:?}"
    );
    response
        .pointer("/result/sessionId")
        .and_then(|value| value.as_str())
        .expect("session/new response should include sessionId")
        .to_string()
}

#[tokio::test]
async fn new_session_returns_model_mode_thought_options() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));

    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/new",
            Some(serde_json::json!({
                "cwd": cwd,
                "additionalDirectories": [],
                "mcpServers": {},
            })),
        )
        .await;
    let response = response.expect("session/new should write a response");
    let options = result_config_options(&response);

    assert_eq!(option_ids(&options), vec!["model", "thought_level", "mode"]);
    assert_eq!(find_option(&options, "model")["currentValue"], "model-a");
    assert_eq!(
        find_option(&options, "thought_level")["currentValue"],
        "medium"
    );
    assert_eq!(find_option(&options, "mode")["currentValue"], "default");
}

#[tokio::test]
async fn model_options_come_from_available_models() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));

    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/new",
            Some(serde_json::json!({
                "cwd": cwd,
                "additionalDirectories": [],
                "mcpServers": {},
            })),
        )
        .await;
    let response = response.expect("session/new should write a response");
    let options = result_config_options(&response);

    assert_eq!(
        option_values(find_option(&options, "model")),
        vec!["model-a", "model-b"]
    );
    assert_eq!(
        option_values(find_option(&options, "mode")),
        vec![
            "default",
            "auto",
            "bypass",
            "plan",
            "acceptEdits",
            "dontAsk"
        ]
    );
}

#[tokio::test]
async fn set_model_updates_app_state() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));
    let session_id = new_session(&mut harness, &cwd).await;

    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/set_config_option",
            Some(serde_json::json!({
                "sessionId": session_id,
                "configId": "model",
                "value": "model-b",
            })),
        )
        .await;
    let response = response.expect("session/set_config_option should write a response");
    assert!(
        response.get("error").is_none(),
        "set model should succeed: {response:?}"
    );

    let session = harness
        .session_manager
        .get_session(&session_id)
        .await
        .expect("session should exist");
    assert_eq!(session.engine.app_state().main_loop_model, "model-b");
}

#[tokio::test]
async fn set_unknown_config_rejects() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));
    let session_id = new_session(&mut harness, &cwd).await;

    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/set_config_option",
            Some(serde_json::json!({
                "sessionId": session_id,
                "configId": "unknown",
                "value": "model-b",
            })),
        )
        .await;
    let response = response.expect("session/set_config_option should write a response");
    assert_eq!(
        response
            .pointer("/error/code")
            .and_then(|value| value.as_i64()),
        Some(-32602)
    );
}

#[tokio::test]
async fn set_invalid_value_rejects() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));
    let session_id = new_session(&mut harness, &cwd).await;

    let (response, _pre_response) = harness
        .send_request_and_capture(
            "session/set_config_option",
            Some(serde_json::json!({
                "sessionId": session_id,
                "configId": "model",
                "value": "missing-model",
            })),
        )
        .await;
    let response = response.expect("session/set_config_option should write a response");
    assert_eq!(
        response
            .pointer("/error/code")
            .and_then(|value| value.as_i64()),
        Some(-32602)
    );
}

#[tokio::test]
async fn set_option_sends_config_option_update() {
    let cwd = std::env::current_dir().unwrap();
    let mut harness =
        RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(ConfigEngineFactory::default()));
    let session_id = new_session(&mut harness, &cwd).await;

    let (response, pre_response) = harness
        .send_request_and_capture(
            "session/set_config_option",
            Some(serde_json::json!({
                "sessionId": session_id,
                "configId": "thought_level",
                "value": "low",
            })),
        )
        .await;
    let response = response.expect("session/set_config_option should write a response");
    assert!(
        response.get("error").is_none(),
        "set thought_level should succeed: {response:?}"
    );

    let update = pre_response
        .iter()
        .find_map(|message| message.pointer("/params/update"))
        .filter(|update| {
            update.get("sessionUpdate").and_then(|value| value.as_str())
                == Some("config_option_update")
        })
        .expect("set_config_option should send config_option_update before response");
    let options = update
        .get("configOptions")
        .and_then(|value| value.as_array())
        .expect("config_option_update should include configOptions");
    assert_eq!(find_option(options, "thought_level")["currentValue"], "low");
}

#[tokio::test]
async fn prompt_uses_session_submit_overrides() {
    let cwd = std::env::current_dir().unwrap();
    let factory = ConfigEngineFactory {
        model: "model-b".to_string(),
        effort: Some("low".to_string()),
        ..ConfigEngineFactory::default()
    };
    let mut harness = RuntimeHarness::new_with_factory(cwd.clone(), Arc::new(factory));
    let session_id = new_session(&mut harness, &cwd).await;

    let session = harness
        .session_manager
        .get_session(&session_id)
        .await
        .expect("session should exist");
    let overrides = allthecodes_acp::config_options::submit_overrides_for_session(&session);

    assert_eq!(overrides.model.as_deref(), Some("model-b"));
    assert_eq!(overrides.effort.as_deref(), Some("low"));
}
