//! Error conversion helpers for the ACP runtime.
//!
//! Provides convenient conversions from common error types to ACP JSON-RPC errors.

use agent_client_protocol_schema::v2;

/// Convert an anyhow error into an ACP InternalError.
pub fn internal_error(err: impl std::fmt::Display) -> v2::Error {
    v2::Error::internal_error().data(err.to_string())
}

/// Convert an anyhow error into an ACP InvalidParams error.
pub fn invalid_params(err: impl std::fmt::Display) -> v2::Error {
    v2::Error::invalid_params().data(err.to_string())
}

/// Wrap a custom error with InvalidParams.
pub fn invalid_params_msg(msg: &str) -> v2::Error {
    v2::Error::invalid_params().data(msg.to_string())
}

/// Build a ResourceNotFound error for a session.
pub fn session_not_found(session_id: &str) -> v2::Error {
    v2::Error::resource_not_found(Some(format!("session:{session_id}")))
}

/// Build a MethodNotFound error.
pub fn method_not_found(method: &str) -> v2::Error {
    v2::Error::method_not_found().data(format!("method not found: {method}"))
}
