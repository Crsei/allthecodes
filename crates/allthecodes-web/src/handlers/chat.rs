//! Chat, abort, and state handlers — core chat API.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;
use tracing::info;

use allthecodes_daemon::web::sdk_stream_to_sse;
use allthecodes_engine::types::config::{QuerySource, SubmitContextMode, SubmitMessageOverrides};
use allthecodes_types::sdk::SdkMessage;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Serialize)]
pub struct UsageResponse {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub api_call_count: u64,
}

#[derive(Serialize, Clone)]
pub struct CommandInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub thinking_enabled: Option<bool>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub skill_ids: Option<Vec<String>>,
    #[serde(default)]
    pub context_mode: Option<String>,
}

#[derive(Deserialize)]
pub struct AbortRequest {
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Serialize)]
pub struct StateResponse {
    pub model: String,
    pub session_id: String,
    pub workspace_key: String,
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
    pub tools: Vec<String>,
    pub permission_mode: String,
    pub thinking_enabled: Option<bool>,
    pub fast_mode: bool,
    pub effort: Option<String>,
    // Phase 3 additions
    pub usage: UsageResponse,
    pub commands: Vec<CommandInfo>,
    pub settings_map: HashMap<String, Value>,
    pub effective_system_prompt: String,
    pub version: String,
    pub capabilities: HashMap<String, bool>,
}

#[derive(Serialize)]
pub struct SystemPromptResponse {
    pub prompt: String,
}

#[derive(Serialize)]
pub struct CodingAgentStatus {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub command: Option<String>,
    pub error: Option<String>,
}

/// POST /api/chat -- Start a streaming chat response via SSE.
pub async fn chat_handler(
    State(state): State<WebState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let foreground_engine = state.engine();
    let active_session_id = req
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| foreground_engine.current_session_id().to_string());

    if state.is_session_streaming(&active_session_id) {
        return (
            StatusCode::CONFLICT,
            Json(ProtocolApiError::EngineBusy.into_body()),
        )
            .into_response();
    }

    let engine = match state.engine_for_session(&active_session_id) {
        Some(engine) => engine,
        None => {
            match crate::handlers::sessions::build_engine_for_session(&state, &active_session_id) {
                Ok(engine) => {
                    state.cache_session_engine(engine.clone());
                    engine
                }
                Err(error) => {
                    return crate::api_errors::protocol_error_response(error).into_response();
                }
            }
        }
    };

    let mode_id = req
        .mode
        .as_deref()
        .map(|mode| crate::handlers::normalize_mode_or_normal(Some(mode)))
        .unwrap_or_else(|| {
            crate::handlers::chat_mode_preference_for_cwd_session(engine.cwd(), &active_session_id)
                .effective_chat_mode
        });
    let activation = match crate::handlers::resolve_mode_activation(&state, Some(&mode_id)) {
        Ok(activation) => activation,
        Err(error) => {
            return crate::api_errors::protocol_error_response(error).into_response();
        }
    };
    if req.mode.is_some() {
        if let Err(error) = allthecodes_session::storage::set_session_chat_mode_override(
            &active_session_id,
            Some(&mode_id),
            engine.cwd(),
        ) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    ProtocolApiError::BadRequest {
                        code: "session_mode_update_failed",
                        message: format!("Failed to persist session mode: {error}"),
                    }
                    .into_body(),
                ),
            )
                .into_response();
        }
    }

    let requested_session = active_session_id.as_str();
    let requested_model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let context_mode = req
        .context_mode
        .as_deref()
        .and_then(SubmitContextMode::parse);
    let overrides = SubmitMessageOverrides {
        model: requested_model.clone(),
        thinking_enabled: req.thinking_enabled,
        effort: req.effort.clone(),
        allowed_tools: req.allowed_tools.clone(),
        skill_ids: req.skill_ids.clone(),
        context_mode,
    };
    info!(
        message = %req.message,
        session_id = %requested_session,
        model = requested_model.as_deref().unwrap_or(""),
        mode = %mode_id,
        "POST /api/chat"
    );

    state.set_session_streaming(&active_session_id, true);

    // Get the stream from the engine
    let prompt = match activation {
        Some(activation) => {
            info!(mode = %activation.mode_id, "chat mode activation applied");
            format!("{}{}", activation.prompt_prefix, req.message)
        }
        None => req.message.clone(),
    };
    let stream = engine.submit_message_with_overrides(&prompt, QuerySource::Sdk, overrides);

    // Wrap in a stream that clears is_streaming when done
    let stream_state = state.clone();
    let stream_session_id = active_session_id.clone();
    let wrapped_stream = Box::pin(futures::stream::unfold(
        (stream, stream_state, stream_session_id, false),
        |(mut stream, state, session_id, done)| async move {
            if done {
                return None;
            }
            use futures::StreamExt;
            match stream.next().await {
                Some(msg) => {
                    let is_result = matches!(&msg, SdkMessage::Result(_));
                    if is_result {
                        state.set_session_streaming(&session_id, false);
                    }
                    Some((msg, (stream, state, session_id, is_result)))
                }
                None => {
                    state.set_session_streaming(&session_id, false);
                    None
                }
            }
        },
    ));

    sdk_stream_to_sse(wrapped_stream).into_response()
}

