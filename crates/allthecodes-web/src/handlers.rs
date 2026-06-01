//! Axum route handlers for the web chat API.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use allthecodes_bootstrap::SessionId;
use allthecodes_commands::Command;
use allthecodes_daemon::web::sdk_stream_to_sse;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::{QueryEngineConfig, QuerySource};
use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_session::{resume as session_resume, storage};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};
use allthecodes_types::sdk::SdkMessage;

use allthecodes_auth::{resolve_auth, AuthMethod};
use allthecodes_config::settings::{
    load_global_config, write_user_settings, ProviderProfileSettings, RawSettings,
};
use chrono::Utc;

use super::state::{SessionOwner, WebState};

type CommandProvider = fn() -> Vec<Command>;

static COMMAND_PROVIDER: OnceLock<RwLock<Option<CommandProvider>>> = OnceLock::new();

/// Install the root-owned slash-command registry used by web command routes.
pub fn set_command_provider(provider: CommandProvider) {
    let slot = COMMAND_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

fn get_all_commands() -> Vec<Command> {
    COMMAND_PROVIDER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
        .map(|provider| provider())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AbortRequest {
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
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
}

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

// Phase 3: Settings mutation
#[derive(Deserialize)]
pub struct SettingsRequest {
    pub action: String,
    pub value: serde_json::Value,
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub ok: bool,
    pub message: String,
}

// Phase 3: Command execution
#[derive(Deserialize)]
pub struct CommandRequest {
    pub command: String,
    #[serde(default)]
    pub args: String,
}

#[derive(Serialize)]
pub struct CommandResponse {
    #[serde(rename = "type")]
    pub response_type: String, // "output" | "clear" | "error"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct DebugActionRequest {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize)]
pub struct DebugActionResponse {
    pub action_id: String,
    pub session_id: String,
    pub trace_ref: String,
    pub mutates_runtime: bool,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct DebugStateResponse {
    pub enabled: bool,
    pub session_id: String,
    pub ownership: super::state::SessionOwnership,
    pub is_streaming: bool,
    pub pty: crate::ws::tui::PtyDiagnosticsSnapshot,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

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
        "POST /api/chat"
    );

    state.is_streaming.store(true, Ordering::SeqCst);

    // Get the stream from the engine
    let stream = engine.submit_message(&req.message, QuerySource::Sdk);

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
    let commands: Vec<CommandInfo> = get_all_commands()
        .iter()
        .map(|c| CommandInfo {
            name: c.name.clone(),
            aliases: c.aliases.clone(),
            description: c.description.clone(),
        })
        .collect();

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
    })
}

/// GET /api/debug/state -- Dev-only diagnostics snapshot.
pub async fn debug_state_handler(State(state): State<WebState>) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let engine = state.engine();
    Json(DebugStateResponse {
        enabled: true,
        session_id: engine.current_session_id().to_string(),
        ownership: state.ownership_snapshot(),
        is_streaming: state.is_streaming.load(Ordering::SeqCst),
        pty: state.pty_diagnostics.snapshot(),
    })
    .into_response()
}

/// GET /api/debug/sessions/{id}/trace -- Dev-only session trace snapshot.
#[derive(Serialize)]
pub struct SessionTraceResponse {
    pub session_id: String,
    pub ownership: super::state::SessionOwnership,
    pub is_streaming: bool,
    pub current_turn: Option<TurnTraceItem>,
    pub pending_permissions: Vec<String>,
    pub pending_questions: Vec<String>,
}

#[derive(Serialize)]
pub struct TurnTraceItem {
    pub status: String,
    pub started_at: Option<i64>,
}

pub async fn debug_session_trace_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let ownership = state.ownership_snapshot();
    let is_streaming = state.is_streaming.load(Ordering::SeqCst);

    Json(SessionTraceResponse {
        session_id: id,
        ownership,
        is_streaming,
        current_turn: if is_streaming {
            Some(TurnTraceItem {
                status: "streaming".to_string(),
                started_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                ),
            })
        } else {
            None
        },
        pending_permissions: Vec::new(),
        pending_questions: Vec::new(),
    })
    .into_response()
}

/// POST /api/debug/actions/{*action} -- Dev-only scripted action endpoint.
pub async fn debug_action_handler(
    AxumPath(path_action): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<DebugActionRequest>,
) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let _params = req.params;
    let action = req.action.unwrap_or(path_action);
    let mutates_runtime = matches!(action.as_str(), "chat/abort");
    let mut ok = true;
    let mut error = None;

    match action.as_str() {
        "ui/state" | "fixtures/load" | "tui/open" | "tui/input" | "tui/resize" | "tui/close" => {}
        "chat/abort" => {
            state.engine().abort();
            state.is_streaming.store(false, Ordering::SeqCst);
            state.release_owner(SessionOwner::ChatStream);
        }
        "chat/submit" => {
            ok = false;
            error = Some(
                "chat/submit must use /api/chat so the caller receives the SSE stream".to_string(),
            );
        }
        "permissions/respond" | "questions/respond" => {
            // Permission/question responses are handled via the IPC WebSocket
            // connection. This debug action acknowledges the request but the
            // actual response must be sent over the active IPC WS connection.
            // For testing, use the async debug fixture setup instead.
            ok = true;
        }
        _ => {
            ok = false;
            error = Some(format!("Unknown debug action: {}", action));
        }
    }

    let session_id = state.engine().current_session_id().to_string();
    let action_id = format!(
        "debug-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    );
    Json(DebugActionResponse {
        trace_ref: format!("debug/actions/{action_id}"),
        action_id,
        session_id,
        mutates_runtime,
        ok,
        error,
    })
    .into_response()
}

