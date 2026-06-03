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
    Json(CapabilityDiscoveryResponse {
        capabilities: capabilities_map(),
    })
}

pub fn capabilities_map() -> HashMap<String, bool> {
    let mut caps = HashMap::new();
    // Ready capabilities
    caps.insert("chat".into(), true);
    caps.insert("sessions".into(), true);
    caps.insert("settings".into(), true);
    caps.insert("agents".into(), true);
    caps.insert("people".into(), true);
    caps.insert("hooks".into(), true);
    caps.insert("prompts".into(), true);
    caps.insert("mcp_servers".into(), true);
    caps.insert("plugins".into(), true);
    caps.insert("channels".into(), true);
    caps.insert("computer_use".into(), true);
    caps.insert("appshots".into(), true);
    caps.insert("activity_recorder".into(), true);
    caps.insert("chrome_relay".into(), true);
    caps.insert("git".into(), true);
    caps.insert("proxy".into(), true);
    caps.insert("debug".into(), true);
    caps.insert("state".into(), true);
    // Not yet implemented
    caps.insert("auth".into(), true);
    caps.insert("profiles".into(), true);
    caps.insert("gateways".into(), true);
    caps.insert("models".into(), true);
    caps.insert("providers".into(), true);
    caps.insert("credentials".into(), true);
    caps.insert("usage".into(), true);
    caps.insert("skills".into(), false);
    caps.insert("memory".into(), true);
    caps.insert("speech".into(), true);
    caps.insert("tts".into(), true);
    caps.insert("web_search".into(), true);
    caps.insert("network".into(), true);
    caps.insert("data".into(), true);
    caps.insert("token_savings".into(), true);
    caps.insert("kanban".into(), false);
    caps.insert("jobs".into(), false);
    caps.insert("group_chat".into(), false);
    caps.insert("files".into(), false);
    caps.insert("logs".into(), false);
    caps.insert("backend_services".into(), false);
    caps
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
