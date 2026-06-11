//! Chat, abort, and state handlers — core chat API.

use std::collections::HashMap;
use std::convert::Infallible;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

use allthecodes_engine::types::config::{QuerySource, SubmitContextMode, SubmitMessageOverrides};
use allthecodes_types::callbacks::{PermissionRequestPayload, PermissionResponsePayload};
use allthecodes_types::sdk::SdkMessage;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

const CHAT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(30);

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

#[derive(Deserialize)]
pub struct ChatPermissionResponseRequest {
    pub session_id: String,
    pub decision: String,
    #[serde(default)]
    pub feedback: Option<String>,
}

#[derive(Serialize)]
struct ChatPermissionRequestEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    session_id: String,
    tool_use_id: String,
    tool: String,
    command: String,
    input: Value,
    options: Vec<String>,
}

enum ChatSseItem {
    Sdk(SdkMessage),
    Permission(ChatPermissionRequestEvent),
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

    // Match the TUI submit path: a previous abort must not poison the next turn.
    engine.reset_abort();
    state.set_session_streaming(&active_session_id, true);

    // Get the stream from the engine
    let prompt = match activation {
        Some(activation) => {
            info!(mode = %activation.mode_id, "chat mode activation applied");
            format!("{}{}", activation.prompt_prefix, req.message)
        }
        None => req.message.clone(),
    };
    let (sse_tx, sse_rx) = tokio::sync::mpsc::unbounded_channel::<ChatSseItem>();
    let permission_state = state.clone();
    let permission_session_id = active_session_id.clone();
    let permission_sse_tx = sse_tx.clone();
    let permission_callback: allthecodes_types::callbacks::PermissionCallback =
        Arc::new(move |request: PermissionRequestPayload| {
            let state = permission_state.clone();
            let session_id = permission_session_id.clone();
            let sse_tx = permission_sse_tx.clone();
            Box::pin(async move {
                let tool_use_id = request.tool_use_id.clone();
                let command = request.legacy_command();
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                state.insert_chat_permission(&session_id, &tool_use_id, response_tx);

                let event = ChatPermissionRequestEvent {
                    event_type: "permission_request",
                    session_id: session_id.clone(),
                    tool_use_id: tool_use_id.clone(),
                    tool: request.tool_name,
                    command,
                    input: request.tool_input,
                    options: request.options,
                };

                if sse_tx.send(ChatSseItem::Permission(event)).is_err() {
                    state.remove_chat_permission(&session_id, &tool_use_id);
                    return PermissionResponsePayload::deny();
                }

                match tokio::time::timeout(CHAT_PERMISSION_TIMEOUT, response_rx).await {
                    Ok(Ok(response)) => response,
                    Ok(Err(_)) | Err(_) => {
                        state.remove_chat_permission(&session_id, &tool_use_id);
                        PermissionResponsePayload::deny()
                    }
                }
            })
        });
    let previous_permission_callback =
        engine.replace_permission_callback(Some(permission_callback));

    let stream_engine = engine.clone();
    let stream_state = state.clone();
    let stream_session_id = active_session_id.clone();
    tokio::spawn(async move {
        let mut stream =
            stream_engine.submit_message_with_overrides(&prompt, QuerySource::Sdk, overrides);
        while let Some(msg) = stream.next().await {
            let is_result = matches!(&msg, SdkMessage::Result(_));
            if sse_tx.send(ChatSseItem::Sdk(msg)).is_err() {
                break;
            }
            if is_result {
                break;
            }
        }
        stream_state.set_session_streaming(&stream_session_id, false);
        stream_engine.replace_permission_callback(previous_permission_callback);
    });

    chat_sse_response(sse_rx).into_response()
}

/// POST /api/chat/permissions/:tool_use_id/response -- Resolve a pending chat permission request.
pub async fn chat_permission_response_handler(
    AxumPath(tool_use_id): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<ChatPermissionResponseRequest>,
) -> impl IntoResponse {
    let decision = req.decision.trim().to_ascii_lowercase();
    if !matches!(decision.as_str(), "allow" | "deny" | "always_allow") {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                ProtocolApiError::BadRequest {
                    code: "invalid_permission_decision",
                    message: "decision must be allow, deny, or always_allow".to_string(),
                }
                .into_body(),
            ),
        )
            .into_response();
    }

    let resolved = state.resolve_chat_permission(
        &req.session_id,
        &tool_use_id,
        PermissionResponsePayload::new(decision, req.feedback),
    );
    if !resolved {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "permission request is no longer pending",
                "code": "stale_permission_response",
                "details": {
                    "session_id": req.session_id,
                    "tool_use_id": tool_use_id,
                },
            })),
        )
            .into_response();
    }

    Json(json!({ "status": "ok" })).into_response()
}

fn chat_sse_response(
    rx: tokio::sync::mpsc::UnboundedReceiver<ChatSseItem>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let item = rx.recv().await?;
        Some((chat_sse_event(item), rx))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn chat_sse_event(item: ChatSseItem) -> Result<Event, Infallible> {
    let event = match item {
        ChatSseItem::Sdk(msg) => {
            let event_name = msg.event_name();
            let data = serde_json::to_string(&msg)
                .unwrap_or_else(|error| format!(r#"{{"error":"serialization failed: {error}"}}"#));
            Event::default().event(event_name).data(data)
        }
        ChatSseItem::Permission(event) => {
            let data = serde_json::to_string(&event)
                .unwrap_or_else(|error| format!(r#"{{"error":"serialization failed: {error}"}}"#));
            Event::default().event("permission_request").data(data)
        }
    };
    Ok(event)
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