/// POST /api/abort -- Abort the current generation.
pub async fn abort_handler(
    State(state): State<WebState>,
    Json(req): Json<AbortRequest>,
) -> impl IntoResponse {
    let requested_session = req.session_id.as_deref().unwrap_or("");
    info!(session_id = %requested_session, "POST /api/abort");
    if requested_session.is_empty() {
        state.engine().abort();
        state
            .is_streaming
            .store(false, std::sync::atomic::Ordering::SeqCst);
    } else {
        state
            .engine_for_session(requested_session)
            .unwrap_or_else(|| state.engine())
            .abort();
        state.set_session_streaming(requested_session, false);
    }
    StatusCode::OK
}

/// GET /api/state -- Return current application state (enhanced for Phase 3).
pub async fn state_handler(State(state): State<WebState>) -> impl IntoResponse {
    let app_state = state.engine().app_state();
    let permission_mode = app_state.tool_permission_context.mode.as_str();

    // Get tool names from engine
    let tool_names: Vec<String> = state.engine().tool_names();

    // Get usage tracking
    let usage = state.engine().usage();

    // Get command list
    let commands: Vec<CommandInfo> = crate::handlers::get_all_commands()
        .iter()
        .map(|c| CommandInfo {
            name: c.name.clone(),
            aliases: c.aliases.clone(),
            description: c.description.clone(),
        })
        .collect();

    let settings_map = app_state.settings.settings_map();
    let effective_system_prompt = effective_system_prompt_from_map(&settings_map);
    let session_id = state.engine().current_session_id().to_string();
    let chat_mode_preference =
        crate::handlers::chat_mode_preference_for_cwd_session(state.engine().cwd(), &session_id);

    Json(StateResponse {
        model: app_state.main_loop_model.clone(),
        session_id,
        workspace_key: chat_mode_preference.workspace_key,
        default_chat_mode: chat_mode_preference.default_chat_mode,
        chat_mode_override: chat_mode_preference.chat_mode_override,
        effective_chat_mode: chat_mode_preference.effective_chat_mode,
        tools: tool_names,
        permission_mode: permission_mode.to_string(),
        thinking_enabled: app_state.thinking_enabled,
        fast_mode: app_state.fast_mode,
        effort: app_state.effort_value.clone(),
        usage: UsageResponse {
            total_input_tokens: usage.total_input_tokens,
            total_output_tokens: usage.total_output_tokens,
            total_cache_read_tokens: usage.total_cache_read_tokens,
            total_cache_creation_tokens: usage.total_cache_creation_tokens,
            total_cost_usd: usage.total_cost_usd,
            api_call_count: usage.api_call_count,
        },
        commands,
        settings_map,
        effective_system_prompt,
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: crate::handlers::capabilities_map(),
    })
}

/// GET /api/system-prompt -- Return the prompt text currently exposed to chat.
pub async fn system_prompt_handler(State(state): State<WebState>) -> impl IntoResponse {
    let map = state.engine().app_state().settings.settings_map();
    Json(SystemPromptResponse {
        prompt: effective_system_prompt_from_map(&map),
    })
}

/// GET /api/coding-agents/status -- Probe local agent commands for General settings.
pub async fn coding_agent_status_handler() -> impl IntoResponse {
    Json(vec![
        CodingAgentStatus {
            id: "allthecodes".to_string(),
            label: "allthecodes".to_string(),
            available: true,
            command: std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string()),
            error: None,
        },
        probe_agent("claude_code", "Claude Code CLI", &["claude", "claude-code"]),
        probe_agent("codex", "Codex CLI", &["codex"]),
    ])
}

fn effective_system_prompt_from_map(map: &HashMap<String, Value>) -> String {
    map.get("system_prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or("You are a helpful AI assistant.")
        .to_string()
}

fn probe_agent(id: &str, label: &str, commands: &[&str]) -> CodingAgentStatus {
    let mut last_error = None;
    for command in commands {
        match Command::new(command).arg("--version").output() {
            Ok(output) if output.status.success() => {
                return CodingAgentStatus {
                    id: id.to_string(),
                    label: label.to_string(),
                    available: true,
                    command: Some((*command).to_string()),
                    error: None,
                };
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                last_error = Some(if stderr.is_empty() {
                    format!("{command} exited with {}", output.status)
                } else {
                    stderr
                });
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
    }

    CodingAgentStatus {
        id: id.to_string(),
        label: label.to_string(),
        available: false,
        command: None,
        error: last_error.or_else(|| Some("command not found".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use serde_json::json;

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
        assert_eq!(body["capabilities"]["skills"], json!(true));
    }

    #[tokio::test]
    async fn build_router_accepts_phase3_routes() {
        let _router = crate::build_router(make_web_state());
    }

    #[tokio::test]
    async fn web_state_tracks_streaming_by_session() {
        let state = make_web_state();

        state.set_session_streaming("session-a", true);
        assert!(state.is_session_streaming("session-a"));
        assert!(!state.is_session_streaming("session-b"));
        assert!(state.is_streaming.load(std::sync::atomic::Ordering::SeqCst));

        state.set_session_streaming("session-b", true);
        state.set_session_streaming("session-a", false);
        assert!(!state.is_session_streaming("session-a"));
        assert!(state.is_session_streaming("session-b"));
        assert!(state.is_streaming.load(std::sync::atomic::Ordering::SeqCst));

        state.set_session_streaming("session-b", false);
        assert!(!state.is_streaming.load(std::sync::atomic::Ordering::SeqCst));
    }
}
