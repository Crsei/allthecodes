//! Axum route handlers for the web chat API.
//!
//! This module is organized by feature group. Each submodule exposes its
//! handler functions and request/response types; `mod.rs` re-exports all
//! public items so that `handlers::chat_handler` and similar paths used in
//! the router builder continue to resolve.

use std::sync::{OnceLock, RwLock};

use serde::Serialize;

use allthecodes_commands::Command;

pub mod activity_recorder;
pub mod admin;
pub mod agents;
pub mod appshots;
pub mod auth;
pub mod capabilities;
pub mod channels;
pub mod chat;
pub mod chrome_relay;
pub mod computer_use;
pub mod credentials;
pub mod git;
pub mod hooks;
pub mod mcp_servers;
pub mod models;
pub mod people;
pub mod plugins;
pub mod profiles;
pub mod prompts;
pub mod providers;
pub mod proxy;
pub mod sessions;
pub mod settings_phase1;

// Re-export all public items from each submodule so the router builder
// and external callers can still use `handlers::*` paths.
pub use activity_recorder::*;
pub use admin::*;
pub use agents::*;
pub use appshots::*;
pub use auth::*;
pub use capabilities::*;
pub use channels::*;
pub use chat::*;
pub use chrome_relay::*;
pub use computer_use::*;
pub use credentials::*;
pub use git::*;
pub use hooks::*;
pub use mcp_servers::*;
pub use models::*;
pub use people::*;
pub use plugins::*;
pub use profiles::*;
pub use prompts::*;
pub use providers::*;
pub use proxy::*;
pub use sessions::*;
pub use settings_phase1::*;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
}

// ---------------------------------------------------------------------------
// Command-provider registry
// ---------------------------------------------------------------------------

type CommandProvider = fn() -> Vec<Command>;

static COMMAND_PROVIDER: OnceLock<RwLock<Option<CommandProvider>>> = OnceLock::new();

