//! Capabilities discovery and API fallback handlers.

use std::collections::HashMap;

use allthecodes_protocol::ApiError as ProtocolApiError;
use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

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
    caps.insert("skills".into(), true);
    caps.insert("memory".into(), true);
    caps.insert("speech".into(), true);
    caps.insert("tts".into(), true);
    caps.insert("web_search".into(), true);
    caps.insert("network".into(), true);
    caps.insert("data".into(), true);
    caps.insert("token_savings".into(), true);
    caps.insert("kanban".into(), true);
    caps.insert("jobs".into(), true);
    caps.insert("group_chat".into(), true);
    caps.insert("files".into(), true);
    caps.insert("logs".into(), true);
    caps.insert("backend_services".into(), true);
    caps
}

/// Catch-all handler for unregistered /api/* paths.
/// Returns 501 JSON instead of falling through to static file serving.
pub async fn api_fallback_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let error = ProtocolApiError::NotImplemented { capability: path };
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body()))
}