/// POST /api/settings -- Mutate application settings.
pub async fn settings_handler(
    State(state): State<WebState>,
    Json(req): Json<SettingsRequest>,
) -> impl IntoResponse {
    info!(action = %req.action, "POST /api/settings");

    match req.action.as_str() {
        "set_model" => {
            let model = req.value.as_str().unwrap_or("").to_string();
            let settings = state.engine().app_state().settings.clone();
            let available = settings.available_models.clone();
            let resolved =
                allthecodes_commands::model::resolve_model_alias_with_settings(&model, &settings);
            if let Err(message) = allthecodes_commands::model::check_available_with_settings(
                &resolved, &available, &settings,
            ) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: format!("Rejected: {}", message),
                    }),
                );
            }
            state.engine().update_app_state(|s| {
                s.main_loop_model = resolved.clone();
                s.settings.model = Some(resolved.clone());
            });
            (
                StatusCode::OK,
                Json(SettingsResponse {
                    ok: true,
                    message: format!("Model set to {}", resolved),
                }),
            )
        }
        "set_permission_mode" => {
            let mode_str = req.value.as_str().unwrap_or("default");
            let mode = match mode_str {
                "auto" => PermissionMode::Auto,
                "bypass" => PermissionMode::Bypass,
                "plan" => PermissionMode::Plan,
                _ => PermissionMode::Default,
            };
            let mut blocked_by_policy = false;
            let mut effective_mode = mode.clone();
            state.engine().update_app_state(|s| {
                let transition =
                    allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
                        &mut s.tool_permission_context,
                        mode.clone(),
                    );
                blocked_by_policy = transition.auto_mode_blocked_by_policy;
                effective_mode = s.tool_permission_context.mode.clone();
            });
            if blocked_by_policy {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message:
                            "Auto mode is disabled by configuration (permissions.enableAutoMode=false)."
                                .to_string(),
                    }),
                );
            }
            (
                StatusCode::OK,
                Json(SettingsResponse {
                    ok: true,
                    message: format!("Permission mode set to {}", effective_mode.as_str()),
                }),
            )
        }
        "set_thinking" => {
            let enabled = req.value.as_bool();
            state.engine().update_app_state(|s| {
                s.thinking_enabled = enabled;
            });
            (
                StatusCode::OK,
                Json(SettingsResponse {
                    ok: true,
                    message: format!("Thinking set to {:?}", enabled),
                }),
            )
        }
        "set_fast_mode" => {
            let enabled = req.value.as_bool().unwrap_or(false);
            state.engine().update_app_state(|s| {
                s.fast_mode = enabled;
            });
            (
                StatusCode::OK,
                Json(SettingsResponse {
                    ok: true,
                    message: format!("Fast mode {}", if enabled { "enabled" } else { "disabled" }),
                }),
            )
        }
        "set_effort" => {
            let effort = req.value.as_str().map(|s| s.to_string());
            state.engine().update_app_state(|s| {
                s.effort_value = effort.clone();
            });
            (
                StatusCode::OK,
                Json(SettingsResponse {
                    ok: true,
                    message: format!("Effort set to {:?}", effort),
                }),
            )
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(SettingsResponse {
                ok: false,
                message: format!("Unknown action: {}", req.action),
            }),
        ),
    }
}

