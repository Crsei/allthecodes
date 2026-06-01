//! Capabilities discovery and API fallback handlers.

use std::collections::HashMap;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::handlers::ApiError;

#[derive(Serialize)]
pub struct CapabilityDiscoveryResponse {
    pub capabilities: HashMap<String, bool>,
}

/// GET /api/capabilities -- Return capability discovery map.
pub async fn capabilities_handler() -> impl IntoResponse {
    let mut caps = HashMap::new();
    // Ready capabilities
    caps.insert("chat".into(), true);
    caps.insert("sessions".into(), true);
    caps.insert("settings".into(), true);
    caps.insert("debug".into(), true);
    caps.insert("state".into(), true);
    // Not yet implemented
    caps.insert("auth".into(), true);
    caps.insert("profiles".into(), true);
    caps.insert("gateways".into(), false);
    caps.insert("models".into(), true);
    caps.insert("providers".into(), true);
    caps.insert("credentials".into(), true);
    caps.insert("usage".into(), false);
    caps.insert("skills".into(), false);
    caps.insert("memory".into(), false);
    caps.insert("kanban".into(), false);
    caps.insert("jobs".into(), false);
    caps.insert("group_chat".into(), false);
    caps.insert("files".into(), false);
    caps.insert("logs".into(), false);
    caps.insert("backend_services".into(), false);
    Json(CapabilityDiscoveryResponse { capabilities: caps })
}

/// Catch-all handler for unregistered /api/* paths.
/// Returns 501 JSON instead of falling through to static file serving.
pub async fn api_fallback_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let status = if path.starts_with("api/") {
        StatusCode::NOT_IMPLEMENTED
    } else {
        StatusCode::NOT_FOUND
    };
    (
        status,
        Json(ApiError {
            error: "API endpoint not implemented".into(),
            code: "capability_not_implemented".into(),
        }),
    )
}
