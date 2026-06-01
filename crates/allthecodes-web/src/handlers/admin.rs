//! Settings, command execution, and debug handlers — admin operations.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::info;

use allthecodes_engine::types::tool::PermissionMode;

use crate::handlers::ApiError;
use crate::state::{SessionOwner, WebState};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

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
    pub ownership: crate::state::SessionOwnership,
    pub is_streaming: bool,
    pub pty: crate::ws::tui::PtyDiagnosticsSnapshot,
}

#[derive(Serialize)]
pub struct SessionTraceResponse {
    pub session_id: String,
    pub ownership: crate::state::SessionOwnership,
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

// ---------------------------------------------------------------------------
// Settings handler
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Command handler
// ---------------------------------------------------------------------------

/// POST /api/command -- Execute a slash command.
pub async fn command_handler(
    State(state): State<WebState>,
    Json(req): Json<CommandRequest>,
) -> impl IntoResponse {
    info!(command = %req.command, args = %req.args, "POST /api/command");

    let commands = crate::handlers::get_all_commands();
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

// ---------------------------------------------------------------------------
// Debug handlers
// ---------------------------------------------------------------------------

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
        is_streaming: state.is_streaming.load(std::sync::atomic::Ordering::SeqCst),
        pty: state.pty_diagnostics.snapshot(),
    })
    .into_response()
}

/// GET /api/debug/sessions/{id}/trace -- Dev-only session trace snapshot.
pub async fn debug_session_trace_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let ownership = state.ownership_snapshot();
    let is_streaming = state.is_streaming.load(std::sync::atomic::Ordering::SeqCst);

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
            state.is_streaming.store(false, std::sync::atomic::Ordering::SeqCst);
            state.release_owner(SessionOwner::ChatStream);
        }
        "chat/submit" => {
            ok = false;
            error = Some(
                "chat/submit must use /api/chat so the caller receives the SSE stream".to_string(),
            );
        }
        "permissions/respond" | "questions/respond" => {
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

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

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