/// POST /api/command -- Execute a slash command.
pub async fn command_handler(
    State(state): State<WebState>,
    Json(req): Json<CommandRequest>,
) -> impl IntoResponse {
    info!(command = %req.command, args = %req.args, "POST /api/command");

    let commands = get_all_commands();
    let cmd = commands
        .iter()
        .find(|c| c.name == req.command || c.aliases.contains(&req.command));

    let cmd = match cmd {
        Some(c) => c,
        None => {
            return Json(CommandResponse {
                response_type: "error".into(),
                content: format!("Unknown command: /{}", req.command),
                session_id: None,
            });
        }
    };

    // Build a CommandContext
    let messages = state.engine().messages();
    let app_state = state.engine().app_state();
    let cwd = std::path::PathBuf::from(state.engine().cwd());

    let mut ctx = allthecodes_commands::CommandContext {
        messages,
        cwd,
        app_state: app_state.clone(),
        session_id: state.engine().current_session_id(),
    };

    match cmd.handler.execute(&req.args, &mut ctx).await {
        Ok(result) => {
            // Apply any state mutations from the command
            // Commands mutate ctx.app_state in-place; write it back
            state.engine().update_app_state(|s| {
                s.main_loop_model = ctx.app_state.main_loop_model.clone();
                s.settings = ctx.app_state.settings.clone();
                s.tool_permission_context = ctx.app_state.tool_permission_context.clone();
                s.thinking_enabled = ctx.app_state.thinking_enabled;
                s.fast_mode = ctx.app_state.fast_mode;
                s.effort_value = ctx.app_state.effort_value.clone();
            });

            match result {
                allthecodes_commands::CommandResult::Output(text) => Json(CommandResponse {
                    response_type: "output".into(),
                    content: text,
                    session_id: None,
                }),
                allthecodes_commands::CommandResult::SwitchSession {
                    session_id,
                    messages,
                    notice,
                } => {
                    state.engine().set_current_session_id(session_id.clone());
                    state.engine().replace_messages(messages);
                    Json(CommandResponse {
                        response_type: "switch_session".into(),
                        content: format!("{notice}\nSession: {session_id}"),
                        session_id: Some(session_id.to_string()),
                    })
                }
                allthecodes_commands::CommandResult::Clear => {
                    let session_id = state.engine().start_new_session();
                    Json(CommandResponse {
                        response_type: "clear".into(),
                        content: format!("Started a new session: {}", session_id),
                        session_id: Some(session_id.to_string()),
                    })
                }
                allthecodes_commands::CommandResult::Exit(msg) => Json(CommandResponse {
                    response_type: "output".into(),
                    content: msg,
                    session_id: None,
                }),
                allthecodes_commands::CommandResult::Query(_msgs) => {
                    // TODO: inject messages and start a new SSE stream
                    Json(CommandResponse {
                        response_type: "output".into(),
                        content: "Command queued (query commands not yet supported in web UI)"
                            .into(),
                        session_id: None,
                    })
                }
                allthecodes_commands::CommandResult::None => Json(CommandResponse {
                    response_type: "output".into(),
                    content: "OK".into(),
                    session_id: None,
                }),
            }
        }
        Err(e) => Json(CommandResponse {
            response_type: "error".into(),
            content: format!("Command error: {}", e),
            session_id: None,
        }),
    }
}

/// GET /api/capabilities -- Return capability discovery map.
#[derive(Serialize)]
pub struct CapabilityDiscoveryResponse {
    pub capabilities: std::collections::HashMap<String, bool>,
}

pub async fn capabilities_handler() -> impl IntoResponse {
    let mut caps = std::collections::HashMap::new();
    // Ready capabilities
    caps.insert("chat".into(), true);
    caps.insert("sessions".into(), true);
    caps.insert("settings".into(), true);
    caps.insert("debug".into(), true);
    caps.insert("state".into(), true);
    // Not yet implemented
    caps.insert("auth".into(), true);
    caps.insert("profiles".into(), true);
    caps.insert("gateways".into(), false);
    caps.insert("models".into(), true);
    caps.insert("providers".into(), true);
    caps.insert("credentials".into(), true);
    caps.insert("usage".into(), false);
    caps.insert("skills".into(), false);
    caps.insert("memory".into(), false);
    caps.insert("kanban".into(), false);
    caps.insert("jobs".into(), false);
    caps.insert("group_chat".into(), false);
    caps.insert("files".into(), false);
    caps.insert("logs".into(), false);
    caps.insert("backend_services".into(), false);
    Json(CapabilityDiscoveryResponse { capabilities: caps })
}

/// Catch-all handler for unregistered /api/* paths.
/// Returns 501 JSON instead of falling through to static file serving.
pub async fn api_fallback_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let status = if path.starts_with("api/") {
        StatusCode::NOT_IMPLEMENTED
    } else {
        StatusCode::NOT_FOUND
    };
    (
        status,
        Json(ApiError {
            error: "API endpoint not implemented".into(),
            code: "capability_not_implemented".into(),
        }),
    )
}

// ---------------------------------------------------------------------------
// Auth endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

/// GET /api/auth/status — Return current authentication status.
pub async fn auth_status_handler() -> impl IntoResponse {
    // Use allthecodes-auth to resolve current auth state
    let auth = resolve_auth();
    let authenticated = auth.is_authenticated();
    let subject = auth.api_key().map(|k| {
        if k.len() > 8 {
            format!("{}...{}", &k[..4], &k[k.len() - 4..])
        } else {
            "unknown".to_string()
        }
    });
    let expires_at = None;

    Json(AuthStatusResponse {
        authenticated,
        auth_required: Some(false), // local/anonymous mode
        subject,
        expires_at,
        profile_id: None,
        session_id: None,
    })
}

