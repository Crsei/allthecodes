//! `/api/rpc/ws` — JSON-RPC WebSocket adapter for dispatcher-backed API operations.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tracing::{info, warn};

use allthecodes_protocol::{ApiError, JsonRpcFrame, TransportRequestId};
use allthecodes_server::{
    ConnectionClosedReason, ConnectionId, ConnectionOrigin, OriginRejection, OutboundRouter,
    RouterSendError, SequencedEvent, TransportEvent, TransportKind,
};

use crate::api_dispatcher::{ApiConnectionId, ApiDispatcher, ApiRequestContext};
use crate::state::WebState;

/// GET /api/rpc/ws — Upgrade to the API JSON-RPC WebSocket transport.
pub async fn api_rpc_ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<WebState>,
) -> axum::response::Response {
    let origin = match websocket_origin_from_headers(&headers) {
        Ok(origin) => origin,
        Err(rejection) => {
            warn!(origin = %rejection.origin, "rejecting API JSON-RPC WebSocket origin");
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
    };
    let connection_id = ConnectionId::next();
    let api_connection_id = ApiConnectionId(format!("api-rpc-{}", connection_id.as_str()));

    info!(connection_id = %api_connection_id.0, "GET /api/rpc/ws — WebSocket upgrade");

    ws.on_upgrade(move |socket| {
        handle_api_rpc_socket(socket, state, connection_id, api_connection_id, origin)
    })
    .into_response()
}

async fn handle_api_rpc_socket(
    socket: WebSocket,
    state: WebState,
    connection_id: ConnectionId,
    api_connection_id: ApiConnectionId,
    origin: ConnectionOrigin,
) {
    log_transport_event(TransportEvent::<String>::ConnectionOpened {
        connection_id: connection_id.clone(),
        kind: TransportKind::ApiRpcWebSocket,
        origin,
    });
    let dispatcher = ApiDispatcher::new(state);
    let context = ApiRequestContext::json_rpc_websocket(api_connection_id.clone());
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let router = OutboundRouter::<String>::default();
    let mut outbound_rx = router.register(connection_id.clone());
    let (writer_closed_tx, mut writer_closed_rx) = tokio::sync::oneshot::channel::<()>();
    let writer_connection_id = connection_id.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(event) = outbound_rx.recv().await {
            if ws_sender
                .send(Message::Text(event.message.into()))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = writer_closed_tx.send(());
    });
    let mut next_seq = 1;

    let close_reason = loop {
        tokio::select! {
            message = ws_receiver.next() => {
                let Some(message) = message else {
                    break ConnectionClosedReason::ClientClosed;
                };
                match message {
                    Ok(message) => match handle_api_rpc_message(&dispatcher, &context, &connection_id, message).await {
                    ApiRpcSocketAction::Respond(response) => {
                        match encode_frame(response.as_ref()) {
                            Ok(text) => {
                                let event = SequencedEvent::new(next_seq, text);
                                next_seq += 1;
                                if let Err(error) = router.send_to(&connection_id, event) {
                                    break match error {
                                        RouterSendError::Full { .. } => ConnectionClosedReason::TransportError(
                                            "JSON-RPC WebSocket outbound queue is full".to_string(),
                                        ),
                                        RouterSendError::Closed { .. }
                                        | RouterSendError::UnknownConnection { .. } => {
                                            ConnectionClosedReason::TransportError(
                                                "JSON-RPC WebSocket writer closed".to_string(),
                                            )
                                        }
                                    };
                                }
                            }
                            Err(error) => {
                                warn!(connection_id = %api_connection_id.0, "failed to encode JSON-RPC response: {error}");
                                break ConnectionClosedReason::TransportError(error.to_string());
                            }
                        }
                    }
                    ApiRpcSocketAction::Close => {
                        break ConnectionClosedReason::ClientClosed;
                    }
                    ApiRpcSocketAction::Ignore => {}
                    },
                    Err(error) => {
                        warn!(connection_id = %api_connection_id.0, "API JSON-RPC WebSocket error: {error}");
                        break ConnectionClosedReason::TransportError(error.to_string());
                    }
                };
            }
            _ = &mut writer_closed_rx => {
                break ConnectionClosedReason::TransportError(
                    "JSON-RPC WebSocket writer closed".to_string(),
                );
            }
        }
    };

    router.unregister(&writer_connection_id);
    writer_handle.abort();
    log_transport_event(TransportEvent::<String>::ConnectionClosed {
        connection_id,
        kind: TransportKind::ApiRpcWebSocket,
        reason: close_reason,
    });
    info!(connection_id = %api_connection_id.0, "API JSON-RPC WebSocket closed");
}

