//! JSON-RPC 2.0 envelope parsing and serialization for the ACP stdio transport.
//!
//! Wraps the `agent-client-protocol-schema` `rpc` types with parsing utilities
//! that handle single requests, notifications, batches, and the standard JSON-RPC
//! error response shape.

use std::sync::Arc;

use agent_client_protocol_schema::rpc::{
    JsonRpcBatch, JsonRpcMessage, Notification, RequestId, Response,
};
use agent_client_protocol_schema::v2;
use serde_json::value::RawValue;

/// A parsed inbound message: a single JSON-RPC request, notification, or batch.
#[derive(Debug, Clone)]
pub enum InboundMessage {
    /// A single request (expects a response).
    Request {
        id: RequestId,
        method: Arc<str>,
        params: Option<Box<RawValue>>,
    },
    /// A single notification (no response).
    Notification {
        method: Arc<str>,
        params: Option<Box<RawValue>>,
    },
    /// A non-empty batch containing requests and/or notifications.
    Batch(Vec<InboundBatchEntry>),
}

/// One entry in a parsed batch.
#[derive(Debug, Clone)]
pub enum InboundBatchEntry {
    /// A request within the batch.
    Request {
        id: RequestId,
        method: Arc<str>,
        params: Option<Box<RawValue>>,
    },
    /// A notification within the batch.
    Notification {
        method: Arc<str>,
        params: Option<Box<RawValue>>,
    },
}

/// Parse a single line of JSON-RPC input.
///
/// Returns `None` (silently skip) for valid JSON that is neither an object
/// nor an array (e.g. a bare JSON literal on its own line — not valid JSON-RPC).
///
/// # Errors
///
/// Returns a JSON-RPC `ParseError` response if the input is malformed JSON.
/// Returns `MethodNotFound` for unrecognized top-level shapes.
pub fn parse_frame(raw: &str) -> Result<Option<InboundMessage>, v2::Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| v2::Error::parse_error().data(e.to_string()))?;

    match &value {
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return Err(v2::Error::invalid_request()
                    .data("empty batch is not allowed by JSON-RPC 2.0"));
            }
            let mut entries = Vec::with_capacity(items.len());
            for item in items {
                let entry = parse_single_value(item)?;
                entries.push(entry);
            }
            Ok(Some(InboundMessage::Batch(entries)))
        }
        serde_json::Value::Object(_) => {
            let entry = parse_single_value(&value)?;
            match entry {
                InboundBatchEntry::Request { id, method, params } => {
                    Ok(Some(InboundMessage::Request { id, method, params }))
                }
                InboundBatchEntry::Notification { method, params } => {
                    Ok(Some(InboundMessage::Notification { method, params }))
                }
            }
        }
        _ => {
            // Bare JSON literal: not valid JSON-RPC, silently skip.
            Ok(None)
        }
    }
}

fn parse_single_value(value: &serde_json::Value) -> Result<InboundBatchEntry, v2::Error> {
    let obj = value.as_object().ok_or_else(|| {
        v2::Error::invalid_request().data("each batch element must be a JSON object")
    })?;

    // Check jsonrpc version if present.
    if let Some(ver) = obj.get("jsonrpc") {
        if ver.as_str() != Some("2.0") {
            return Err(v2::Error::invalid_request().data("only jsonrpc 2.0 is supported"));
        }
    }

    let method = obj
        .get("method")
        .and_then(|v| v.as_str())
        .map(|s| Arc::<str>::from(s.to_string()))
        .ok_or_else(|| v2::Error::invalid_request().data("missing 'method' field"))?;

    let has_id = obj.get("id").is_some();

    let params = obj.get("params").and_then(parse_params_raw);

    if has_id {
        let id = parse_request_id(obj.get("id"))?;
        Ok(InboundBatchEntry::Request { id, method, params })
    } else {
        Ok(InboundBatchEntry::Notification { method, params })
    }
}

fn parse_request_id(value: Option<&serde_json::Value>) -> Result<RequestId, v2::Error> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(RequestId::Null),
        Some(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Ok(RequestId::Number(i))
            } else if let Some(u) = n.as_u64() {
                Ok(RequestId::Number(u as i64))
            } else {
                Err(v2::Error::invalid_request().data("numeric request id out of range"))
            }
        }
        Some(serde_json::Value::String(s)) => Ok(RequestId::Str(s.clone())),
        _ => Err(v2::Error::invalid_request().data("invalid request id type")),
    }
}

fn parse_params_raw(value: &serde_json::Value) -> Option<Box<RawValue>> {
    match value {
        serde_json::Value::Null => None,
        other => {
            let json_str = serde_json::to_string(other).ok()?;
            RawValue::from_string(json_str).ok()
        }
    }
}

/// Build a JSON-RPC response for a given request id and result/error.
pub fn build_response<T: serde::Serialize>(
    id: &RequestId,
    result: Result<T, v2::Error>,
) -> serde_json::Value {
    let response = match result {
        Ok(value) => Response::Result {
            id: id.clone(),
            result: serde_json::to_value(value).unwrap_or_default(),
        },
        Err(err) => Response::Error {
            id: id.clone(),
            error: err,
        },
    };
    let msg = JsonRpcMessage::wrap(response);
    serde_json::to_value(msg).unwrap_or_default()
}