/// POST /api/auth/login — Authenticate with an API key or token.
pub async fn auth_login_handler(Json(req): Json<LoginRequest>) -> Response {
    // Accept API key from token field
    if let Some(token) = &req.token {
        if allthecodes_auth::api_key::validate_api_key(token) {
            match allthecodes_auth::api_key::store_api_key(token) {
                Ok(_) => {
                    return Json(LoginResponse {
                        authenticated: true,
                        session_id: None,
                        expires_at: None,
                        subject: Some(format!("{}...{}", &token[..4], &token[token.len() - 4..])),
                        profile_id: None,
                        access_token: None,
                        bearer_token: Some(token.clone()),
                    })
                    .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: format!("Failed to store API key: {}", e),
                            code: "internal_error".into(),
                        }),
                    )
                        .into_response();
                }
            }
        }
        // Also try OpenAI key validation
        if allthecodes_auth::api_key::validate_openai_api_key(token) {
            match allthecodes_auth::api_key::store_openai_api_key(token) {
                Ok(_) => {
                    return Json(LoginResponse {
                        authenticated: true,
                        session_id: None,
                        expires_at: None,
                        subject: Some(format!(
                            "openai:{}...{}",
                            &token[..4],
                            &token[token.len() - 4..]
                        )),
                        profile_id: None,
                        access_token: None,
                        bearer_token: Some(token.clone()),
                    })
                    .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: format!("Failed to store API key: {}", e),
                            code: "internal_error".into(),
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: "Invalid API key format".into(),
            code: "validation_error".into(),
        }),
    )
        .into_response()
}

/// POST /api/auth/logout — Clear authentication.
pub async fn auth_logout_handler() -> impl IntoResponse {
    let _ = allthecodes_auth::oauth_logout();
    let _ = allthecodes_auth::api_key::remove_api_key();
    let _ = allthecodes_auth::api_key::remove_openai_api_key();
    StatusCode::OK
}

/// POST /api/auth/refresh — Refresh the session state.
pub async fn auth_refresh_handler() -> impl IntoResponse {
    auth_status_handler().await
}

// ---------------------------------------------------------------------------
// Profile endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ProfileListResponse {
    pub active_profile_id: Option<String>,
    pub profiles: Vec<ProfileSummary>,
}

#[derive(Deserialize)]
pub struct ProfileCreateRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ProfileUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileImportRequest {
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// Load profiles from user settings, returning the active profile id and
/// a sorted list of profiles.
fn load_profile_list() -> (Option<String>, Vec<ProfileSummary>) {
    let settings = load_global_config().unwrap_or_default();
    let active_id = settings.active_auth_profile.clone();
    let mut profiles: Vec<ProfileSummary> = Vec::new();

    if let Some(auth_profiles) = settings.auth_profiles {
        for (id, _profile) in auth_profiles {
            let active = Some(&id) == active_id.as_ref();
            profiles.push(ProfileSummary {
                id: id.clone(),
                name: id.clone(),
                active,
                created_at: Some(chrono::Utc::now().timestamp()),
                updated_at: Some(chrono::Utc::now().timestamp()),
            });
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    (active_id, profiles)
}

fn save_profile_list(
    active_id: &Option<String>,
    profiles: &[ProfileSummary],
) -> Result<(), String> {
    let mut settings = load_global_config().unwrap_or_default();
    settings.active_auth_profile = active_id.clone();

    let mut auth_profiles = std::collections::HashMap::new();
    for p in profiles {
        auth_profiles.insert(
            p.id.clone(),
            ProviderProfileSettings {
                backend: None,
                api_provider: None,
                model: None,
                available_models: None,
                model_capabilities: None,
                model_reasoning_effort: None,
                base_url: None,
                api_key: None,
                env: None,
                auth_source: None,
                extra: HashMap::new(),
            },
        );
    }
    settings.auth_profiles = Some(auth_profiles);

    write_user_settings(&settings).map_err(|e| e.to_string())?;
    Ok(())
}

/// GET /api/profiles — List all profiles.
pub async fn profiles_list_handler() -> impl IntoResponse {
    let (active_id, profiles) = load_profile_list();
    Json(ProfileListResponse {
        active_profile_id: active_id,
        profiles,
    })
}

/// POST /api/profiles — Create a new profile.
pub async fn profiles_create_handler(Json(req): Json<ProfileCreateRequest>) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Profile name cannot be empty".into(),
                code: "validation_error".into(),
            }),
        )
            .into_response();
    }

    let (active_id, mut profiles) = load_profile_list();

    // Check for duplicate
    if profiles.iter().any(|p| p.id == req.name.trim()) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!("Profile '{}' already exists", req.name),
                code: "conflict".into(),
            }),
        )
            .into_response();
    }

    let now = chrono::Utc::now().timestamp();
    profiles.push(ProfileSummary {
        id: req.name.trim().to_string(),
        name: req.name.trim().to_string(),
        active: false,
        created_at: Some(now),
        updated_at: Some(now),
    });

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/profiles/{id} — Get a single profile detail.
pub async fn profiles_detail_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (_, profiles) = load_profile_list();
    if let Some(profile) = profiles.into_iter().find(|p| p.id == id) {
        Json(profile).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Profile '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response()
    }
}

