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

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tracing::{info, warn};

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_ipc::adapters::extract_tool_result_output;
use allthecodes_ipc_protocol::{BackendMessage, FrontendMessage};
use allthecodes_server::{
    ConnectionClosedReason, ConnectionId, ConnectionOrigin, EventSeq, OriginRejection,
    TransportEvent, TransportKind,
};
use allthecodes_tool_display::ToolClassifier;
use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionRequestPayload, PermissionResponsePayload,
};
use allthecodes_types::message::{ContentBlock, StreamEvent, ToolResultContent};
use allthecodes_types::sdk::{SdkMessage, SdkStreamEvent, SdkUserReplay};
use allthecodes_types::tool_operation::{OperationStatus, ToolOperation};

use crate::ipc_streams::{ipc_seq_marker, IpcSessionHub};
use crate::state::WebState;

/// Query parameters for the IPC WebSocket endpoint.
#[derive(Deserialize, Default)]
pub struct IpcWsParams {
    pub session_id: Option<String>,
    pub after_seq: Option<EventSeq>,
}

fn parse_legacy_frontend_text(text: &str) -> Result<FrontendMessage, Box<BackendMessage>> {
    serde_json::from_str::<FrontendMessage>(text).map_err(|error| {
        Box::new(BackendMessage::Error {
            message: format!("Invalid FrontendMessage: {error}"),
            recoverable: true,
        })
    })
}

/// GET /api/ipc/ws — Upgrade to WebSocket IPC bridge.
pub async fn ipc_ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<WebState>,
    Query(params): Query<IpcWsParams>,
) -> axum::response::Response {
    info!(
        session_id = ?params.session_id,
        "GET /api/ipc/ws — WebSocket upgrade"
    );

    let origin = match websocket_origin_from_headers(&headers) {
        Ok(origin) => origin,
        Err(rejection) => {
            warn!(origin = %rejection.origin, "rejecting IPC WebSocket origin");
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
    };

    let engine = params
        .session_id
        .as_deref()
        .and_then(|session_id| state.engine_for_session(session_id))
        .unwrap_or_else(|| state.engine());
    let _active_session_id = params
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| engine.current_session_id().to_string());

    let connection_id = ConnectionId::next();
    ws.on_upgrade(move |socket| {
        handle_ipc_socket(
            socket,
            state,
            engine,
            params.session_id,
            params.after_seq,
            connection_id,
            origin,
        )
    })
    .into_response()
}