/// Build a JSON-RPC notification (no response expected).
pub fn build_notification<T: serde::Serialize>(
    method: &str,
    params: &T,
) -> serde_json::Value {
    let notification = Notification {
        method: Arc::from(method),
        params: Some(serde_json::to_value(params).unwrap_or_default()),
    };
    let msg = JsonRpcMessage::wrap(notification);
    serde_json::to_value(msg).unwrap_or_default()
}

/// Build an ACP agent-to-client JSON-RPC notification.
pub fn build_agent_notification(notification: v2::AgentNotification) -> serde_json::Value {
    let method = notification.method().to_string();
    build_notification(&method, &notification)
}

/// Build a JSON-RPC batch response containing responses for requests that
/// require a response.
pub fn build_batch_response(
    entries: &[InboundBatchEntry],
    results: Vec<Result<serde_json::Value, v2::Error>>,
) -> Option<serde_json::Value> {
    let mut responses = Vec::new();
    let mut result_idx = 0;

    for entry in entries {
        match entry {
            InboundBatchEntry::Request { id, .. } => {
                if result_idx >= results.len() {
                    // Fallback: internal error for unmatched entries.
                    let err_resp = Response::<serde_json::Value, v2::Error>::Error {
                        id: id.clone(),
                        error: v2::Error::internal_error(),
                    };
                    responses.push(JsonRpcMessage::wrap(err_resp));
                } else {
                    match &results[result_idx] {
                        Ok(value) => {
                            let resp = Response::Result {
                                id: id.clone(),
                                result: value.clone(),
                            };
                            responses.push(JsonRpcMessage::wrap(resp));
                        }
                        Err(err) => {
                            let resp = Response::Error {
                                id: id.clone(),
                                error: err.clone(),
                            };
                            responses.push(JsonRpcMessage::wrap(resp));
                        }
                    }
                }
                result_idx += 1;
            }
            InboundBatchEntry::Notification { .. } => {
                // Notifications do not get responses.
            }
        }
    }

    if responses.is_empty() {
        return None;
    }

    let batch = JsonRpcBatch::new(responses).ok()?;
    Some(serde_json::to_value(batch).unwrap_or_default())
}

/// Build a JSON-RPC `MethodNotFound` error response.
pub fn method_not_found(id: &RequestId, method: &str) -> serde_json::Value {
    build_response::<()>(
        id,
        Err(v2::Error::method_not_found().data(format!("unknown method: {method}"))),
    )
}

/// Build a JSON-RPC `ParseError` response (id is always null).
pub fn parse_error(message: String) -> serde_json::Value {
    let err = v2::Error::parse_error().data(message);
    let response = Response::<serde_json::Value, v2::Error>::Error {
        id: RequestId::Null,
        error: err,
    };
    let msg = JsonRpcMessage::wrap(response);
    serde_json::to_value(msg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_request() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let result = parse_frame(raw).unwrap().unwrap();
        match result {
            InboundMessage::Request { id, method, params } => {
                assert_eq!(id, RequestId::Number(1));
                assert_eq!(&*method, "initialize");
                assert!(params.is_none());
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn parse_notification_no_response() {
        let raw = r#"{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"abc"}}"#;
        let result = parse_frame(raw).unwrap().unwrap();
        match result {
            InboundMessage::Notification { method, params } => {
                assert_eq!(&*method, "session/cancel");
                assert!(params.is_some());
            }
            _ => panic!("expected notification"),
        }
    }

    #[test]
    fn null_id_is_preserved_as_request_id() {
        let raw = r#"{"jsonrpc":"2.0","id":null,"method":"initialize"}"#;
        let result = parse_frame(raw).unwrap().unwrap();
        match result {
            InboundMessage::Request { id, method, .. } => {
                assert_eq!(id, RequestId::Null);
                assert_eq!(&*method, "initialize");
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn parse_non_empty_batch() {
        let raw = r#"[
            {"jsonrpc":"2.0","id":1,"method":"initialize"},
            {"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"abc"}}
        ]"#;
        let result = parse_frame(raw).unwrap().unwrap();
        match result {
            InboundMessage::Batch(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(matches!(&entries[0], InboundBatchEntry::Request { .. }));
                assert!(matches!(&entries[1], InboundBatchEntry::Notification { .. }));
            }
            _ => panic!("expected batch"),
        }
    }

    #[test]
    fn reject_empty_batch() {
        let raw = r#"[]"#;
        let result = parse_frame(raw);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty batch"));
    }

    #[test]
    fn malformed_json_returns_parse_error() {
        let raw = r#"{invalid json}"#;
        let result = parse_frame(raw);
        assert!(result.is_err());
    }

    #[test]
    fn stdout_writer_appends_single_newline() {
        let value = build_response::<&str>(&RequestId::Number(1), Ok("ok"));
        let json_str = serde_json::to_string(&value).unwrap();
        assert!(!json_str.contains('\n'), "no embedded newlines");
    }

    #[test]
    fn session_update_notification_is_jsonrpc_enveloped() {
        let update = v2::UpdateSessionNotification::new(
            v2::SessionId::new("sess"),
            v2::SessionUpdate::StateUpdate(v2::StateUpdate::Running(
                v2::RunningStateUpdate::new(),
            )),
        );
        let value = build_agent_notification(v2::AgentNotification::UpdateSessionNotification(
            Box::new(update),
        ));

        assert_eq!(value.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
        assert_eq!(
            value.get("method").and_then(|v| v.as_str()),
            Some("session/update")
        );
        assert!(value.get("params").is_some());
        assert!(value.get("sessionId").is_none());
    }
}
