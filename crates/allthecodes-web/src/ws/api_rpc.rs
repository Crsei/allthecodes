//! `/api/rpc/ws` — JSON-RPC WebSocket adapter for dispatcher-backed API operations.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tracing::{info, warn};

use allthecodes_protocol::{ApiError, JsonRpcFrame, TransportRequestId};

use crate::api_dispatcher::{ApiConnectionId, ApiDispatcher, ApiRequestContext};
use crate::state::WebState;

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// GET /api/rpc/ws — Upgrade to the API JSON-RPC WebSocket transport.
pub async fn api_rpc_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
) -> axum::response::Response {
    let connection_id = ApiConnectionId(format!(
        "api-rpc-{}",
        NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
    ));

    info!(connection_id = %connection_id.0, "GET /api/rpc/ws — WebSocket upgrade");

    ws.on_upgrade(move |socket| handle_api_rpc_socket(socket, state, connection_id))
        .into_response()
}

async fn handle_api_rpc_socket(socket: WebSocket, state: WebState, connection_id: ApiConnectionId) {
    let dispatcher = ApiDispatcher::new(state);
    let context = ApiRequestContext::json_rpc_websocket(connection_id.clone());
    let (mut ws_sender, mut ws_receiver) = socket.split();

    while let Some(message) = ws_receiver.next().await {
        match message {
            Ok(message) => match handle_api_rpc_message(&dispatcher, &context, message).await {
                ApiRpcSocketAction::Respond(response) => match encode_frame(&response) {
                    Ok(text) => {
                        if ws_sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        warn!(connection_id = %connection_id.0, "failed to encode JSON-RPC response: {error}");
                        break;
                    }
                },
                ApiRpcSocketAction::Close => break,
                ApiRpcSocketAction::Ignore => {}
            },
            Err(error) => {
                warn!(connection_id = %connection_id.0, "API JSON-RPC WebSocket error: {error}");
                break;
            }
        }
    }

    info!(connection_id = %connection_id.0, "API JSON-RPC WebSocket closed");
}

enum ApiRpcSocketAction {
    Respond(JsonRpcFrame),
    Close,
    Ignore,
}

async fn handle_api_rpc_message(
    dispatcher: &ApiDispatcher,
    context: &ApiRequestContext,
    message: Message,
) -> ApiRpcSocketAction {
    match message {
        Message::Text(text) => {
            ApiRpcSocketAction::Respond(handle_api_rpc_text(dispatcher, context, &text).await)
        }
        Message::Close(_) => ApiRpcSocketAction::Close,
        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => ApiRpcSocketAction::Ignore,
    }
}

async fn handle_api_rpc_text(
    dispatcher: &ApiDispatcher,
    context: &ApiRequestContext,
    text: &str,
) -> JsonRpcFrame {
    match serde_json::from_str::<JsonRpcFrame>(text) {
        Ok(JsonRpcFrame::Request { id, request, .. }) => {
            match dispatcher.dispatch(context.clone(), request).await {
                Ok(response) => JsonRpcFrame::response(id, response),
                Err(error) => JsonRpcFrame::error(id, error),
            }
        }
        Ok(frame) => JsonRpcFrame::error(
            frame_id(&frame).unwrap_or(0),
            ApiError::Validation {
                field: "type".to_string(),
                message: "protocol error: client frames must be JSON-RPC request frames"
                    .to_string(),
            },
        ),
        Err(error) => JsonRpcFrame::error(
            raw_frame_id(text).unwrap_or(0),
            ApiError::Validation {
                field: "frame".to_string(),
                message: format!("invalid JSON-RPC frame: {error}"),
            },
        ),
    }
}

fn encode_frame(frame: &JsonRpcFrame) -> Result<String, serde_json::Error> {
    serde_json::to_string(frame)
}

fn frame_id(frame: &JsonRpcFrame) -> Option<TransportRequestId> {
    match frame {
        JsonRpcFrame::Request { id, .. }
        | JsonRpcFrame::Response { id, .. }
        | JsonRpcFrame::Error { id, .. } => Some(*id),
        JsonRpcFrame::Notification { .. } => None,
    }
}

fn raw_frame_id(text: &str) -> Option<TransportRequestId> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_protocol::{ClientRequest, ClientResponse, NoParams};

    use super::*;

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

    fn test_context() -> ApiRequestContext {
        ApiRequestContext::json_rpc_websocket(ApiConnectionId("test-rpc".to_string()))
    }

    #[tokio::test]
    async fn valid_capabilities_request_returns_response_frame() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let request = JsonRpcFrame::request(7, ClientRequest::Capabilities(NoParams {}));
        let text = serde_json::to_string(&request).expect("request should serialize");

        let response = handle_api_rpc_text(&dispatcher, &test_context(), &text).await;

        match response {
            JsonRpcFrame::Response {
                id,
                response: ClientResponse::Capabilities(body),
                ..
            } => {
                assert_eq!(id, 7);
                assert_eq!(body.capabilities.get("sessions"), Some(&true));
            }
            other => panic!("unexpected response frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsupported_request_returns_error_frame() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let request = JsonRpcFrame::request(9, ClientRequest::TerminalSessionsCreate(NoParams {}));
        let text = serde_json::to_string(&request).expect("request should serialize");

        let response = handle_api_rpc_text(&dispatcher, &test_context(), &text).await;

        match response {
            JsonRpcFrame::Error { id, error, .. } => {
                assert_eq!(id, 9);
                assert_eq!(error.code, "capability_not_implemented");
            }
            other => panic!("unexpected response frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_json_returns_validation_error_with_fallback_id() {
        let dispatcher = ApiDispatcher::new(make_web_state());

        let response = handle_api_rpc_text(&dispatcher, &test_context(), "{").await;

        match response {
            JsonRpcFrame::Error { id, error, .. } => {
                assert_eq!(id, 0);
                assert_eq!(error.code, "validation");
                assert_eq!(error.details["field"], "frame");
            }
            other => panic!("unexpected response frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_request_frame_returns_protocol_validation_error() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let frame = JsonRpcFrame::response(
            11,
            ClientResponse::SessionArchive(allthecodes_protocol::v1::SessionArchiveResponse {
                id: "session-1".to_string(),
                archived: true,
            }),
        );
        let text = serde_json::to_string(&frame).expect("frame should serialize");

        let response = handle_api_rpc_text(&dispatcher, &test_context(), &text).await;

        match response {
            JsonRpcFrame::Error { id, error, .. } => {
                assert_eq!(id, 11);
                assert_eq!(error.code, "validation");
                assert_eq!(error.details["field"], "type");
            }
            other => panic!("unexpected response frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn close_frame_terminates_without_response() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let action =
            handle_api_rpc_message(&dispatcher, &test_context(), Message::Close(None)).await;

        assert!(matches!(action, ApiRpcSocketAction::Close));
    }
}