/// Drive the IPC WebSocket for the lifetime of the connection.
async fn handle_ipc_socket(
    socket: WebSocket,
    state: WebState,
    engine: Arc<QueryEngine>,
    session_id: Option<String>,
    after_seq: Option<EventSeq>,
    connection_id: ConnectionId,
    origin: ConnectionOrigin,
) {
    let actual_session_id = session_id
        .clone()
        .unwrap_or_else(|| engine.current_session_id().to_string());
    let hub = state.ipc_session_hub(&actual_session_id);
    if let Some(mut bridge_rx) = hub.take_bridge_receiver() {
        let bridge_hub = hub.clone();
        tokio::spawn(async move {
            while let Some(message) = bridge_rx.recv().await {
                bridge_hub.publish(message);
            }
        });
    }

    log_transport_event(TransportEvent::<FrontendMessage>::ConnectionOpened {
        connection_id: connection_id.clone(),
        kind: TransportKind::IpcWebSocket,
        origin,
    });

    // Split WebSocket into sender/receiver
    let (mut ws_sender, mut ws_receiver) = socket.split();

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
    let mut outbound_rx = hub.register_connection(connection_id.clone());

    // ── Task: forward outbound messages to WebSocket ──────────────
    let writer_session_id = actual_session_id.clone();
    let writer_hub = hub.clone();
    let writer_connection_id = connection_id.clone();
    let mut outbound_handle = tokio::spawn(async move {
        if send_backend_ws_message(&mut ws_sender, &ready)
            .await
            .is_err()
        {
            return;
        }

        let replay = writer_hub.replay_after(after_seq);
        let replay_high_watermark = replay.high_watermark;
        if let Some(lagged) =
            IpcSessionHub::replay_lagged_message(&replay.status, writer_hub.latest_seq())
        {
            if send_backend_ws_message(&mut ws_sender, &lagged)
                .await
                .is_err()
            {
                return;
            }
        }
        if replay.events.is_empty() {
            let marker = ipc_seq_marker(&writer_session_id, writer_hub.latest_seq());
            if send_backend_ws_message(&mut ws_sender, &marker)
                .await
                .is_err()
            {
                return;
            }
        }
        for event in replay.events {
            if send_backend_ws_message(&mut ws_sender, &event.message)
                .await
                .is_err()
            {
                return;
            }
            let marker = ipc_seq_marker(&writer_session_id, event.seq);
            if send_backend_ws_message(&mut ws_sender, &marker)
                .await
                .is_err()
            {
                return;
            }
        }

        while let Some(event) = outbound_rx.recv().await {
            if event.seq <= replay_high_watermark {
                continue;
            }
            if send_backend_ws_message(&mut ws_sender, &event.message)
                .await
                .is_err()
            {
                break;
            }
            let marker = ipc_seq_marker(&writer_session_id, event.seq);
            if send_backend_ws_message(&mut ws_sender, &marker)
                .await
                .is_err()
            {
                break;
            }
        }

        if let Some(skipped) = writer_hub.take_disconnect_lag(&writer_connection_id) {
            let lagged = IpcSessionHub::lagged_message(skipped, None);
            let _ = send_backend_ws_message(&mut ws_sender, &lagged).await;
        }
    });

    // ── Task: handle incoming FrontendMessages ────────────────────
    let state_for_tasks = state.clone();
    let engine_for_tasks = engine.clone();
    let hub_inner = hub.clone();
    let sid = actual_session_id.clone();
    let inbound_connection_id = connection_id.clone();

    let mut inbound_handle = tokio::spawn(async move {
        let close_reason = loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let frontend = match ipc_text_to_transport_event(&inbound_connection_id, &text) {
                                Ok(TransportEvent::IncomingMessage { message, .. }) => message,
                                Err(err) => {
                                    hub_inner.send_control_to(&inbound_connection_id, *err);
                                    continue;
                                }
                                Ok(_) => continue,
                            };

                            if !handle_frontend_message(
                                frontend,
                                &inbound_connection_id,
                                &state_for_tasks,
                                &engine_for_tasks,
                                &hub_inner,
                                &sid,
                            ).await {
                                break ConnectionClosedReason::ProtocolQuit;
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("IPC WebSocket closed by client");
                            break ConnectionClosedReason::ClientClosed;
                        }
                        Some(Ok(Message::Ping(_))) => {
                            // handled automatically by axum
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Binary(_))) => {}
                        Some(Err(e)) => {
                            warn!("IPC WebSocket error: {e}");
                            break ConnectionClosedReason::TransportError(e.to_string());
                        }
                        None => {
                            break ConnectionClosedReason::ClientClosed;
                        }
                    }
                }
            }
        };

        log_transport_event(ipc_close_transport_event(
            &inbound_connection_id,
            close_reason,
        ));
    });

    // Wait for either task to complete (connection closed)
    tokio::select! {
        _ = &mut outbound_handle => {}
        _ = &mut inbound_handle => {}
    }
    outbound_handle.abort();
    inbound_handle.abort();

    // ── Cleanup ───────────────────────────────────────────────────
    cleanup_ipc_connection_owner(&hub, &connection_id, &state, &engine, &actual_session_id);
    hub.unregister_connection(&connection_id);
    info!("IPC WebSocket connection closed");
}

async fn send_backend_ws_message(
    ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    message: &BackendMessage,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(message).unwrap_or_default();
    ws_sender.send(Message::Text(json.into())).await
}