enum ApiRpcSocketAction {
    Respond(Box<JsonRpcFrame>),
    Close,
    Ignore,
}

async fn handle_api_rpc_message(
    dispatcher: &ApiDispatcher,
    context: &ApiRequestContext,
    connection_id: &ConnectionId,
    message: Message,
) -> ApiRpcSocketAction {
    match message {
        Message::Text(text) => {
            let text = match api_rpc_text_to_transport_event(connection_id, &text) {
                TransportEvent::IncomingMessage { message, .. } => message,
                _ => String::new(),
            };
            ApiRpcSocketAction::Respond(Box::new(
                handle_api_rpc_text(dispatcher, context, &text).await,
            ))
        }
        Message::Close(_) => ApiRpcSocketAction::Close,
        Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => ApiRpcSocketAction::Ignore,
    }
}

fn websocket_origin_from_headers(headers: &HeaderMap) -> Result<ConnectionOrigin, OriginRejection> {
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    ConnectionOrigin::from_websocket_origin(origin)
}

fn api_rpc_text_to_transport_event(
    connection_id: &ConnectionId,
    text: &str,
) -> TransportEvent<String> {
    TransportEvent::IncomingMessage {
        connection_id: connection_id.clone(),
        kind: TransportKind::ApiRpcWebSocket,
        message: text.to_string(),
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
    use allthecodes_protocol::{v1, ClientRequest, ClientResponse, NoParams};

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

    fn test_connection_id() -> ConnectionId {
        ConnectionId::from_static("test-connection")
    }

    #[tokio::test]
    async fn valid_capabilities_request_returns_response_frame() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let request = JsonRpcFrame::request(7, ClientRequest::Capabilities(NoParams {}));
        let text = serde_json::to_string(&request).expect("request should serialize");

        let response = handle_api_rpc_text(&dispatcher, &test_context(), &text).await;

        match response {
            JsonRpcFrame::Response { id, response, .. }
                if matches!(response.as_ref(), ClientResponse::Capabilities(_)) =>
            {
                let ClientResponse::Capabilities(body) = *response else {
                    unreachable!();
                };
                assert_eq!(id, 7);
                assert_eq!(body.capabilities.get("sessions"), Some(&true));
            }
            other => panic!("unexpected response frame: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsupported_request_returns_error_frame() {
        let dispatcher = ApiDispatcher::new(make_web_state());
        let request = JsonRpcFrame::request(
            9,
            ClientRequest::TerminalSessionsCreate(v1::terminal::TerminalCreateRequest {
                profile: "shell".to_string(),
                cwd: None,
                label: None,
                command: None,
                session_id: None,
                persist: None,
                initial_size: None,
            }),
        );
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
        let action = handle_api_rpc_message(
            &dispatcher,
            &test_context(),
            &test_connection_id(),
            Message::Close(None),
        )
        .await;

        assert!(matches!(action, ApiRpcSocketAction::Close));
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
            "http://127.0.0.1:17322".parse().unwrap(),
        );
        assert_eq!(
            websocket_origin_from_headers(&headers).unwrap(),
            ConnectionOrigin::browser("http://127.0.0.1:17322")
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
    fn text_frame_is_wrapped_as_transport_event() {
        let id = test_connection_id();
        let event = api_rpc_text_to_transport_event(&id, r#"{"jsonrpc":"2.0","id":1}"#);

        assert!(matches!(
            event,
            TransportEvent::IncomingMessage {
                connection_id,
                kind: TransportKind::ApiRpcWebSocket,
                message,
            } if connection_id == id && message.contains("\"jsonrpc\"")
        ));
    }
}