/// Install the root-owned slash-command registry used by web command routes.
pub fn set_command_provider(provider: CommandProvider) {
    let slot = COMMAND_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

pub(crate) fn get_all_commands() -> Vec<Command> {
    COMMAND_PROVIDER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
        .map(|provider| provider())
        .unwrap_or_default()
}

pub(crate) fn setting_bool(state: &crate::state::WebState, path: &str) -> Option<bool> {
    let map = state.engine().app_state().settings.settings_map();
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = map.get(first)?;
    for part in parts {
        value = value.get(part)?;
    }
    value.as_bool()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WebState;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_engine::types::tool::PermissionMode;
    use allthecodes_ipc_protocol::subsystem_types::{
        AgentDefinitionEntry, AgentDefinitionSource, ConfigScope, McpServerConfigEntry,
    };
    use axum::body::to_bytes;
    use axum::extract::{Path as AxumPath, Query, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn make_web_state() -> WebState {
        make_web_state_with_cwd(Path::new("."))
    }

    fn make_web_state_with_cwd(cwd: &Path) -> WebState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: cwd.to_string_lossy().to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        WebState::new(engine, Arc::new(AtomicBool::new(false)))
    }

    fn make_agent_entry(name: &str, source: AgentDefinitionSource) -> AgentDefinitionEntry {
        AgentDefinitionEntry {
            name: name.to_string(),
            description: format!("Agent {name}"),
            system_prompt: "You are a test agent.".to_string(),
            tools: vec!["Read".to_string()],
            disallowed_tools: vec![],
            model: None,
            color: None,
            permission_mode: None,
            memory: None,
            max_turns: None,
            effort: None,
            background: false,
            isolation: None,
            skills: vec![],
            hooks: Value::Null,
            mcp_servers: vec![],
            initial_prompt: None,
            filename: None,
            source,
            file_path: None,
        }
    }

    fn make_mcp_entry(name: &str, scope: ConfigScope) -> McpServerConfigEntry {
        McpServerConfigEntry {
            name: name.to_string(),
            transport: "stdio".to_string(),
            command: Some("echo".to_string()),
            args: Some(vec!["ok".to_string()]),
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
            scope,
        }
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("json body")
    }

    fn temp_home() -> (TempDir, EnvGuard) {
        let temp = tempfile::tempdir().expect("tempdir");
        let guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        (temp, guard)
    }

    fn read_user_settings(home: &TempDir) -> allthecodes_config::settings::RawSettings {
        let path = home.path().join("settings.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("settings file"))
            .expect("settings json")
    }

    #[tokio::test]
    #[serial]
    async fn set_model_rejects_values_outside_available_models() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.available_models = vec!["gpt-4o".to_string()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("SOTA"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(false));
        assert!(body["message"]
            .as_str()
            .expect("message")
            .contains("not in availableModels"));
        assert_ne!(
            state.engine().app_state().main_loop_model,
            "claude-opus-4-20250514"
        );
    }

    #[tokio::test]
    #[serial]
    async fn set_model_accepts_alias_when_full_id_is_allowlisted() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let expected_model = allthecodes_commands::model::resolve_model_alias("SOTA");
        state.engine().update_app_state(|s| {
            s.settings.sota_model = Some(expected_model.clone());
            s.settings.available_models = vec![expected_model.clone()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("SOTA"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(state.engine().app_state().main_loop_model, expected_model);
        assert_eq!(
            state.engine().app_state().settings.model.as_deref(),
            Some(expected_model.as_str())
        );
    }

    #[tokio::test]
    #[serial]
    async fn set_permission_mode_auto_respects_disabled_policy() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.tool_permission_context.is_auto_mode_available = Some(false);
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_permission_mode".to_string(),
                value: json!("auto"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(false));
        assert!(body["message"]
            .as_str()
            .expect("message")
            .contains("permissions.enableAutoMode=false"));
        assert_eq!(
            state.engine().app_state().tool_permission_context.mode,
            PermissionMode::Default
        );
    }

    #[tokio::test]
    #[serial]
    async fn compatibility_settings_actions_persist_user_settings() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.available_models = vec!["gpt-4o".to_string()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("gpt-4o"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.model.as_deref(), Some("gpt-4o"));
        assert_eq!(state.engine().app_state().main_loop_model, "gpt-4o");

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_permission_mode".to_string(),
                value: json!("plan"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.permission_mode.as_deref(), Some("plan"));
        assert_eq!(
            state.engine().app_state().tool_permission_context.mode,
            PermissionMode::Plan
        );

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_thinking".to_string(),
                value: json!(true),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(
            raw.thinking
                .as_ref()
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str),
            Some("enabled")
        );
        assert_eq!(state.engine().app_state().thinking_enabled, Some(true));
    }

    #[tokio::test]
    #[serial]
    async fn set_ext_persists_typed_and_unknown_paths() {
        let (home, _guard) = temp_home();
        let state = make_web_state();

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_ext".to_string(),
                value: json!({
                    "path": "network.proxy_enabled",
                    "value": true
                }),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            state.engine().app_state().settings.proxy_enabled,
            Some(true)
        );
        let raw = read_user_settings(&home);
        assert_eq!(raw.proxy_enabled, Some(true));

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_ext".to_string(),
                value: json!({
                    "path": "customPanel.feature_flag",
                    "value": "enabled"
                }),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(
            raw.extra
                .get("customPanel")
                .and_then(|value| value.get("feature_flag")),
            Some(&json!("enabled"))
        );
        assert_eq!(
            state
                .engine()
                .app_state()
                .settings
                .settings_map()
                .get("customPanel"),
            Some(&json!({ "feature_flag": "enabled" }))
        );
    }

    #[tokio::test]
    #[serial]
    async fn phase1_helper_endpoints_return_stable_json() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.auto_memory_enabled = Some(true);
            s.settings.memory_max_retrieved = Some(8);
        });

        let response = memory_config_get_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["config"]["auto_memory_enabled"], json!(true));
        assert_eq!(body["config"]["memory_max_retrieved"], json!(8));
        assert!(body["stats"]["total"].as_u64().is_some());

        let response = speech_models_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["models"].as_array().expect("models").len() >= 4);

        let response = speech_model_download_handler(Json(SpeechModelDownloadRequest {
            model_id: "tiny".to_string(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("speech_download_not_implemented"));

        let response = search_cookies_import_handler(Json(SearchCookiesImportRequest {
            cookies: json!([{ "name": "session", "value": "redacted" }]),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("search-cookies.json").exists());

        let response = search_cookies_export_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["cookies"][0]["name"], json!("session"));

        let response = search_cookies_clear_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("search-cookies.json").exists());

        let response = data_export_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert!(body["bytes"].as_u64().expect("bytes") > 0);

        let response = token_savings_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["total_saved_tokens"], json!(0));
        assert_eq!(body["cache_hit_rate"], json!(0.0));
    }

    #[tokio::test]
    async fn state_response_includes_settings_map_version_and_capabilities() {
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.language = Some("zh-CN".to_string());
            s.settings.proxy_enabled = Some(true);
        });

        let response = state_handler(State(state)).await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["settings_map"]["language"], json!("zh-CN"));
        assert_eq!(body["settings_map"]["proxy_enabled"], json!(true));
        assert!(body["version"].as_str().is_some());
        assert_eq!(body["capabilities"]["settings"], json!(true));
        assert_eq!(body["capabilities"]["memory"], json!(true));
        assert_eq!(body["capabilities"]["agents"], json!(true));
        assert_eq!(body["capabilities"]["people"], json!(true));
        assert_eq!(body["capabilities"]["hooks"], json!(true));
        assert_eq!(body["capabilities"]["prompts"], json!(true));
        assert_eq!(body["capabilities"]["mcp_servers"], json!(true));
        assert_eq!(body["capabilities"]["plugins"], json!(true));
        assert_eq!(body["capabilities"]["channels"], json!(true));
        assert_eq!(body["capabilities"]["gateways"], json!(true));
        assert_eq!(body["capabilities"]["computer_use"], json!(true));
        assert_eq!(body["capabilities"]["appshots"], json!(true));
        assert_eq!(body["capabilities"]["activity_recorder"], json!(true));
        assert_eq!(body["capabilities"]["chrome_relay"], json!(true));
    }

    #[test]
    fn build_router_accepts_phase3_routes() {
        let _router = crate::build_router(make_web_state());
    }

    #[tokio::test]
    #[serial]
    async fn mcp_servers_crud_uses_editable_settings_scopes() {
        let (home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let mut user = allthecodes_config::settings::RawSettings::default();
        user.extra.insert(
            "mcpServers".to_string(),
            json!({
                "user-srv": {
                    "type": "stdio",
                    "command": "user-cmd"
                }
            }),
        );
        allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

        let mut project_raw = allthecodes_config::settings::RawSettings::default();
        project_raw.extra.insert(
            "mcpServers".to_string(),
            json!({
                "project-srv": {
                    "type": "stdio",
                    "command": "project-cmd"
                }
            }),
        );
        let project_settings = project.path().join(".allthecodes").join("settings.json");
        allthecodes_config::settings::write_settings_file(&project_settings, &project_raw)
            .expect("seed project settings");

        let response = mcp_servers_list_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let servers = body["servers"].as_array().expect("servers");
        assert!(servers.iter().any(|server| {
            server["name"] == json!("user-srv") && server["scope"]["kind"] == json!("user")
        }));
        assert!(servers.iter().any(|server| {
            server["name"] == json!("project-srv") && server["scope"]["kind"] == json!("project")
        }));

        let response = mcp_servers_create_handler(
            State(state.clone()),
            Json(McpServerUpsertRequest::Entry(make_mcp_entry(
                "created",
                ConfigScope::User,
            ))),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.extra["mcpServers"]["created"]["command"], json!("echo"));

        let mut updated = make_mcp_entry("created", ConfigScope::User);
        updated.command = Some("printf".to_string());
        let response = mcp_servers_update_handler(
            State(state.clone()),
            AxumPath("created".to_string()),
            Json(McpServerUpsertRequest::Wrapped { entry: updated }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(
            raw.extra["mcpServers"]["created"]["command"],
            json!("printf")
        );

        let response = mcp_servers_delete_handler(
            State(state.clone()),
            AxumPath("created".to_string()),
            Query(McpServerDeleteQuery {
                scope: "user".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert!(raw.extra["mcpServers"].get("created").is_none());

        let response = mcp_servers_create_handler(
            State(state.clone()),
            Json(McpServerUpsertRequest::Entry(make_mcp_entry(
                "plugin-owned",
                ConfigScope::Plugin {
                    id: "plug".to_string(),
                },
            ))),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("validation_error"));

        let response = mcp_servers_delete_handler(
            State(state),
            AxumPath("project-srv".to_string()),
            Query(McpServerDeleteQuery {
                scope: "plugin:plug".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial]
    async fn plugins_local_install_list_marketplace_and_uninstall_round_trip() {
        let (_home, _guard) = temp_home();
        allthecodes_plugins::clear_plugins();
        let state = make_web_state();
        let plugin_source = tempfile::tempdir().expect("plugin source");
        std::fs::write(
            plugin_source.path().join("plugin.json"),
            r#"{
                "name": "local-plugin",
                "display_name": "Local Plugin",
                "version": "1.0.0",
                "description": "Local test plugin"
            }"#,
        )
        .expect("plugin manifest");

        let response = plugins_install_handler(
            State(state),
            Json(PluginInstallRequest {
                source: plugin_source.path().to_string_lossy().to_string(),
                scope: Some("user".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["plugin"]["id"], json!("local-plugin@local"));
        assert_eq!(body["fresh_install"], json!(true));

        let response = plugins_list_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["plugins"]
            .as_array()
            .expect("plugins")
            .iter()
            .any(|plugin| plugin["id"] == json!("local-plugin@local")));
        assert!(body["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty());

        let response = plugins_marketplace_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["plugins"].is_array());

        let response = plugins_uninstall_handler(
            AxumPath("local-plugin@local".to_string()),
            Json(PluginUninstallRequest { purge: false }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let response = plugins_list_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["plugins"].as_array().expect("plugins").is_empty());
        allthecodes_plugins::clear_plugins();
    }

    #[tokio::test]
    #[serial]
    async fn channels_report_stopped_daemon_without_starting_it() {
        let (_home, _guard) = temp_home();

        let response = channels_list_handler().await.into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("daemon_stopped"));
    }

    #[tokio::test]
    #[serial]
    async fn system_action_endpoints_return_explicit_501_codes() {
        let (_home, _guard) = temp_home();

        let response = computer_use_test_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("computer_use_test_not_implemented"));

        let response =
            computer_use_permission_request_handler(AxumPath("accessibility".to_string()))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(
            body["code"],
            json!("computer_use_permission_request_not_implemented")
        );

        let response = appshots_capture_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("appshots_capture_not_implemented"));

        let response = chrome_relay_launch_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("chrome_relay_launch_not_implemented"));

        let response = chrome_relay_token_regenerate_handler()
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(
            body["code"],
            json!("chrome_relay_token_regenerate_not_implemented")
        );
    }

    #[tokio::test]
    #[serial]
    async fn activity_recorder_empty_store_status_sessions_and_clear_round_trip() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();

        let response = activity_recorder_status_handler(State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["enabled"], json!(false));
        assert_eq!(body["available"], json!(false));

        let response = activity_recorder_sessions_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["sessions"], json!([]));

        let response = activity_recorder_clear_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["cleared"], json!(0));
        assert_eq!(body["sessions"], json!([]));
    }

    #[tokio::test]
    #[serial]
    async fn agents_rest_handlers_list_persist_and_restore() {
        let (home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = agents_list_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .any(|agent| agent["name"] == json!("general-purpose")));
        assert!(body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["name"] == json!("Read")));

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("web-user", AgentDefinitionSource::User),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("agents/web-user.md").exists());

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("web-project", AgentDefinitionSource::Project),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(project
            .path()
            .join(".allthecodes/agents/web-project.md")
            .exists());

        let response = agents_update_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("general-purpose", AgentDefinitionSource::Builtin),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("validation_error"));

        let response = agents_delete_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
            Query(AgentDeleteQuery {
                source: "builtin".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("general-purpose", AgentDefinitionSource::User),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("agents/general-purpose.md").exists());

        let response = agents_restore_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("agents/general-purpose.md").exists());
        let body = response_json(response).await;
        assert!(body["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .any(|agent| {
                agent["name"] == json!("general-purpose")
                    && agent["source"]["kind"] == json!("builtin")
            }));
    }

    #[tokio::test]
    #[serial]
    async fn people_crud_round_trips_json_and_validates_ids() {
        let (home, _guard) = temp_home();

        let response = people_create_handler(Json(PersonCreateRequest {
            id: None,
            name: "Ada Lovelace".to_string(),
            telegram_id: Some("ada-tg".to_string()),
            discord_id: None,
            discord_username: Some("ada".to_string()),
            feishu_id: None,
            username: Some("ada".to_string()),
            profile_content: "First programmer".to_string(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["id"], json!("ada-lovelace"));
        assert!(home.path().join("people/ada-lovelace.json").exists());

        let response = people_create_handler(Json(PersonCreateRequest {
            id: Some("ada-lovelace".to_string()),
            name: "Ada Duplicate".to_string(),
            telegram_id: None,
            discord_id: None,
            discord_username: None,
            feishu_id: None,
            username: None,
            profile_content: String::new(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = people_update_handler(
            AxumPath("ada-lovelace".to_string()),
            Json(PersonUpdateRequest {
                name: Some("Ada Byron".to_string()),
                telegram_id: Some(None),
                discord_id: None,
                discord_username: None,
                feishu_id: None,
                username: None,
                profile_content: Some("Updated profile".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["name"], json!("Ada Byron"));
        assert_eq!(body["telegram_id"], Value::Null);

        let response = people_create_handler(Json(PersonCreateRequest {
            id: Some("bad/id".to_string()),
            name: "Bad".to_string(),
            telegram_id: None,
            discord_id: None,
            discord_username: None,
            feishu_id: None,
            username: None,
            profile_content: String::new(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = people_delete_handler(AxumPath("ada-lovelace".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("people/ada-lovelace.json").exists());
    }

    #[tokio::test]
    #[serial]
    async fn hooks_crud_updates_user_settings_only_and_test_is_explicit_501() {
        let (home, _guard) = temp_home();
        let initial = allthecodes_config::settings::RawSettings {
            language: Some("en".to_string()),
            ..Default::default()
        };
        allthecodes_config::settings::write_user_settings(&initial).expect("seed settings");

        let config = json!({
            "matcher": "Read",
            "hooks": [{ "type": "command", "command": "echo ok" }]
        });
        let response = hooks_create_handler(Json(HookEventRequest {
            event: "PreToolUse".to_string(),
            configs: vec![config.clone()],
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.language.as_deref(), Some("en"));
        assert_eq!(
            raw.hooks.as_ref().and_then(|hooks| hooks.get("PreToolUse")),
            Some(&json!([config.clone()]))
        );

        let response = hooks_create_handler(Json(HookEventRequest {
            event: "PreToolUse".to_string(),
            configs: vec![config.clone()],
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = hooks_update_handler(
            AxumPath("PreToolUse".to_string()),
            Json(HookEventUpdateRequest {
                configs: vec![json!({ "matcher": "*", "hooks": [] })],
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let response = hooks_create_handler(Json(HookEventRequest {
            event: "PostToolUse".to_string(),
            configs: vec![json!({ "matcher": "Read" })],
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = hooks_test_handler(Json(HookEventRequest {
            event: "PreToolUse".to_string(),
            configs: vec![json!({ "matcher": "*", "hooks": [] })],
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("hook_test_not_implemented"));

        let response = hooks_delete_handler(AxumPath("PreToolUse".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert!(!raw.hooks.unwrap_or_default().contains_key("PreToolUse"));
    }

    #[tokio::test]
    #[serial]
    async fn prompts_crud_round_trips_store_and_rejects_slash_names() {
        let (home, _guard) = temp_home();

        let response = prompts_create_handler(Json(PromptCreateRequest {
            id: None,
            name: "Summarize Thread".to_string(),
            content: "Summarize this thread.".to_string(),
            description: "summary prompt".to_string(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["id"], json!("summarize-thread"));
        assert!(home.path().join("quick-prompts.json").exists());

        let response = prompts_create_handler(Json(PromptCreateRequest {
            id: Some("summarize-thread".to_string()),
            name: "Duplicate".to_string(),
            content: "duplicate".to_string(),
            description: String::new(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = prompts_create_handler(Json(PromptCreateRequest {
            id: None,
            name: "/bad".to_string(),
            content: "bad".to_string(),
            description: String::new(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = prompts_update_handler(
            AxumPath("summarize-thread".to_string()),
            Json(PromptUpdateRequest {
                name: Some("Summarize".to_string()),
                content: Some("Updated".to_string()),
                description: Some("updated description".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["content"], json!("Updated"));

        let response = prompts_delete_handler(AxumPath("summarize-thread".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["prompts"], json!([]));
    }
}