fn cleanup_ipc_connection_owner(
    hub: &IpcSessionHub,
    connection_id: &ConnectionId,
    state: &WebState,
    engine: &Arc<QueryEngine>,
    session_id: &str,
) {
    if !hub.release_turn_if_owner(connection_id) {
        return;
    }

    engine.abort();
    let cleanup = hub.runtime().cleanup_pending();
    if cleanup.permissions > 0 || cleanup.questions > 0 || cleanup.server_requests > 0 {
        info!(
            permissions = cleanup.permissions,
            questions = cleanup.questions,
            server_requests = cleanup.server_requests,
            "IPC WebSocket cleaned up pending interactions"
        );
    }
    engine.clear_permission_callback();
    engine.clear_ask_user_callback();
    state.set_session_streaming(session_id, false);
}

fn websocket_origin_from_headers(headers: &HeaderMap) -> Result<ConnectionOrigin, OriginRejection> {
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    ConnectionOrigin::from_websocket_origin(origin)
}

fn ipc_text_to_transport_event(
    connection_id: &ConnectionId,
    text: &str,
) -> Result<TransportEvent<FrontendMessage>, Box<BackendMessage>> {
    parse_legacy_frontend_text(text).map(|message| TransportEvent::IncomingMessage {
        connection_id: connection_id.clone(),
        kind: TransportKind::IpcWebSocket,
        message,
    })
}

fn ipc_close_transport_event(
    connection_id: &ConnectionId,
    reason: ConnectionClosedReason,
) -> TransportEvent<FrontendMessage> {
    TransportEvent::ConnectionClosed {
        connection_id: connection_id.clone(),
        kind: TransportKind::IpcWebSocket,
        reason,
    }
}

fn log_transport_event<T: std::fmt::Debug>(event: TransportEvent<T>) {
    match event {
        TransportEvent::ConnectionOpened {
            connection_id,
            kind,
            origin,
        } => info!(
            connection_id = %connection_id,
            kind = ?kind,
            origin = ?origin,
            "transport connection opened"
        ),
        TransportEvent::IncomingMessage {
            connection_id,
            kind,
            message,
        } => info!(
            connection_id = %connection_id,
            kind = ?kind,
            message = ?message,
            "transport incoming message"
        ),
        TransportEvent::ConnectionClosed {
            connection_id,
            kind,
            reason,
        } => info!(
            connection_id = %connection_id,
            kind = ?kind,
            reason = ?reason,
            "transport connection closed"
        ),
    }
}

