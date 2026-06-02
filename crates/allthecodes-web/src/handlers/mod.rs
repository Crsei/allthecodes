//! Axum route handlers for the web chat API.
//!
//! This module is organized by feature group. Each submodule exposes its
//! handler functions and request/response types; `mod.rs` re-exports all
//! public items so that `handlers::chat_handler` and similar paths used in
//! the router builder continue to resolve.

use std::sync::{OnceLock, RwLock};

use serde::Serialize;

use allthecodes_commands::Command;

pub mod admin;
pub mod auth;
pub mod capabilities;
pub mod chat;
pub mod credentials;
pub mod models;
pub mod profiles;
pub mod providers;
pub mod sessions;
pub mod settings_phase1;

// Re-export all public items from each submodule so the router builder
// and external callers can still use `handlers::*` paths.
pub use admin::*;
pub use auth::*;
pub use capabilities::*;
pub use chat::*;
pub use credentials::*;
pub use models::*;
pub use profiles::*;
pub use providers::*;
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
    use axum::body::to_bytes;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn make_web_state() -> WebState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: ".".to_string(),
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

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("json body")
    }

    #[tokio::test]
    async fn set_model_rejects_values_outside_available_models() {
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
    async fn set_model_accepts_alias_when_full_id_is_allowlisted() {
        let state = make_web_state();
        let expected_model = allthecodes_commands::model::resolve_model_alias("SOTA");
        state.engine().update_app_state(|s| {
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
    async fn set_permission_mode_auto_respects_disabled_policy() {
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
    }
}
