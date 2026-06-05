//! Chat, abort, and state handlers — core chat API.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use allthecodes_daemon::web::sdk_stream_to_sse;
use allthecodes_engine::types::config::QuerySource;
use allthecodes_types::sdk::SdkMessage;

use crate::handlers::ApiError;
use crate::state::{SessionOwner, WebState};

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
    pub mode: Option<String>,
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
    // Check if already streaming
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is already in progress".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }

    let activation = match crate::handlers::resolve_mode_activation(&state, req.mode.as_deref()) {
        Ok(activation) => activation,
        Err((status, error)) => return (status, Json(error)).into_response(),
    };

    let engine = state.engine();
    let active_session_id = req
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| engine.current_session_id().to_string());
    if let Err(owner) = state.try_claim_chat(active_session_id.clone()) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!(
                    "Session is currently owned by {:?}{}",
                    owner.owner,
                    owner
                        .session_id
                        .as_deref()
                        .map(|id| format!(" ({id})"))
                        .unwrap_or_default()
                ),
                code: "session_owned".into(),
            }),
        )
            .into_response();
    }

    let requested_session = active_session_id.as_str();
    info!(
        message = %req.message,
        session_id = %requested_session,
        mode = %req.mode.as_deref().unwrap_or("normal"),
        "POST /api/chat"
    );

    state.is_streaming.store(true, Ordering::SeqCst);

    // Get the stream from the engine
    let prompt = match activation {
        Some(activation) => {
            info!(mode = %activation.mode_id, "chat mode activation applied");
            format!("{}{}", activation.prompt_prefix, req.message)
        }
        None => req.message.clone(),
    };
    let stream = engine.submit_message(&prompt, QuerySource::Sdk);

    // Wrap in a stream that clears is_streaming when done
    let is_streaming = state.is_streaming.clone();
    let release_state = state.clone();
    let wrapped_stream = Box::pin(futures::stream::unfold(
        (stream, is_streaming, release_state, false),
        |(mut stream, flag, release_state, done)| async move {
            if done {
                return None;
            }
            use futures::StreamExt;
            match stream.next().await {
                Some(msg) => {
                    let is_result = matches!(&msg, SdkMessage::Result(_));
                    if is_result {
                        flag.store(false, Ordering::SeqCst);
                        release_state.release_owner(SessionOwner::ChatStream);
                    }
                    Some((msg, (stream, flag, release_state, is_result)))
                }
                None => {
                    flag.store(false, Ordering::SeqCst);
                    release_state.release_owner(SessionOwner::ChatStream);
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
    state.engine().abort();
    state.is_streaming.store(false, Ordering::SeqCst);
    state.release_owner(SessionOwner::ChatStream);
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

    Json(StateResponse {
        model: app_state.main_loop_model.clone(),
        session_id: state.engine().current_session_id().to_string(),
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