/// PATCH /api/profiles/{id} — Update a profile.
pub async fn profiles_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProfileUpdateRequest>,
) -> impl IntoResponse {
    let (active_id, mut profiles) = load_profile_list();

    let profile = match profiles.iter_mut().find(|p| p.id == id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    if let Some(new_name) = &req.name {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Profile name cannot be empty".into(),
                    code: "validation_error".into(),
                }),
            )
                .into_response();
        }
        profile.id = trimmed.clone();
        profile.name = trimmed;
        profile.updated_at = Some(chrono::Utc::now().timestamp());
    }

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/profiles/{id} — Delete a profile.
pub async fn profiles_delete_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (active_id, mut profiles) = load_profile_list();

    let pos = match profiles.iter().position(|p| p.id == id) {
        Some(pos) => pos,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    profiles.remove(pos);

    let active_id = if active_id.as_deref() == Some(&id) {
        None
    } else {
        active_id
    };

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/profiles/{id}/switch — Switch the active profile.
pub async fn profiles_switch_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (_, mut profiles) = load_profile_list();

    if !profiles.iter().any(|p| p.id == id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Profile '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response();
    }

    // Update active flags
    for p in &mut profiles {
        p.active = p.id == id;
    }

    let new_active_id = Some(id.clone());
    match save_profile_list(&new_active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: new_active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/profiles/import — Import a profile from a JSON payload.
pub async fn profiles_import_handler(Json(req): Json<ProfileImportRequest>) -> impl IntoResponse {
    let payload = match req.payload {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Missing 'payload' field".into(),
                    code: "validation_error".into(),
                }),
            )
                .into_response();
        }
    };

    // Extract profile name from payload
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("imported");
    let (active_id, mut profiles) = load_profile_list();

    let now = chrono::Utc::now().timestamp();
    profiles.push(ProfileSummary {
        id: name.to_string(),
        name: name.to_string(),
        active: false,
        created_at: Some(now),
        updated_at: Some(now),
    });

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/profiles/{id}/export — Export a profile as JSON.
pub async fn profiles_export_handler(AxumPath(id): AxumPath<String>) -> Response {
    let (_, profiles) = load_profile_list();
    let profile = match profiles.into_iter().find(|p| p.id == id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    Json(profile).into_response()
}

// ---------------------------------------------------------------------------
// Provider endpoints (Phase 3)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ProviderListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub providers: Vec<ProviderSummary>,
}

#[derive(Deserialize)]
pub struct ProviderCreateRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct ProviderUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub base_url: Option<String>,
}

/// Build a list of providers from settings auth profiles.
fn provider_summaries_from_settings() -> Vec<ProviderSummary> {
    let settings = load_global_config().unwrap_or_default();
    let mut providers = Vec::new();

    if let Some(profiles) = settings.auth_profiles {
        for (id, profile) in profiles {
            let kind = profile
                .api_provider
                .clone()
                .unwrap_or_else(|| "anthropic".to_string());
            providers.push(ProviderSummary {
                id: id.clone(),
                name: id.clone(),
                kind,
                enabled: true,
                preset: Some(false),
                profile_id: None,
                base_url: profile.base_url.clone(),
                auth_kind: Some("api_key".to_string()),
                credential_status: None,
                credential_subject: None,
                models_count: profile.available_models.as_ref().map(|m| m.len()),
                last_refreshed_at: None,
                diagnostics: None,
            });
        }
    }

    providers
}

/// GET /api/providers — List all providers.
pub async fn providers_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let providers = provider_summaries_from_settings();

    // Also add the engine's current provider if it's not already listed
    let engine = state.engine();
    let _current_model = engine.app_state().main_loop_model.clone();

    Json(ProviderListResponse {
        profile_id: None,
        providers,
    })
}

/// POST /api/providers — Create a new provider.
pub async fn providers_create_handler(Json(req): Json<ProviderCreateRequest>) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    if profiles.contains_key(&req.name) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!("Provider '{}' already exists", req.name),
                code: "conflict".into(),
            }),
        )
            .into_response();
    }

    let profile = ProviderProfileSettings {
        backend: None,
        api_provider: Some(req.kind.clone()),
        model: None,
        available_models: None,
        model_capabilities: None,
        model_reasoning_effort: None,
        base_url: req.base_url.clone(),
        api_key: None,
        env: None,
        auth_source: None,
        extra: HashMap::new(),
    };
    profiles.insert(req.name.clone(), profile);
    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// PATCH /api/providers/{id} — Update a provider.