/// Handle a single FrontendMessage, returning false if the loop should exit.
async fn handle_frontend_message(
    msg: FrontendMessage,
    connection_id: &ConnectionId,
    state: &WebState,
    engine: &Arc<QueryEngine>,
    hub: &Arc<IpcSessionHub>,
    session_id: &str,
) -> bool {
    match msg {
        FrontendMessage::SubmitPrompt { text, id } => {
            submit_prompt_via_ipc(text, id, connection_id, state, engine, hub, session_id).await;
            true
        }
        FrontendMessage::AbortQuery => {
            engine.abort();
            state.set_session_streaming(session_id, false);
            hub.release_turn_if_owner(connection_id);
            let msg = BackendMessage::SystemInfo {
                text: "Query aborted".to_string(),
                level: "info".to_string(),
            };
            let _ = hub.runtime().send_backend(msg).await;
            true
        }
        FrontendMessage::PermissionResponse { .. } => {
            hub.runtime().resolve_legacy_client_response(&msg);
            true
        }
        FrontendMessage::QuestionResponse { .. } => {
            hub.runtime().resolve_legacy_client_response(&msg);
            true
        }
        FrontendMessage::SlashCommand { raw } => {
            let result = execute_slash_command(raw, state, hub, session_id).await;
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
            let _ = hub.runtime().send_backend(msg).await;
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
            let _ = hub.runtime().send_backend(msg).await;
            true
        }
        // Agent/team commands — forward to engine
        FrontendMessage::AgentCommand { command } => {
            // The engine's command dispatcher handles this
            let msg = BackendMessage::SystemInfo {
                text: format!("Agent command: {command:?}"),
                level: "info".to_string(),
            };
            let _ = hub.runtime().send_backend(msg).await;
            true
        }
        FrontendMessage::TeamCommand { command } => {
            let msg = BackendMessage::SystemInfo {
                text: format!("Team command: {command:?}"),
                level: "info".to_string(),
            };
            let _ = hub.runtime().send_backend(msg).await;
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
            let _ = hub.runtime().send_backend(msg).await;
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
            let _ = hub.runtime().send_backend(msg).await;
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
    connection_id: &ConnectionId,
    state: &WebState,
    engine: &Arc<QueryEngine>,
    hub: &Arc<IpcSessionHub>,
    _session_id: &str,
) {
    if !hub.claim_turn(connection_id) {
        hub.send_control_to(
            connection_id,
            BackendMessage::Error {
                message: "A query is already in progress".to_string(),
                recoverable: true,
            },
        );
        return;
    }

    let runtime_permissions = hub.runtime().clone();
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

    let runtime_questions = hub.runtime().clone();
    let ask_user_cb: allthecodes_types::callbacks::AskUserCallback =
        Arc::new(move |req: AskUserRequestPayload| {
            let runtime = runtime_questions.clone();
            Box::pin(async move { runtime.request_question(req).await.unwrap_or_default() })
        });
    engine.set_ask_user_callback(ask_user_cb);

    state.set_session_streaming(_session_id, true);

    let stream = engine.submit_message(&text, allthecodes_engine::types::config::QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    use futures::StreamExt;

    let mut draft_id: Option<String> = None;
    let mut tool_use_cache: ToolUseContextCache = HashMap::new();

    while let Some(sdk_msg) = stream.next().await {
        let backend_msgs = sdk_to_backend_messages(sdk_msg, &mut draft_id, &mut tool_use_cache);
        for backend_msg in backend_msgs {
            if hub.runtime().send_backend(backend_msg).await.is_err() {
                state.set_session_streaming(_session_id, false);
                hub.release_turn_if_owner(connection_id);
                engine.clear_permission_callback();
                engine.clear_ask_user_callback();
                return;
            }
        }
    }

    state.set_session_streaming(_session_id, false);
    hub.release_turn_if_owner(connection_id);
    engine.clear_permission_callback();
    engine.clear_ask_user_callback();
}

type ToolUseContextCache = HashMap<String, (String, serde_json::Value)>;

/// Convert an SdkMessage to zero or more BackendMessage events.
fn sdk_to_backend_messages(
    msg: SdkMessage,
    draft_id: &mut Option<String>,
    tool_use_cache: &mut ToolUseContextCache,
) -> Vec<BackendMessage> {
    match msg {
        SdkMessage::StreamEvent(ev) => stream_event_to_backend(ev, draft_id, tool_use_cache),
        SdkMessage::Assistant(assistant) => {
            *draft_id = None;
            cache_tool_uses_from_blocks(&assistant.message.content, tool_use_cache);
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
            tool_use_cache.clear();
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
        SdkMessage::UserReplay(replay) => user_replay_to_backend_messages(replay, tool_use_cache),
        SdkMessage::SystemInit(_) => vec![],
    }
}

/// Convert a stream event to BackendMessage events.
fn stream_event_to_backend(
    ev: SdkStreamEvent,
    draft_id: &mut Option<String>,
    tool_use_cache: &mut ToolUseContextCache,
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
                    let operation =
                        ToolClassifier::classify(&name, &input, OperationStatus::InProgress);
                    tool_use_cache.insert(id.clone(), (name.clone(), input.clone()));
                    vec![BackendMessage::ToolUse {
                        id,
                        name,
                        input,
                        operation: Some(operation),
                    }]
                }
                ContentBlock::ServerToolUse { id, name, input } => {
                    let operation =
                        ToolClassifier::classify(&name, &input, OperationStatus::InProgress);
                    tool_use_cache.insert(id.clone(), (name.clone(), input.clone()));
                    vec![BackendMessage::ToolUse {
                        id,
                        name,
                        input,
                        operation: Some(operation),
                    }]
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let (output, content_blocks) = tool_result_content_to_output(&content);
                    let operation =
                        tool_result_operation(tool_use_cache, &tool_use_id, &output, is_error);
                    vec![BackendMessage::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        content_blocks,
                        result_summary: operation
                            .as_ref()
                            .and_then(|operation| operation.result_summary.clone()),
                        operation,
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

fn cache_tool_uses_from_blocks(blocks: &[ContentBlock], tool_use_cache: &mut ToolUseContextCache) {
    for block in blocks {
        match block {
            ContentBlock::ToolUse { id, name, input }
            | ContentBlock::ServerToolUse { id, name, input } => {
                tool_use_cache.insert(id.clone(), (name.clone(), input.clone()));
            }
            _ => {}
        }
    }
}

fn user_replay_to_backend_messages(
    replay: SdkUserReplay,
    tool_use_cache: &mut ToolUseContextCache,
) -> Vec<BackendMessage> {
    let Some(blocks) = replay.content_blocks else {
        return vec![];
    };
    let replay_tool_preview = if blocks
        .iter()
        .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
        .count()
        == 1
    {
        replay.tool_use_result
    } else {
        None
    };

    blocks
        .into_iter()
        .filter_map(|block| {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                return None;
            };
            let (output, content_blocks) = tool_result_content_to_output(&content);
            let output = replay_tool_preview.clone().unwrap_or(output);
            let operation = tool_result_operation(tool_use_cache, &tool_use_id, &output, is_error);

            Some(BackendMessage::ToolResult {
                tool_use_id,
                output,
                is_error,
                content_blocks,
                result_summary: operation
                    .as_ref()
                    .and_then(|operation| operation.result_summary.clone()),
                operation,
            })
        })
        .collect()
}

fn tool_result_operation(
    tool_use_cache: &mut ToolUseContextCache,
    tool_use_id: &str,
    output: &str,
    is_error: bool,
) -> Option<ToolOperation> {
    let (tool_name, tool_input) = tool_use_cache.remove(tool_use_id)?;
    let status = if is_error {
        OperationStatus::Error
    } else {
        OperationStatus::Resolved
    };
    Some(ToolClassifier::classify_with_result(
        &tool_name,
        &tool_input,
        status,
        Some(output),
        is_error,
    ))
}

/// Convert ToolResultContent to a plain string plus optional structured blocks.
fn tool_result_content_to_output(
    content: &ToolResultContent,
) -> (
    String,
    Option<Vec<allthecodes_ipc_protocol::ToolResultContentInfo>>,
) {
    match content {
        ToolResultContent::Text(t) => (t.clone(), None),
        ToolResultContent::Blocks(blocks) => extract_tool_result_output(blocks),
    }
}

/// Execute a slash command and send the result as BackendMessage(s).
async fn execute_slash_command(
    raw: String,
    _state: &WebState,
    hub: &IpcSessionHub,
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
    let _ = hub.runtime().send_backend(msg).await;

    // Note: Full slash command execution via IPC is a future enhancement.
    // For now, clients should use the POST /api/command REST endpoint.
    true
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    fn test_connection_id() -> ConnectionId {
        ConnectionId::from_static("test-ipc")
    }

    #[test]
    fn origin_headers_allow_missing_and_loopback() {
        let headers = HeaderMap::new();
        assert_eq!(
            websocket_origin_from_headers(&headers).unwrap(),
            ConnectionOrigin::LocalNative
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ORIGIN,
            "http://localhost:17322".parse().unwrap(),
        );
        assert_eq!(
            websocket_origin_from_headers(&headers).unwrap(),
            ConnectionOrigin::browser("http://localhost:17322")
        );
    }

    #[test]
    fn origin_headers_reject_non_loopback() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ORIGIN,
            "https://example.com".parse().unwrap(),
        );

        assert!(websocket_origin_from_headers(&headers).is_err());
    }

    #[test]
    fn valid_text_frame_becomes_incoming_message_event() {
        let id = test_connection_id();
        let event = ipc_text_to_transport_event(&id, r#"{"type":"quit"}"#).unwrap();

        assert!(matches!(
            event,
            TransportEvent::IncomingMessage {
                connection_id,
                kind: TransportKind::IpcWebSocket,
                message: FrontendMessage::Quit,
            } if connection_id == id
        ));
    }

    #[test]
    fn invalid_text_frame_keeps_recoverable_error_behavior() {
        let err = ipc_text_to_transport_event(&test_connection_id(), "{bad json").unwrap_err();

        assert!(matches!(
            *err,
            BackendMessage::Error {
                recoverable: true,
                ..
            }
        ));
    }

    #[test]
    fn close_frame_becomes_connection_closed_event() {
        let id = test_connection_id();
        let event = ipc_close_transport_event(&id, ConnectionClosedReason::ClientClosed);

        assert!(matches!(
            event,
            TransportEvent::ConnectionClosed {
                connection_id,
                kind: TransportKind::IpcWebSocket,
                reason: ConnectionClosedReason::ClientClosed,
            } if connection_id == id
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ipc_parse_error_stays_bare_backend_message() {
        let error = parse_legacy_frontend_text("{bad json").unwrap_err();

        let encoded = serde_json::to_string(&error).unwrap();
        assert!(matches!(
            *error,
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

    #[test]
    fn stream_tool_events_include_operation_metadata() {
        let mut draft_id = None;
        let mut cache = ToolUseContextCache::new();

        let tool_use = stream_event_to_backend(
            SdkStreamEvent {
                event: StreamEvent::ContentBlockStart {
                    index: 0,
                    content_block: ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command": "cargo test"}),
                    },
                },
                session_id: "session-1".to_string(),
                uuid: uuid::Uuid::nil(),
            },
            &mut draft_id,
            &mut cache,
        );

        assert!(matches!(
            &tool_use[0],
            BackendMessage::ToolUse {
                operation: Some(operation),
                ..
            } if operation.kind == allthecodes_types::tool_operation::OperationKind::Execute
                && operation.subtype == Some(allthecodes_types::tool_operation::OperationSubtype::Test)
        ));

        let tool_result = stream_event_to_backend(
            SdkStreamEvent {
                event: StreamEvent::ContentBlockStart {
                    index: 1,
                    content_block: ContentBlock::ToolResult {
                        tool_use_id: "tool-1".to_string(),
                        content: ToolResultContent::Text(
                            r#"{"exit_code":0,"stdout":"ok\n","stderr":""}"#.to_string(),
                        ),
                        is_error: false,
                    },
                },
                session_id: "session-1".to_string(),
                uuid: uuid::Uuid::nil(),
            },
            &mut draft_id,
            &mut cache,
        );

        assert!(matches!(
            &tool_result[0],
            BackendMessage::ToolResult {
                result_summary: Some(summary),
                operation: Some(operation),
                ..
            } if summary.exit_code == Some(0)
                && operation.subtype == Some(allthecodes_types::tool_operation::OperationSubtype::Test)
        ));
    }

    #[test]
    fn user_replay_tool_result_uses_cached_tool_context() {
        let mut cache = ToolUseContextCache::new();
        cache.insert(
            "tool-1".to_string(),
            (
                "Read".to_string(),
                serde_json::json!({"file_path": "Cargo.toml"}),
            ),
        );

        let messages = user_replay_to_backend_messages(
            SdkUserReplay {
                content: "[tool result]".to_string(),
                session_id: "session-1".to_string(),
                uuid: uuid::Uuid::nil(),
                timestamp: 0,
                is_replay: true,
                is_synthetic: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
                content_blocks: Some(vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: ToolResultContent::Text("line 1\nline 2\n".to_string()),
                    is_error: false,
                }]),
            },
            &mut cache,
        );

        assert!(matches!(
            &messages[0],
            BackendMessage::ToolResult {
                result_summary: Some(summary),
                operation: Some(operation),
                ..
            } if operation.kind == allthecodes_types::tool_operation::OperationKind::Read
                && operation.target.as_deref() == Some("Cargo.toml")
                && summary.file_lines == Some(2)
        ));
    }
}
