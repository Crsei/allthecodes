//! `/api/ipc/ws` — WebSocket-based IPC bridge for FrontendMessage/BackendMessage.
//!
//! Provides bidirectional communication between the React frontend and the Rust
//! engine using the structured `FrontendMessage` / `BackendMessage` protocol.
//!
//! ## Wire protocol
//!
//! **Client → Server** (FrontendMessage as JSON text frames)
//! ```json
//! {"type":"submit_prompt","text":"hello","id":"ui-1"}
//! {"type":"permission_response","tool_use_id":"tool-1","decision":"allow"}
//! {"type":"question_response","id":"q-1","text":"yes"}
//! {"type":"abort_query"}
//! {"type":"slash_command","raw":"/help"}
//! ```
//!
//! **Server → Client** (BackendMessage as JSON text frames)
//! ```json
//! {"type":"ready","session_id":"...","model":"...","cwd":"...","permission_mode":"...","available_models":[...]}
//! {"type":"stream_start","message_id":"..."}
//! {"type":"stream_delta","message_id":"...","text":"Hello"}
//! {"type":"tool_use","id":"tool-1","name":"Bash","input":{...}}
//! {"type":"permission_request","tool_use_id":"tool-1","tool":"Bash","command":"...","input":{...},"options":[...]}
//! {"type":"question_request","id":"q-1","text":"Continue?","choices":["Yes","No"],"allow_free_text":false}
//! {"type":"usage_update","input_tokens":100,"output_tokens":50,"cost_usd":0.01}
//! {"type":"error","message":"...","recoverable":false}
//! ```

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tracing::{info, warn};

use allthecodes_ipc::runtime::IpcRuntime;
use allthecodes_ipc_protocol::{BackendMessage, FrontendMessage};
use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionRequestPayload, PermissionResponsePayload,
};
use allthecodes_types::message::{ContentBlock, StreamEvent, ToolResultContent};
use allthecodes_types::sdk::{SdkMessage, SdkStreamEvent};

use crate::state::{SessionOwner, WebState};

/// Query parameters for the IPC WebSocket endpoint.
#[derive(Deserialize, Default)]
pub struct IpcWsParams {
    pub session_id: Option<String>,
}

fn parse_legacy_frontend_text(text: &str) -> Result<FrontendMessage, BackendMessage> {
    serde_json::from_str::<FrontendMessage>(text).map_err(|error| BackendMessage::Error {
        message: format!("Invalid FrontendMessage: {error}"),
        recoverable: true,
    })
}

/// GET /api/ipc/ws — Upgrade to WebSocket IPC bridge.
pub async fn ipc_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    Query(params): Query<IpcWsParams>,
) -> axum::response::Response {
    info!(
        session_id = ?params.session_id,
        "GET /api/ipc/ws — WebSocket upgrade"
    );

    if state.is_streaming.load(std::sync::atomic::Ordering::SeqCst) {
        return (
            axum::http::StatusCode::CONFLICT,
            "A query is already in progress",
        )
            .into_response();
    }

    let engine = state.engine();
    let active_session_id = params
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| engine.current_session_id().to_string());

    if let Err(owner) = state.try_claim(SessionOwner::IpcWs, active_session_id) {
        return (
            axum::http::StatusCode::CONFLICT,
            format!(
                "Session is currently owned by {:?}{}",
                owner.owner,
                owner
                    .session_id
                    .as_deref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ),
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_ipc_socket(socket, state, params.session_id))
        .into_response()
}

