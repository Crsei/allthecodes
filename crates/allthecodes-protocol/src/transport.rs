//! Protocol-level transport contracts shared by REST, WebSocket, and IPC adapters.

use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ApiError, ApiErrorBody, ClientRequest, ClientResponse, ServerNotification};

pub type TransportRequestId = u64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JsonRpcFrame {
    Request {
        jsonrpc: JsonRpcVersion,
        id: TransportRequestId,
        request: ClientRequest,
    },
    Response {
        jsonrpc: JsonRpcVersion,
        id: TransportRequestId,
        response: ClientResponse,
    },
    Error {
        jsonrpc: JsonRpcVersion,
        id: TransportRequestId,
        error: ApiErrorBody,
    },
    Notification {
        jsonrpc: JsonRpcVersion,
        notification: ServerNotification,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum JsonRpcVersion {
    #[serde(rename = "2.0")]
    #[default]
    V2,
}

impl JsonRpcFrame {
    pub fn request(id: TransportRequestId, request: ClientRequest) -> Self {
        Self::Request {
            jsonrpc: JsonRpcVersion::V2,
            id,
            request,
        }
    }

    pub fn response(id: TransportRequestId, response: ClientResponse) -> Self {
        Self::Response {
            jsonrpc: JsonRpcVersion::V2,
            id,
            response,
        }
    }

    pub fn error(id: TransportRequestId, error: ApiError) -> Self {
        Self::Error {
            jsonrpc: JsonRpcVersion::V2,
            id,
            error: error.into_body(),
        }
    }

    pub fn notification(notification: ServerNotification) -> Self {
        Self::Notification {
            jsonrpc: JsonRpcVersion::V2,
            notification,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransportError {
    #[error("transport connection is closed")]
    ConnectionClosed,
    #[error("transport does not support this operation: {0}")]
    Unsupported(String),
    #[error("protocol error: {0}")]
    Protocol(#[from] ApiError),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[async_trait]
pub trait MessageProcessor: Send + Sync {
    async fn process_request(&self, request: ClientRequest) -> Result<ClientResponse, ApiError>;

    async fn process_notification(&self, notification: ServerNotification) -> Result<(), ApiError>;
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_request(&self, request: ClientRequest) -> Result<ClientResponse, TransportError>;

    async fn send_notification(
        &self,
        notification: ServerNotification,
    ) -> Result<(), TransportError>;

    fn supports_streaming(&self) -> bool;
}

#[derive(Clone)]
pub struct DirectTransport<P> {
    processor: Arc<P>,
    streaming: bool,
}

impl<P> DirectTransport<P> {
    pub fn new(processor: P) -> Self {
        Self {
            processor: Arc::new(processor),
            streaming: false,
        }
    }

    pub fn with_streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }
}

#[async_trait]
impl<P> Transport for DirectTransport<P>
where
    P: MessageProcessor + 'static,
{
    async fn send_request(&self, request: ClientRequest) -> Result<ClientResponse, TransportError> {
        self.processor
            .process_request(request)
            .await
            .map_err(TransportError::Protocol)
    }

    async fn send_notification(
        &self,
        notification: ServerNotification,
    ) -> Result<(), TransportError> {
        self.processor
            .process_notification(notification)
            .await
            .map_err(TransportError::Protocol)
    }

    fn supports_streaming(&self) -> bool {
        self.streaming
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{NoParams, SerializationScope};
    use crate::v1;

    struct FakeProcessor;

    #[async_trait]
    impl MessageProcessor for FakeProcessor {
        async fn process_request(
            &self,
            request: ClientRequest,
        ) -> Result<ClientResponse, ApiError> {
            match request {
                ClientRequest::SessionList(NoParams {}) => {
                    Ok(ClientResponse::SessionList(v1::SessionListResponse {
                        sessions: vec![v1::SessionSummary {
                            id: "session-1".to_string(),
                            title: Some("Test".to_string()),
                            archived: false,
                            chat_mode_override: None,
                            effective_chat_mode: "normal".to_string(),
                        }],
                    }))
                }
                other => Err(ApiError::Validation {
                    field: "method".to_string(),
                    message: format!("unsupported request: {:?}", other.method()),
                }),
            }
        }

        async fn process_notification(
            &self,
            notification: ServerNotification,
        ) -> Result<(), ApiError> {
            match notification {}
        }
    }

    #[tokio::test]
    async fn direct_transport_dispatches_client_request() {
        let transport = DirectTransport::new(FakeProcessor);
        let response = transport
            .send_request(ClientRequest::SessionList(NoParams {}))
            .await
            .expect("fake request should succeed");

        assert!(matches!(response, ClientResponse::SessionList(_)));
        assert!(!transport.supports_streaming());
    }

    #[test]
    fn json_rpc_frame_roundtrips_request() {
        let frame = JsonRpcFrame::request(7, ClientRequest::SessionList(NoParams {}));
        let encoded = serde_json::to_string(&frame).expect("frame should serialize");
        let decoded: JsonRpcFrame =
            serde_json::from_str(&encoded).expect("frame should deserialize");

        assert_eq!(decoded, frame);
    }

    #[test]
    fn json_rpc_error_frame_roundtrips() {
        let frame = JsonRpcFrame::error(
            9,
            ApiError::Validation {
                field: "method".to_string(),
                message: "unsupported".to_string(),
            },
        );
        let encoded = serde_json::to_string(&frame).expect("frame should serialize");
        let decoded: JsonRpcFrame =
            serde_json::from_str(&encoded).expect("frame should deserialize");

        assert_eq!(decoded, frame);
        match decoded {
            JsonRpcFrame::Error { id, error, .. } => {
                assert_eq!(id, 9);
                assert_eq!(error.code, "validation");
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    #[test]
    fn json_rpc_invalid_frame_is_rejected() {
        let invalid = serde_json::json!({
            "type": "request",
            "jsonrpc": "1.0",
            "id": 3,
            "request": {
                "method": "SessionList",
                "params": {}
            }
        });

        let error = serde_json::from_value::<JsonRpcFrame>(invalid)
            .expect_err("invalid jsonrpc version should not deserialize");
        assert!(
            error.to_string().contains("2.0"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn frame_preserves_request_serialization_metadata() {
        let request = ClientRequest::SessionResume(v1::SessionResumeParams {
            id: "session-2".to_string(),
        });
        assert_eq!(
            request.serialization_scope(),
            SerializationScope::PerKey {
                field: "id",
                key: "session-2".to_string(),
            }
        );
    }
}
