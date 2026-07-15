//! Web-facing local gateway lifecycle handlers.

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_daemon::gateway_client::{LocalGatewayClient, LocalGatewayDaemonStatus};
use allthecodes_daemon::process_state;
use allthecodes_daemon::readiness;

use allthecodes_protocol::ApiError as ProtocolApiError;

use allthecodes_protocol::v1::gateways::GatewayListResponse as ProtocolGatewayListResponse;
use allthecodes_protocol::v1::gateways::GatewayStatusResponse as ProtocolGatewayStatusResponse;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::NoParams;
use async_trait::async_trait;
use axum::extract::State;
use axum::routing::get;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GatewayStatusProcessor {
    state: WebState,
}

impl From<WebState> for GatewayStatusProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for GatewayStatusProcessor {
    type Request = NoParams;
    type Response = ProtocolGatewayStatusResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "gateway.status"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let handler_resp = status_response(None);
        serde_json::from_value(serde_json::to_value(&handler_resp).map_err(|e| {
            ProtocolApiError::Internal {
                message: e.to_string(),
            }
        })?)
        .map_err(|e| ProtocolApiError::Internal {
            message: e.to_string(),
        })
    }
}

#[derive(Clone)]
pub struct GatewaysListProcessor {
    state: WebState,
}

impl From<WebState> for GatewaysListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for GatewaysListProcessor {
    type Request = NoParams;
    type Response = ProtocolGatewayListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "gateways.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let handler_resp = GatewayListResponse {
            gateways: vec![status_response(None)],
            active_gateway_id: Some(LOCAL_GATEWAY_ID.to_string()),
        };
        serde_json::from_value(serde_json::to_value(&handler_resp).map_err(|e| {
            ProtocolApiError::Internal {
                message: e.to_string(),
            }
        })?)
        .map_err(|e| ProtocolApiError::Internal {
            message: e.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::GatewayStatus, get(gateway_status_handler))
        .handle(ApiMethod::GatewaysList, get(gateways_list_handler))
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

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
pub async fn gateway_status_handler(
    State(state): State<WebState>,
    Query(_query): Query<allthecodes_protocol::v1::gateways::GatewayQuery>,
) -> Response {
    rest_processor_response::<GatewayStatusProcessor>(state, ApiMethod::GatewayStatus, NoParams {})
        .await
}

/// GET /api/gateways
pub async fn gateways_list_handler(
    State(state): State<WebState>,
    Query(_query): Query<allthecodes_protocol::v1::gateways::GatewayQuery>,
) -> Response {
    rest_processor_response::<GatewaysListProcessor>(state, ApiMethod::GatewaysList, NoParams {})
        .await
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
            ready_url,
            binary_version,
            binary_path,
            log_path,
            identity,
        } => {
            let port = parse_port(&base_url).or_else(|| parse_port(&health_url));
            let bind_address =
                parse_bind_address(&base_url).or_else(|| parse_bind_address(&health_url));
            let mut diagnostics = vec![
                format!("pid={pid}"),
                format!("health_url={health_url}"),
                format!("version={}", binary_version.as_deref().unwrap_or("unknown")),
                format!("binary={}", binary_path.as_deref().unwrap_or("unknown")),
                format!("log={}", log_path.as_deref().unwrap_or("unknown")),
                identity,
            ];
            if let Some(port) = port {
                let ready_url = ready_url.unwrap_or_else(|| readiness::ready_url(port));
                diagnostics.push(format!("ready_url={ready_url}"));
                diagnostics.push(
                    match readiness::probe_ready(port, std::time::Duration::from_millis(500)) {
                        Ok(()) => "readiness=ok".to_string(),
                        Err(error) => format!("readiness=error:{error}"),
                    },
                );
            } else {
                diagnostics.push("ready_url=unknown".to_string());
                diagnostics.push("readiness=error:daemon port unavailable".to_string());
            }
            base_status(
                "running",
                port,
                profile_id,
                Some("Daemon gateway is running.".to_string()),
                bind_address,
                diagnostics,
            )
        }
        LocalGatewayDaemonStatus::Stale {
            pid,
            binary_version,
            log_path,
            identity,
        } => {
            let diagnostics = vec![
                format!("pid={pid}"),
                format!("version={}", binary_version.as_deref().unwrap_or("unknown")),
                format!("log={}", log_path.as_deref().unwrap_or("unknown")),
                identity,
            ];
            base_status(
                "error",
                None,
                profile_id,
                Some(format!("Daemon state is stale for pid {pid}.")),
                None,
                diagnostics,
            )
        }
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

fn api_error(status: StatusCode, code: &'static str, error: impl Into<String>) -> Response {
    let body = ProtocolApiError::BadRequest {
        code,
        message: error.into(),
    }
    .into_body();
    (status, Json(body)).into_response()
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
    use crate::handlers::test_support::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use serde_json::json;
    use tower::ServiceExt;

    use allthecodes_protocol::v1::gateways::GatewayQuery as ProtocolGatewayQuery;

    #[tokio::test]
    #[serial_test::serial]
    async fn gateway_status_returns_stopped_when_daemon_state_absent() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();

        let response = gateway_status_handler(
            State(state),
            Query(ProtocolGatewayQuery { profile_id: None }),
        )
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
    async fn gateway_status_running_includes_readiness_diagnostics() {
        let (_home, _guard) = temp_home();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        allthecodes_daemon::process_state::write_started(port, std::path::Path::new("."))
            .expect("daemon state");
        let state = make_web_state();

        let response = gateway_status_handler(
            State(state),
            Query(ProtocolGatewayQuery { profile_id: None }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["status"], json!("running"));
        let diagnostics = body["diagnostics"].as_array().expect("diagnostics");
        assert!(diagnostics
            .iter()
            .any(|value| value == &json!(format!("ready_url=http://127.0.0.1:{port}/readyz"))));
        assert!(diagnostics
            .iter()
            .any(|value| value == &json!(format!("version={}", env!("CARGO_PKG_VERSION")))));
        assert!(diagnostics
            .iter()
            .any(|value| value.as_str().is_some_and(|diagnostic| {
                diagnostic.starts_with("log=") && diagnostic.ends_with("supervisor.log")
            })));
        assert!(diagnostics
            .iter()
            .any(|value| value == &json!("identity=matched")));
        assert!(diagnostics.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|diagnostic| diagnostic.starts_with("readiness=error:"))
        }));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn gateways_list_returns_default_gateway() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();

        let response = gateways_list_handler(
            State(state),
            Query(ProtocolGatewayQuery { profile_id: None }),
        )
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
        let app = crate::build_router(
            make_web_state()
                .with_control_token("test-control-token")
                .with_listener_authority("127.0.0.1:17322"),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/gateways/local-daemon/start")
                    .header("host", "127.0.0.1:17322")
                    .header(crate::auth::CONTROL_TOKEN_HEADER, "test-control-token")
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
        let app = crate::build_router(
            make_web_state()
                .with_control_token("test-control-token")
                .with_listener_authority("127.0.0.1:17322"),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/gateways/remote/stop")
                    .header("host", "127.0.0.1:17322")
                    .header(crate::auth::CONTROL_TOKEN_HEADER, "test-control-token")
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
            let app = crate::build_router(
                make_web_state()
                    .with_control_token("test-control-token")
                    .with_listener_authority("127.0.0.1:17322"),
            );
            let response = app
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(uri)
                        .header("host", "127.0.0.1:17322")
                        .header(crate::auth::CONTROL_TOKEN_HEADER, "test-control-token")
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
