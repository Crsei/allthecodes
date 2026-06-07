//! Web-facing local gateway lifecycle handlers.

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_daemon::gateway_client::{LocalGatewayClient, LocalGatewayDaemonStatus};
use allthecodes_daemon::process_state;

use crate::handlers::ApiError;

const LOCAL_GATEWAY_ID: &str = "local-daemon";
const LOCAL_GATEWAY_NAME: &str = "Local daemon gateway";

#[derive(Debug, Deserialize)]
pub struct GatewayQuery {
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayActionRequest {
    pub profile_id: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct GatewayStatusResponse {
    pub status: String,
    pub port: Option<u16>,
    pub profile_id: Option<String>,
    pub message: Option<String>,
    pub id: String,
    pub name: String,
    pub bind_address: Option<String>,
    pub diagnostics: Vec<String>,
    pub log_ref: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct GatewayListResponse {
    pub gateways: Vec<GatewayStatusResponse>,
    pub active_gateway_id: Option<String>,
}

/// GET /api/gateway/status
pub async fn gateway_status_handler(Query(query): Query<GatewayQuery>) -> Response {
    Json(status_response(query.profile_id)).into_response()
}

/// GET /api/gateways
pub async fn gateways_list_handler(Query(query): Query<GatewayQuery>) -> Response {
    Json(GatewayListResponse {
        gateways: vec![status_response(query.profile_id)],
        active_gateway_id: Some(LOCAL_GATEWAY_ID.to_string()),
    })
    .into_response()
}

/// POST /api/gateways/{id}/start
pub async fn gateway_start_handler(
    AxumPath(id): AxumPath<String>,
    body: Option<Json<GatewayActionRequest>>,
) -> Response {
    if !is_local_gateway_id(&id) {
        return api_error(
            StatusCode::NOT_FOUND,
            "invalid_gateway_id",
            format!("Unknown gateway id: {id}"),
        );
    }

    let port = body
        .as_ref()
        .and_then(|Json(req)| req.port)
        .unwrap_or(17322);
    api_error(
        StatusCode::CONFLICT,
        "gateway_start_requires_shell",
        format!("Start the local daemon from a shell with `FEATURE_KAIROS=1 allthecodes daemon start --port {port}`."),
    )
}

/// POST /api/gateways/{id}/stop
pub async fn gateway_stop_handler(
    AxumPath(id): AxumPath<String>,
    body: Option<Json<GatewayActionRequest>>,
) -> Response {
    if !is_local_gateway_id(&id) {
        return api_error(
            StatusCode::NOT_FOUND,
            "invalid_gateway_id",
            format!("Unknown gateway id: {id}"),
        );
    }

    let profile_id = body.and_then(|Json(req)| req.profile_id);
    if let Err(error) = process_state::request_shutdown("web gateway stop") {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_state_unavailable",
            format!("The daemon shutdown request could not be written: {error}"),
        );
    }

    let mut response = status_response(profile_id);
    if response.status == "running" {
        response.message = Some("Daemon shutdown was requested.".to_string());
        response
            .diagnostics
            .push("The daemon may remain running briefly while it exits.".to_string());
    }
    Json(response).into_response()
}

fn status_response(profile_id: Option<String>) -> GatewayStatusResponse {
    match LocalGatewayClient::daemon_status() {
        Ok(status) => status_from_daemon(status, profile_id),
        Err(error) => base_status(
            "error",
            None,
            profile_id,
            Some("The daemon state could not be read.".to_string()),
            None,
            vec![error.to_string()],
        ),
    }
}

fn status_from_daemon(
    status: LocalGatewayDaemonStatus,
    profile_id: Option<String>,
) -> GatewayStatusResponse {
    match status {
        LocalGatewayDaemonStatus::Running {
            pid,
            base_url,
            health_url,
        } => {
            let port = parse_port(&base_url).or_else(|| parse_port(&health_url));
            let bind_address =
                parse_bind_address(&base_url).or_else(|| parse_bind_address(&health_url));
            base_status(
                "running",
                port,
                profile_id,
                Some("Daemon gateway is running.".to_string()),
                bind_address,
                vec![format!("pid={pid}"), format!("health_url={health_url}")],
            )
        }
        LocalGatewayDaemonStatus::Stale { pid } => base_status(
            "error",
            None,
            profile_id,
            Some(format!("Daemon state is stale for pid {pid}.")),
            None,
            vec![format!("pid={pid}")],
        ),
        LocalGatewayDaemonStatus::Stopped => base_status(
            "stopped",
            None,
            profile_id,
            Some("The daemon is not running.".to_string()),
            None,
            Vec::new(),
        ),
    }
}

fn base_status(
    status: &str,
    port: Option<u16>,
    profile_id: Option<String>,
    message: Option<String>,
    bind_address: Option<String>,
    diagnostics: Vec<String>,
) -> GatewayStatusResponse {
    GatewayStatusResponse {
        status: status.to_string(),
        port,
        profile_id,
        message,
        id: LOCAL_GATEWAY_ID.to_string(),
        name: LOCAL_GATEWAY_NAME.to_string(),
        bind_address,
        diagnostics,
        log_ref: Some(
            process_state::daemon_dir()
                .join("supervisor.log")
                .display()
                .to_string(),
        ),
        updated_at: Utc::now().timestamp_millis(),
    }
}

fn is_local_gateway_id(id: &str) -> bool {
    matches!(id, LOCAL_GATEWAY_ID | "default")
}

fn api_error(status: StatusCode, code: &str, error: impl Into<String>) -> Response {
    (
        status,
        Json(ApiError {
            error: error.into(),
            code: code.to_string(),

            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn parse_port(url: &str) -> Option<u16> {
    let authority = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = authority.split('/').next().unwrap_or(authority);
    let port = authority.rsplit_once(':')?.1;
    port.parse().ok()
}

fn parse_bind_address(url: &str) -> Option<String> {
    let authority = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = authority.split('/').next().unwrap_or(authority);
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WebState;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request};
    use serde_json::{json, Value};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn temp_home() -> (TempDir, EnvGuard) {
        let temp = tempfile::tempdir().expect("tempdir");
        let guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        (temp, guard)
    }

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

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("json body")
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn gateway_status_returns_stopped_when_daemon_state_absent() {
        let (_home, _guard) = temp_home();

        let response = gateway_status_handler(Query(GatewayQuery { profile_id: None }))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["id"], json!("local-daemon"));
        assert_eq!(body["status"], json!("stopped"));
        assert_eq!(body["message"], json!("The daemon is not running."));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn gateways_list_returns_default_gateway() {
        let (_home, _guard) = temp_home();

        let response = gateways_list_handler(Query(GatewayQuery { profile_id: None }))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["active_gateway_id"], json!("local-daemon"));
        assert_eq!(body["gateways"][0]["id"], json!("local-daemon"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn gateway_start_route_does_not_fall_through() {
        let (_home, _guard) = temp_home();
        let app = crate::build_router(make_web_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/gateways/local-daemon/start")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("gateway_start_requires_shell"));
        assert_ne!(body["code"], json!("capability_not_implemented"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn gateway_stop_route_rejects_unknown_id() {
        let (_home, _guard) = temp_home();
        let app = crate::build_router(make_web_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/gateways/remote/stop")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("invalid_gateway_id"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn capabilities_gateways_true_has_registered_routes() {
        let (_home, _guard) = temp_home();
        let caps = crate::handlers::capabilities_map();
        assert_eq!(caps.get("gateways"), Some(&true));

        for (method, uri, expected_status) in [
            (Method::GET, "/api/gateway/status", StatusCode::OK),
            (Method::GET, "/api/gateways", StatusCode::OK),
            (
                Method::POST,
                "/api/gateways/local-daemon/start",
                StatusCode::CONFLICT,
            ),
            (
                Method::POST,
                "/api/gateways/unknown/stop",
                StatusCode::NOT_FOUND,
            ),
        ] {
            let app = crate::build_router(make_web_state());
            let response = app
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), expected_status, "{method} {uri}");
            let body = response_json(response).await;
            assert_ne!(body["code"], json!("capability_not_implemented"));
        }
    }
}