/// Drive the IPC WebSocket for the lifetime of the connection.
async fn handle_ipc_socket(socket: WebSocket, state: WebState, session_id: Option<String>) {
    let engine = state.engine();
    let actual_session_id = session_id
        .clone()
        .unwrap_or_else(|| engine.current_session_id().to_string());

    // Split WebSocket into sender/receiver
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let (ipc_runtime, mut outbound_rx) = IpcRuntime::new(actual_session_id.clone(), 256);
    // ── Install permission callback ───────────────────────────────
    let runtime_permissions = ipc_runtime.clone();
    let permission_cb: allthecodes_types::callbacks::PermissionCallback =
        Arc::new(move |req: PermissionRequestPayload| {
            let runtime = runtime_permissions.clone();
            Box::pin(async move {
                runtime
                    .request_permission(req)
                    .await
                    .unwrap_or_else(|_| PermissionResponsePayload::deny())
            })
        });
    engine.set_permission_callback(permission_cb);

    // ── Install ask_user callback ─────────────────────────────────
    let runtime_questions = ipc_runtime.clone();
    let ask_user_cb: allthecodes_types::callbacks::AskUserCallback =
        Arc::new(move |req: AskUserRequestPayload| {
            let runtime = runtime_questions.clone();
            Box::pin(async move { runtime.request_question(req).await.unwrap_or_default() })
        });
    engine.set_ask_user_callback(ask_user_cb);

    // ── Send Ready message ────────────────────────────────────────
    let app_state = engine.app_state();
    let ready = BackendMessage::Ready {
        session_id: actual_session_id.clone(),
        model: app_state.main_loop_model.clone(),
        cwd: engine.cwd().to_string(),
        permission_mode: app_state.tool_permission_context.mode.as_str().to_string(),
        available_models: app_state.settings.available_models.clone(),
        plan_workflow: None,
        editor_mode: None,
        view_mode: None,
        keybindings: None,
    };
    let _ = ipc_runtime.send_backend(ready).await;

    // ── Task: forward outbound messages to WebSocket ──────────────
    let outbound_handle = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let json = serde_json::to_string(&message).unwrap_or_default();
            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // ── Task: handle incoming FrontendMessages ────────────────────
    let engine_for_tasks = state.clone();
    let runtime_inner = ipc_runtime.clone();
    let sid = actual_session_id.clone();

    let inbound_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let frontend: FrontendMessage = match parse_legacy_frontend_text(&text) {
                                Ok(msg) => msg,
                                Err(err) => {
                                    let _ = runtime_inner.send_backend(err).await;
                                    continue;
                                }
                            };

                            if !handle_frontend_message(
                                frontend,
                                &engine_for_tasks,
                                &runtime_inner,
                                &sid,
                            ).await {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("IPC WebSocket closed by client");
                            break;
                        }
                        Some(Ok(Message::Ping(_))) => {
                            // handled automatically by axum
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Binary(_))) => {}
                        Some(Err(e)) => {
                            warn!("IPC WebSocket error: {e}");
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        // Cleanup: abort any running query
        engine_for_tasks.engine().abort();
        engine_for_tasks
            .is_streaming
            .store(false, std::sync::atomic::Ordering::SeqCst);
    });

    // Wait for either task to complete (connection closed)
    tokio::select! {
        _ = outbound_handle => {}
        _ = inbound_handle => {}
    }

    // ── Cleanup ───────────────────────────────────────────────────
    let cleanup = ipc_runtime.cleanup_pending();
    if cleanup.permissions > 0 || cleanup.questions > 0 {
        info!(
            permissions = cleanup.permissions,
            questions = cleanup.questions,
            "IPC WebSocket cleaned up pending interactions"
        );
    }
    engine.clear_permission_callback();
    engine.clear_ask_user_callback();
    state
        .is_streaming
        .store(false, std::sync::atomic::Ordering::SeqCst);
    state.release_owner(SessionOwner::IpcWs);
    info!("IPC WebSocket connection closed");
}