pub async fn providers_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProviderUpdateRequest>,
) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    // Check for duplicate name before mutable access
    if let Some(name) = &req.name {
        if name != &id && profiles.contains_key(name) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: format!("Provider '{}' already exists", name),
                    code: "conflict".into(),
                }),
            )
                .into_response();
        }
    }

    let profile = match profiles.get_mut(&id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Provider '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    if let Some(base_url) = &req.base_url {
        profile.base_url = Some(base_url.clone());
    }

    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/providers/{id} — Delete a provider.
pub async fn providers_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    if profiles.remove(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Provider '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response();
    }

    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/providers/{id}/models/refresh — Refresh models from provider.
pub async fn providers_refresh_models_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    let engine = state.engine();
    let app_state = engine.app_state();
    let available = &app_state.settings.available_models;

    let models: Vec<ModelSummary> = available
        .iter()
        .map(|m| ModelSummary {
            id: m.clone(),
            provider_id: id.clone(),
            provider_name: Some(id.clone()),
            display_name: Some(m.clone()),
            alias: None,
            visible: true,
            default: None,
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
            supports_vision: None,
            updated_at: Some(Utc::now().timestamp()),
        })
        .collect();

    Json(ModelDiscoveryResponse {
        provider_id: id,
        refreshed_at: Some(Utc::now().timestamp()),
        models,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Model endpoints (Phase 3)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ModelRegistryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model_id: Option<String>,
    pub models: Vec<ModelSummary>,
}

#[derive(Deserialize)]
pub struct ModelUpdateRequest {
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
}

#[derive(Deserialize)]
pub struct SetDefaultModelRequest {
    pub model_id: String,
}

#[derive(Serialize)]
pub struct ModelDiscoveryResponse {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    pub models: Vec<ModelSummary>,
}

/// GET /api/models — List the model registry.
pub async fn models_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let engine = state.engine();
    let app_state = engine.app_state();
    let available = &app_state.settings.available_models;
    let current_model = &app_state.main_loop_model;

    let models: Vec<ModelSummary> = available
        .iter()
        .map(|m| ModelSummary {
            id: m.clone(),
            provider_id: "default".to_string(),
            provider_name: None,
            display_name: Some(m.clone()),
            alias: None,
            visible: true,
            default: Some(m == current_model),
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
            supports_vision: None,
            updated_at: None,
        })
        .collect();

    Json(ModelRegistryResponse {
        profile_id: None,
        default_model_id: Some(current_model.clone()).filter(|m| !m.is_empty()),
        models,
    })
}

/// PATCH /api/models/{id} — Update a model's properties.
pub async fn models_update_handler(
    AxumPath(_id): AxumPath<String>,
    Json(_req): Json<ModelUpdateRequest>,
) -> Response {
    // Model update is a no-op in this initial implementation
    // The engine's available_models list is read-only from settings
    Json(ModelRegistryResponse {
        profile_id: None,
        default_model_id: None,
        models: Vec::new(),
    })
    .into_response()
}

