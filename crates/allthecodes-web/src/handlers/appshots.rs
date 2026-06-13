//! Appshots status and explicit capture boundary.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::handlers::setting_bool;
use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Serialize)]
pub struct AppshotsStatusResponse {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub diagnostics: Vec<String>,
}

/// GET /api/appshots/status
pub async fn appshots_status_handler(State(state): State<WebState>) -> impl IntoResponse {
    Json(AppshotsStatusResponse {
        enabled: setting_bool(&state, "appshots.enabled").unwrap_or(false),
        available: false,
        status: "not_implemented".to_string(),
        diagnostics: vec!["appshot runtime is not implemented by this backend".to_string()],
    })
}

/// POST /api/appshots/capture
pub async fn appshots_capture_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            ProtocolApiError::BadRequest {
                code: "appshots_capture_not_implemented",
                message: "Appshot capture is not implemented by this backend".into(),
            }
            .into_body(),
        ),
    )
}
