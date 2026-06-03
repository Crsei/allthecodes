//! Chrome Relay status and explicit action boundaries.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::handlers::ApiError;
use crate::state::WebState;

#[derive(Serialize)]
pub struct ChromeRelayStatusResponse {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub diagnostics: Vec<String>,
}

/// GET /api/chrome-relay/status
pub async fn chrome_relay_status_handler(State(state): State<WebState>) -> impl IntoResponse {
    let enabled = state
        .engine()
        .app_state()
        .settings
        .claude_in_chrome_default_enabled
        .unwrap_or(false);
    Json(ChromeRelayStatusResponse {
        enabled,
        available: false,
        status: "not_implemented".to_string(),
        diagnostics: vec![
            "Chrome Relay launch and token management are not implemented by this backend"
                .to_string(),
        ],
    })
}

/// POST /api/chrome-relay/launch
pub async fn chrome_relay_launch_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: "Chrome Relay launch is not implemented by this backend".into(),
            code: "chrome_relay_launch_not_implemented".into(),
        }),
    )
}

/// POST /api/chrome-relay/token/regenerate
pub async fn chrome_relay_token_regenerate_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: "Chrome Relay token regeneration is not implemented by this backend".into(),
            code: "chrome_relay_token_regenerate_not_implemented".into(),
        }),
    )
}