/// POST /api/models/default — Set the default model.
pub async fn models_set_default_handler(
    State(state): State<WebState>,
    Json(req): Json<SetDefaultModelRequest>,
) -> Response {
    state.engine().update_app_state(|s| {
        s.main_loop_model = req.model_id.clone();
        s.settings.model = Some(req.model_id.clone());
    });

    Json(SettingsResponse {
        ok: true,
        message: format!("Default model set to {}", req.model_id),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Credential & OAuth endpoints (Phase 3)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CredentialSummary {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Serialize)]
pub struct CredentialStatusResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub credentials: Vec<CredentialSummary>,
}

#[derive(Serialize)]
pub struct OAuthStartResponse {
    pub flow_id: String,
    pub provider: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct OAuthStartRequest {
    pub provider: String,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Serialize)]
pub struct OAuthPollResponse {
    pub flow_id: String,
    pub provider: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// GET /api/credentials — Return credential status.
pub async fn credentials_handler() -> impl IntoResponse {
    let auth = resolve_auth();
    let mut credentials = Vec::new();

    if auth.is_authenticated() {
        let subject = auth.api_key().map(|k| {
            if k.len() > 8 {
                format!("{}...{}", &k[..4], &k[k.len() - 4..])
            } else {
                "configured".to_string()
            }
        });

        credentials.push(CredentialSummary {
            provider: "anthropic".to_string(),
            provider_id: None,
            provider_name: Some("Anthropic".to_string()),
            status: "configured".to_string(),
            subject,
            expires_at: None,
            updated_at: None,
            profile_id: None,
        });
    }

    Json(CredentialStatusResponse {
        profile_id: None,
        credentials,
    })
}

/// POST /api/oauth/{provider}/start — Start an OAuth flow.
pub async fn oauth_start_handler(
    AxumPath(provider): AxumPath<String>,
    Json(_req): Json<OAuthStartRequest>,
) -> Response {
    Json(OAuthStartResponse {
        flow_id: format!("oauth-{}", Utc::now().timestamp()),
        provider,
        status: "failed".to_string(),
        verification_uri: None,
        user_code: None,
        expires_at: None,
        interval_ms: None,
        message: Some(
            "OAuth flow is not available in the web UI yet. Use the CLI `/login` command instead."
                .to_string(),
        ),
    })
    .into_response()
}

/// POST /api/oauth/{provider}/poll — Poll OAuth flow status.
pub async fn oauth_poll_handler(AxumPath(provider): AxumPath<String>) -> Response {
    Json(OAuthPollResponse {
        flow_id: String::new(),
        provider,
        status: "failed".to_string(),
        credential: None,
        message: Some("OAuth flow is not available in the web UI yet.".to_string()),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Session management endpoints (Phase 2)
// ---------------------------------------------------------------------------

/// Lightweight description of a workspace used to group sessions.
#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub key: String,
    pub root: String,
    pub name: String,
}

/// Response shape for `GET /api/sessions`.
#[derive(Serialize)]
pub struct SessionListResponse {
    /// Workspace derived from the engine's cwd — the UI uses this to mark
    /// which group is "current".
    pub current_workspace: WorkspaceInfo,
    /// Session id currently loaded in the engine.
    pub active_session_id: String,
    /// All known sessions on disk, sorted by last_modified desc.
    pub sessions: Vec<SessionSummary>,
}

/// Serializable session summary including derived grouping fields.
#[derive(Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    pub message_count: usize,
    pub cwd: String,
    pub title: String,
    pub workspace_key: String,
    pub workspace_root: String,
    pub workspace_name: String,
}

impl From<storage::SessionInfo> for SessionSummary {
    fn from(s: storage::SessionInfo) -> Self {
        Self {
            session_id: s.session_id,
            created_at: s.created_at,
            last_modified: s.last_modified,
            message_count: s.message_count,
            cwd: s.cwd,
            title: s.title,
            workspace_key: s.workspace_key,
            workspace_root: s.workspace_root,
            workspace_name: s.workspace_name,
        }
    }
}

/// Simplified message shape used by session detail / resume responses.
#[derive(Serialize)]
pub struct StoredMessage {
    pub uuid: String,
    pub timestamp: i64,
    pub role: String,
    /// Plain-text view of the message (concatenation of text blocks for
    /// assistant, or the raw text for a user message).
    pub content: String,
    /// Structured blocks (text / tool_use / tool_result / thinking / image)
    /// when available — matches the shape the frontend already renders for
    /// live messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,
}

#[derive(Serialize)]
pub struct SessionDetailResponse {
    pub session_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    pub cwd: String,
    pub title: String,
    pub workspace_name: String,
    pub messages: Vec<StoredMessage>,
}

#[derive(Serialize)]
pub struct NewSessionResponse {
    pub session_id: String,
}

/// GET /api/sessions -- List all sessions with workspace grouping metadata.
pub async fn sessions_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let engine = state.engine();
    let cwd_str = engine.cwd().to_string();
    let cwd_path = Path::new(&cwd_str);

    let ws_key = storage::workspace_key(cwd_path);
    let ws_root = storage::workspace_root(cwd_path);
    let ws_name = storage::workspace_name(&ws_root);

    let sessions = match storage::list_sessions() {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to list sessions");
            Vec::new()
        }
    };

    let summaries: Vec<SessionSummary> = sessions.into_iter().map(SessionSummary::from).collect();

    Json(SessionListResponse {
        current_workspace: WorkspaceInfo {
            key: ws_key,
            root: ws_root.to_string_lossy().to_string(),
            name: ws_name,
        },
        active_session_id: engine.current_session_id().to_string(),
        sessions: summaries,
    })
}

/// GET /api/sessions/:id -- Load a session's message history for preview.
pub async fn session_detail_handler(
    AxumPath(id): AxumPath<String>,
    State(_state): State<WebState>,
) -> impl IntoResponse {
    info!(session_id = %id, "GET /api/sessions/:id");

    let messages = match session_resume::resume_session(&id) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Session not found: {}", e),
                    code: "session_not_found".into(),
                }),
            )
                .into_response();
        }
    };

    // Look up disk metadata for title / cwd / timestamps.
    let info = storage::list_sessions()
        .ok()
        .and_then(|list| list.into_iter().find(|s| s.session_id == id));

    let (title, cwd, created_at, last_modified, workspace_name) = match info {
        Some(i) => (
            i.title,
            i.cwd,
            i.created_at,
            i.last_modified,
            i.workspace_name,
        ),
        None => (String::new(), String::new(), 0, 0, String::new()),
    };

    let rendered: Vec<StoredMessage> = messages.iter().map(stored_message_from).collect();

    Json(SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        messages: rendered,
    })
    .into_response()
}