/// Handle a single FrontendMessage, returning false if the loop should exit.
async fn handle_frontend_message(
    msg: FrontendMessage,
    state: &WebState,
    runtime: &IpcRuntime,
    session_id: &str,
) -> bool {
    match msg {
        FrontendMessage::SubmitPrompt { text, id } => {
            submit_prompt_via_ipc(text, id, state, runtime, session_id).await;
            true
        }
        FrontendMessage::AbortQuery => {
            state.engine().abort();
            state
                .is_streaming
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let msg = BackendMessage::SystemInfo {
                text: "Query aborted".to_string(),
                level: "info".to_string(),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        FrontendMessage::PermissionResponse { .. } => {
            runtime.resolve_legacy_client_response(&msg);
            true
        }
        FrontendMessage::QuestionResponse { .. } => {
            runtime.resolve_legacy_client_response(&msg);
            true
        }
        FrontendMessage::SlashCommand { raw } => {
            let result = execute_slash_command(raw, state, runtime, session_id).await;
            result
        }
        FrontendMessage::Resize { cols: _, rows: _ } => {
            // Not applicable for IPC — handled by TUI WS
            true
        }
        FrontendMessage::Quit => {
            info!("IPC WebSocket quit received");
            false
        }
        FrontendMessage::QuerySubsystemStatus => {
            // TODO: implement subsystem status query
            let msg = BackendMessage::SystemInfo {
                text: "Subsystem status query not yet implemented".to_string(),
                level: "info".to_string(),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        FrontendMessage::RequestCompletions {
            input: _,
            cursor_pos: _,
            request_id,
        } => {
            // TODO: implement completions
            let msg = BackendMessage::Completions {
                items: vec![],
                request_id,
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        // Agent/team commands — forward to engine
        FrontendMessage::AgentCommand { command } => {
            // The engine's command dispatcher handles this
            let msg = BackendMessage::SystemInfo {
                text: format!("Agent command: {command:?}"),
                level: "info".to_string(),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        FrontendMessage::TeamCommand { command } => {
            let msg = BackendMessage::SystemInfo {
                text: format!("Team command: {command:?}"),
                level: "info".to_string(),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        // Subsystem commands — TODO
        FrontendMessage::LspCommand { .. }
        | FrontendMessage::McpCommand { .. }
        | FrontendMessage::PluginCommand { .. }
        | FrontendMessage::SkillCommand { .. }
        | FrontendMessage::IdeCommand { .. }
        | FrontendMessage::AgentSettingsCommand { .. } => {
            let msg = BackendMessage::SystemInfo {
                text: "Subsystem commands not yet implemented via IPC WebSocket".to_string(),
                level: "info".to_string(),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        // File search — TODO
        FrontendMessage::SearchFiles { request_id, .. } => {
            let msg = BackendMessage::FileSearchResult {
                request_id,
                matches: vec![],
                truncated: false,
                error: Some("File search not yet implemented via IPC WebSocket".to_string()),
            };
            let _ = runtime.send_backend(msg).await;
            true
        }
        // Completions acceptance — no-op for now
        FrontendMessage::AcceptCompletion { .. }
        | FrontendMessage::InstallRecommendedPlugin { .. }
        | FrontendMessage::RefreshPluginTelemetry
        | FrontendMessage::RequestLspRecommendations { .. } => true,
    }
}

/// Execute a submit_prompt by calling engine.submit_message() and streaming
/// the SdkMessage stream as BackendMessage events.
async fn submit_prompt_via_ipc(
    text: String,
    _id: String,
    state: &WebState,
    runtime: &IpcRuntime,
    _session_id: &str,
) {
    state
        .is_streaming
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let engine = state.engine();
    let stream = engine.submit_message(&text, allthecodes_engine::types::config::QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    use futures::StreamExt;

    let mut draft_id: Option<String> = None;

    loop {
        match stream.next().await {
            Some(sdk_msg) => {
                let backend_msgs = sdk_to_backend_messages(sdk_msg, &mut draft_id);
                for backend_msg in backend_msgs {
                    if runtime.send_backend(backend_msg).await.is_err() {
                        state
                            .is_streaming
                            .store(false, std::sync::atomic::Ordering::SeqCst);
                        state.release_owner(SessionOwner::IpcWs);
                        return;
                    }
                }
            }
            None => break,
        }
    }

    state
        .is_streaming
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Convert an SdkMessage to zero or more BackendMessage events.
fn sdk_to_backend_messages(msg: SdkMessage, draft_id: &mut Option<String>) -> Vec<BackendMessage> {
    match msg {
        SdkMessage::StreamEvent(ev) => stream_event_to_backend(ev, draft_id),
        SdkMessage::Assistant(assistant) => {
            *draft_id = None;
            vec![BackendMessage::AssistantMessage {
                id: assistant.message.uuid.to_string(),
                content: serde_json::to_value(&assistant.message.content).unwrap_or_default(),
                cost_usd: assistant.message.cost_usd,
            }]
        }
        SdkMessage::Result(result) => {
            let mut msgs: Vec<BackendMessage> = Vec::new();
            if result.is_error || !result.errors.is_empty() {
                for err in &result.errors {
                    msgs.push(BackendMessage::Error {
                        message: err.clone(),
                        recoverable: false,
                    });
                }
            }
            msgs.push(BackendMessage::UsageUpdate {
                input_tokens: result.usage.total_input_tokens,
                output_tokens: result.usage.total_output_tokens,
                cost_usd: result.total_cost_usd,
            });
            msgs
        }
        SdkMessage::ApiRetry(retry) => {
            vec![BackendMessage::SystemInfo {
                text: format!(
                    "API retry {} of {}: {}",
                    retry.attempt, retry.max_retries, retry.error
                ),
                level: "warning".to_string(),
            }]
        }
        SdkMessage::CompactBoundary(_) => {
            vec![BackendMessage::SystemInfo {
                text: "Context compacted".to_string(),
                level: "info".to_string(),
            }]
        }
        SdkMessage::ToolUseSummary(summary) => {
            vec![BackendMessage::SystemInfo {
                text: summary.summary.clone(),
                level: "info".to_string(),
            }]
        }
        SdkMessage::GoalUpdated(update) => {
            vec![BackendMessage::GoalUpdated {
                event: update.event,
                goal: update.goal,
            }]
        }
        SdkMessage::Tombstone(tombstone) => {
            *draft_id = None;
            vec![BackendMessage::Tombstone {
                message_id: tombstone.message.uuid.to_string(),
            }]
        }
        SdkMessage::SystemInit(_) | SdkMessage::UserReplay(_) => vec![],
    }
}

/// Convert a stream event to BackendMessage events.
fn stream_event_to_backend(
    ev: SdkStreamEvent,
    draft_id: &mut Option<String>,
) -> Vec<BackendMessage> {
    match ev.event {
        StreamEvent::ContentBlockStart {
            index: _,
            content_block,
        } => {
            let uuid = ev.uuid.to_string();
            match content_block {
                ContentBlock::Text { text } => {
                    *draft_id = Some(uuid.clone());
                    vec![
                        BackendMessage::StreamStart {
                            message_id: uuid.clone(),
                        },
                        BackendMessage::StreamDelta {
                            message_id: uuid,
                            text,
                        },
                    ]
                }
                ContentBlock::Thinking {
                    thinking,
                    signature: _,
                } => {
                    vec![BackendMessage::ThinkingDelta {
                        message_id: uuid,
                        thinking,
                    }]
                }
                ContentBlock::RedactedThinking { data: _ } => {
                    vec![BackendMessage::ThinkingDelta {
                        message_id: uuid,
                        thinking: "Redacted thinking".to_string(),
                    }]
                }
                ContentBlock::ToolUse { id, name, input } => {
                    vec![BackendMessage::ToolUse { id, name, input }]
                }
                ContentBlock::ServerToolUse { id, name, input } => {
                    vec![BackendMessage::ToolUse { id, name, input }]
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let output = tool_result_content_to_string(&content);
                    vec![BackendMessage::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        content_blocks: None,
                    }]
                }
                ContentBlock::ConnectorText {
                    connector_text,
                    signature: _,
                } => {
                    *draft_id = Some(uuid.clone());
                    vec![
                        BackendMessage::StreamStart {
                            message_id: uuid.clone(),
                        },
                        BackendMessage::StreamDelta {
                            message_id: uuid,
                            text: connector_text,
                        },
                    ]
                }
                ContentBlock::Image { source: _ } => vec![],
            }
        }
        StreamEvent::ContentBlockDelta { index: _, delta } => {
            let mut msgs = Vec::new();
            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                let id = draft_id.clone().unwrap_or_else(|| ev.uuid.to_string());
                if draft_id.is_none() {
                    *draft_id = Some(id.clone());
                    msgs.push(BackendMessage::StreamStart {
                        message_id: id.clone(),
                    });
                }
                msgs.push(BackendMessage::StreamDelta {
                    message_id: id,
                    text: text.to_string(),
                });
            }
            if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                let id = ev.uuid.to_string();
                msgs.push(BackendMessage::ThinkingDelta {
                    message_id: id,
                    thinking: thinking.to_string(),
                });
            }
            if delta.get("signature").and_then(|v| v.as_str()).is_some() {
                // Signature-only deltas (associated with thinking) — ignore for now
                if msgs.is_empty() {
                    let id = ev.uuid.to_string();
                    msgs.push(BackendMessage::ThinkingDelta {
                        message_id: id,
                        thinking: String::new(),
                    });
                }
            }
            msgs
        }
        StreamEvent::ContentBlockStop { index: _ } => {
            if let Some(ref id) = *draft_id {
                vec![BackendMessage::StreamEnd {
                    message_id: id.clone(),
                }]
            } else {
                vec![]
            }
        }
        StreamEvent::MessageStart { usage: _ } => vec![],
        StreamEvent::MessageDelta { delta: _, usage: _ } => {
            // Could emit usage update here
            vec![]
        }
        StreamEvent::MessageStop => vec![],
    }
}

/// Convert ToolResultContent to a plain string.
fn tool_result_content_to_string(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(t) => t.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Execute a slash command and send the result as BackendMessage(s).
async fn execute_slash_command(
    raw: String,
    _state: &WebState,
    runtime: &IpcRuntime,
    _session_id: &str,
) -> bool {
    let trimmed = raw.trim().trim_start_matches('/');
    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    let cmd_name = parts.first().unwrap_or(&"");
    let args = parts.get(1).unwrap_or(&"").to_string();

    // Fetch commands from the command provider (installed at startup)
    // We route through the engine's command_dispatcher or use the same
    // lookup as handlers.rs
    let msg = BackendMessage::SystemInfo {
        text: format!("Slash command /{cmd_name} {args}"),
        level: "info".to_string(),
    };
    let _ = runtime.send_backend(msg).await;

    // Note: Full slash command execution via IPC is a future enhancement.
    // For now, clients should use the POST /api/command REST endpoint.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ipc_parse_error_stays_bare_backend_message() {
        let error = parse_legacy_frontend_text("{bad json").unwrap_err();

        let encoded = serde_json::to_string(&error).unwrap();
        assert!(matches!(
            error,
            BackendMessage::Error {
                recoverable: true,
                ..
            }
        ));
        assert!(encoded.starts_with(r#"{"type":"error""#));
        assert!(!encoded.contains("payload"));
        assert!(!encoded.contains("kind"));
    }

    #[test]
    fn legacy_ipc_ready_message_stays_bare_backend_message() {
        let ready = BackendMessage::Ready {
            session_id: "session-1".to_string(),
            model: "test-model".to_string(),
            cwd: "/repo".to_string(),
            permission_mode: "default".to_string(),
            available_models: vec!["test-model".to_string()],
            plan_workflow: None,
            editor_mode: None,
            view_mode: None,
            keybindings: None,
        };

        let encoded = serde_json::to_string(&ready).unwrap();

        assert!(encoded.starts_with(r#"{"type":"ready""#));
        assert!(encoded.contains(r#""session_id":"session-1""#));
        assert!(!encoded.contains("payload"));
        assert!(!encoded.contains("kind"));
    }

    #[test]
    fn legacy_ipc_submit_prompt_stays_bare_frontend_message() {
        let parsed =
            parse_legacy_frontend_text(r#"{"type":"submit_prompt","text":"hello","id":"ui-1"}"#)
                .unwrap();

        assert!(matches!(
            parsed,
            FrontendMessage::SubmitPrompt { text, id } if text == "hello" && id == "ui-1"
        ));
    }

    #[test]
    fn legacy_ipc_permission_response_stays_bare_frontend_message() {
        let parsed = parse_legacy_frontend_text(
            r#"{"type":"permission_response","tool_use_id":"tool-1","decision":"allow","feedback":"ok"}"#,
        )
        .unwrap();

        assert!(matches!(
            parsed,
            FrontendMessage::PermissionResponse {
                tool_use_id,
                decision,
                feedback: Some(feedback),
                ..
            } if tool_use_id == "tool-1" && decision == "allow" && feedback == "ok"
        ));
    }

    #[test]
    fn legacy_ipc_question_response_stays_bare_frontend_message() {
        let parsed =
            parse_legacy_frontend_text(r#"{"type":"question_response","id":"q-1","text":"yes"}"#)
                .unwrap();

        assert!(matches!(
            parsed,
            FrontendMessage::QuestionResponse { id, text, .. } if id == "q-1" && text == "yes"
        ));
    }
}
