//! Shared API error responses for Axum adapters.

use allthecodes_protocol::{ApiError as ProtocolApiError, ApiErrorBody};
use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

pub type ApiError = ApiErrorBody;

pub fn api_error_body(error: impl Into<String>, code: impl Into<String>) -> ApiErrorBody {
    ApiErrorBody {
        error: error.into(),
        code: code.into(),
        details: json!({}),
    }
}

pub fn protocol_error_response(error: ProtocolApiError) -> impl IntoResponse {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body()))
}

/// Catch-all handler for unregistered /api/* paths.
/// Returns 501 JSON instead of falling through to static file serving.
pub async fn api_fallback_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    protocol_error_response(ProtocolApiError::NotImplemented { capability: path })
}