/// POST /api/sessions/new -- Start a fresh session in the current workspace.
///
/// The existing engine is detached (its history is preserved on disk by the
/// auto-save path) and a new engine is constructed in its place with an empty
/// message history and a new session id.
pub async fn session_new_handler(State(state): State<WebState>) -> impl IntoResponse {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is in progress — abort it before starting a new session".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }
    if let Some(response) = ownership_conflict_response(&state) {
        return response;
    }

    let engine = rebuild_engine(&state, None);
    let new_id = engine.current_session_id().to_string();
    state.replace_engine(engine);

    info!(session_id = %new_id, "POST /api/sessions/new");
    Json(NewSessionResponse { session_id: new_id }).into_response()
}

/// POST /api/sessions/:id/resume -- Load an existing session into the engine.
pub async fn session_resume_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "A query is in progress — abort it before switching sessions".into(),
                code: "engine_busy".into(),
            }),
        )
            .into_response();
    }
    if let Some(response) = ownership_conflict_response(&state) {
        return response;
    }

    info!(session_id = %id, "POST /api/sessions/:id/resume");

    let messages = match session_resume::resume_session(&id) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Session not found: {}", e),
                    code: "session_not_found".into(),
                }),
            )
                .into_response();
        }
    };

    let engine = rebuild_engine_with_session_id(&state, Some(messages.clone()), Some(&id));
    state.replace_engine(engine);

    let rendered: Vec<StoredMessage> = messages.iter().map(stored_message_from).collect();

    let info = storage::list_sessions()
        .ok()
        .and_then(|list| list.into_iter().find(|s| s.session_id == id));

    let (title, cwd, created_at, last_modified, workspace_name) = match info {
        Some(i) => (
            i.title,
            i.cwd,
            i.created_at,
            i.last_modified,
            i.workspace_name,
        ),
        None => (String::new(), String::new(), 0, 0, String::new()),
    };

    Json(SessionDetailResponse {
        session_id: id,
        created_at,
        last_modified,
        cwd,
        title,
        workspace_name,
        messages: rendered,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ownership_conflict_response(state: &WebState) -> Option<Response> {
    let owner = state.ownership_snapshot();
    if owner.owner == SessionOwner::None {
        return None;
    }
    Some(
        (
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
            .into_response(),
    )
}

fn debug_enabled() -> bool {
    std::env::var("ALLTHECODES_WEB_DEBUG")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn debug_disabled_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "Debug API is disabled".into(),
            code: "debug_disabled".into(),
        }),
    )
        .into_response()
}

/// Build a fresh engine that inherits the current engine's config, with an
/// optional seed message list. The new engine gets a freshly minted session id.
fn rebuild_engine(state: &WebState, seed: Option<Vec<Message>>) -> Arc<QueryEngine> {
    rebuild_engine_with_session_id(state, seed, None)
}

/// Rebuild with a caller-provided session id (used by resume so the engine's
/// auto-save keeps writing back to the resumed session file).
fn rebuild_engine_with_session_id(
    state: &WebState,
    seed: Option<Vec<Message>>,
    session_id: Option<&str>,
) -> Arc<QueryEngine> {
    let current = state.engine();
    let mut cfg: QueryEngineConfig = current.config_ref().clone();
    cfg.initial_messages = seed;

    let mut engine = QueryEngine::new(cfg);
    engine.set_hook_runner(current.hook_runner());
    engine.set_command_dispatcher(current.command_dispatcher());
    if let Some(id) = session_id {
        let id = SessionId::from_string(id);
        engine.session_id = id.clone();
        engine.set_current_session_id(id);
    }
    Arc::new(engine)
}

/// Convert an internal `Message` into the lightweight wire form used by the
/// session detail / resume responses.
fn stored_message_from(msg: &Message) -> StoredMessage {
    match msg {
        Message::User(u) => {
            let (text, blocks) = match &u.content {
                MessageContent::Text(t) => (t.clone(), None),
                MessageContent::Blocks(bs) => {
                    let text = bs
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    (text, Some(bs.clone()))
                }
            };
            StoredMessage {
                uuid: u.uuid.to_string(),
                timestamp: u.timestamp,
                role: "user".into(),
                content: text,
                content_blocks: blocks,
            }
        }
        Message::Assistant(a) => {
            let text = a
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            StoredMessage {
                uuid: a.uuid.to_string(),
                timestamp: a.timestamp,
                role: "assistant".into(),
                content: text,
                content_blocks: Some(a.content.clone()),
            }
        }
        Message::System(s) => StoredMessage {
            uuid: s.uuid.to_string(),
            timestamp: s.timestamp,
            role: "system".into(),
            content: s.content.clone(),
            content_blocks: None,
        },
        Message::Progress(p) => StoredMessage {
            uuid: p.uuid.to_string(),
            timestamp: p.timestamp,
            role: "progress".into(),
            content: String::new(),
            content_blocks: None,
        },
        Message::Attachment(a) => StoredMessage {
            uuid: a.uuid.to_string(),
            timestamp: a.timestamp,
            role: "attachment".into(),
            content: String::new(),
            content_blocks: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
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
}
