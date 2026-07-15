//! Web server module — Axum-based HTTP server for the chat UI.

pub mod api_dispatcher;
pub mod api_errors;
pub(crate) mod api_operation_registry;
mod auth;
pub mod handler_registry;
pub mod handlers;
pub mod ipc_streams;
pub mod processors;
pub mod serialization;
pub mod state;
pub mod static_files;
pub mod web_state_routes;
pub mod workspace_metadata;
pub mod ws;

use std::net::SocketAddr;

use allthecodes_server::RootProbeResponse;
use axum::{
    extract::State,
    http::{header, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::state::WebState;

async fn control_auth_middleware(
    State(state): State<WebState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let public_probe = matches!(path, "/healthz" | "/readyz" | "/startupz");
    let protected = !public_probe
        && (path.starts_with("/api/")
            || path.starts_with("/proxy/")
            || path.starts_with("/anthropic-proxy/"));
    if !protected {
        return next.run(request).await;
    }

    if request.method() == Method::OPTIONS {
        let origin = match auth::authorize_preflight(request.headers(), state.listener_authority())
        {
            Ok(origin) => origin,
            Err(status) => return status.into_response(),
        };
        let requested_method = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|value| value.to_str().ok());
        if !matches!(
            requested_method,
            Some("GET" | "POST" | "PUT" | "PATCH" | "DELETE")
        ) {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        let requested_headers = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !requested_headers.is_empty()
            && requested_headers.split(',').any(|name| {
                !matches!(
                    name.trim().to_ascii_lowercase().as_str(),
                    "authorization"
                        | "content-type"
                        | "x-allthecodes-control-token"
                        | "x-allthecodes-web-privileged-token"
                )
            })
        {
            return StatusCode::FORBIDDEN.into_response();
        }
        let mut response = StatusCode::OK.into_response();
        add_cors_headers(&mut response, &origin);
        return response;
    }

    let privileged = requires_privileged_capability(request.method(), path);
    let origin = match auth::authorize(
        request.headers(),
        state.control_token(),
        state.privileged_token(),
        state.listener_authority(),
        privileged,
    ) {
        Ok(origin) => origin,
        Err(status) => return status.into_response(),
    };
    let mut response = next.run(request).await;
    if let Some(origin) = origin.as_deref() {
        add_cors_headers(&mut response, origin);
    }
    response
}

fn requires_privileged_capability(method: &Method, path: &str) -> bool {
    let Some(path) = api_path_suffix(path) else {
        return false;
    };

    if matches!(path, "rpc/ws" | "ipc/ws" | "tui/ws")
        || path == "terminal/sessions"
        || path.starts_with("terminal/sessions/")
    {
        return true;
    }

    if matches!(
        (method, path),
        (&Method::PUT, "kairos/config")
            | (&Method::POST, "kairos/start")
            | (&Method::POST, "kairos/stop")
            | (&Method::POST, "kairos/restart")
            | (&Method::PUT, "files/write")
            | (&Method::POST, "files/upload")
            | (&Method::POST, "files/mkdir")
            | (&Method::POST, "files/rename")
            | (&Method::POST, "files/copy")
            | (&Method::POST, "files/move")
            | (&Method::DELETE, "files")
    ) {
        return true;
    }

    if method == Method::POST && has_single_parameter(path, "chat/permissions/", "/response") {
        return true;
    }

    if (method == Method::PATCH && has_single_parameter(path, "memory/", ""))
        || (method == Method::POST
            && (has_single_parameter(path, "memory/proposals/", "/approve")
                || has_single_parameter(path, "memory/proposals/", "/reject")))
    {
        return true;
    }

    if method == Method::POST
        && (has_single_parameter(path, "workflows/", "/runs")
            || has_single_parameter(path, "workflow-runs/", "/advance")
            || has_single_parameter(path, "workflow-runs/", "/cancel"))
    {
        return true;
    }

    if method == Method::POST
        && (matches!(
            path,
            "plugins/install"
                | "plugins/update"
                | "plugins/uninstall"
                | "plugins/enable"
                | "plugins/disable"
                | "plugins/restart"
        ) || has_single_parameter(path, "plugins/", "/uninstall"))
    {
        return true;
    }

    if matches!(
        (method, path),
        (&Method::POST, "auth/login")
            | (&Method::POST, "auth/logout")
            | (&Method::POST, "auth/refresh")
            | (&Method::POST, "account-auth/login/start")
            | (&Method::POST, "account-auth/login/complete")
            | (&Method::POST, "account-auth/refresh")
            | (&Method::POST, "account-auth/logout")
            | (&Method::POST, "providers")
            | (&Method::POST, "providers/openai-codex/apply-local")
    ) {
        return true;
    }

    if matches!(method, &Method::PATCH | &Method::DELETE)
        && has_single_parameter(path, "providers/", "")
    {
        return true;
    }

    is_provider_oauth_mutation(method, path) || is_mcp_oauth_mutation(method, path)
}

fn api_path_suffix(path: &str) -> Option<&str> {
    path.strip_prefix("/api/v2/")
        .or_else(|| path.strip_prefix("/api/"))
}

fn has_single_parameter(path: &str, prefix: &str, suffix: &str) -> bool {
    path.strip_prefix(prefix)
        .and_then(|path| path.strip_suffix(suffix))
        .is_some_and(|parameter| !parameter.is_empty() && !parameter.contains('/'))
}

fn is_provider_oauth_mutation(method: &Method, path: &str) -> bool {
    if method != Method::POST {
        return false;
    }
    let mut segments = path.split('/');
    matches!(
        (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next()
        ),
        (Some("oauth"), Some(provider), Some("start" | "poll"), None) if !provider.is_empty()
    )
}

fn is_mcp_oauth_mutation(method: &Method, path: &str) -> bool {
    let mut segments = path.split('/');
    let route = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    );
    match route {
        (Some("mcp-servers"), Some(server), Some("oauth"), Some("start" | "complete"), None)
            if !server.is_empty() =>
        {
            method == Method::POST
        }
        (Some("mcp-servers"), Some(server), Some("oauth"), None, None) if !server.is_empty() => {
            method == Method::DELETE
        }
        _ => false,
    }
}

fn add_cors_headers(response: &mut Response, origin: &str) {
    let Ok(origin) = HeaderValue::from_str(origin) else {
        return;
    };
    response
        .headers_mut()
        .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "authorization, content-type, x-allthecodes-control-token, x-allthecodes-web-privileged-token",
        ),
    );
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Origin"));
}

/// Build the Axum router with all routes.
pub fn build_router(state: WebState) -> Router {
    let registry = handler_registry::all_api_handlers();
    let router = handler_registry::register_protocol_routes(Router::new(), &registry)
        .merge(web_state_routes::routes())
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/startupz", get(startupz))
        .route(
            "/anthropic-proxy/{provider_id}/v1/messages",
            axum::routing::post(handlers::anthropic_proxy_messages_handler),
        )
        .route(
            "/anthropic-proxy/{provider_id}/v1/messages/count_tokens",
            axum::routing::post(handlers::anthropic_proxy_count_tokens_handler),
        )
        .route(
            "/proxy/{provider_id}/v1/responses",
            axum::routing::post(handlers::openai_proxy_responses_handler),
        )
        .route(
            "/api/mcp-bindings",
            get(handlers::mcp_bindings_list_handler).post(handlers::mcp_bindings_create_handler),
        )
        .route(
            "/api/mcp-bindings/{server_id}",
            patch(handlers::mcp_bindings_update_handler)
                .delete(handlers::mcp_bindings_delete_handler),
        )
        .route(
            "/api/v2/mcp-bindings",
            get(handlers::mcp_bindings_list_handler).post(handlers::mcp_bindings_create_handler),
        )
        .route(
            "/api/v2/mcp-bindings/{server_id}",
            patch(handlers::mcp_bindings_update_handler)
                .delete(handlers::mcp_bindings_delete_handler),
        )
        .route("/api/rpc/ws", get(ws::api_rpc::api_rpc_ws_handler));

    router
        // API catch-all: unregistered /api/* paths return JSON 501
        .route(
            "/api/{*path}",
            get(api_errors::api_fallback_handler).post(api_errors::api_fallback_handler),
        )
        .route(
            "/api/v2/{*path}",
            get(api_errors::api_fallback_handler).post(api_errors::api_fallback_handler),
        )
        // Static files (SPA)
        .fallback(static_files::static_handler)
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            control_auth_middleware,
        ))
        .with_state(state)
}

/// Start the web server on the given port.
pub async fn start_server(state: WebState, port: u16, no_open: bool) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    if state.control_token().is_none_or(str::is_empty) {
        anyhow::bail!("ALLTHECODES_WEB_CONTROL_TOKEN is required for Web mode");
    }
    let app = build_router(state.with_listener_authority(addr.to_string()));

    info!("Web UI starting on http://{}", addr);

    if !no_open {
        info!("Open http://{} in your browser", addr);
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Web UI listening on http://{}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn healthz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("healthz", "web"))
}

async fn readyz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("readyz", "web"))
}

async fn startupz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("startupz", "web"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tower::ServiceExt;

    const TEST_AUTHORITY: &str = "127.0.0.1:17322";
    const TEST_CONTROL_TOKEN: &str = "test-secret";
    const TEST_PRIVILEGED_TOKEN: &str = "privileged-secret";

    fn request_builder(method: Method, uri: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, TEST_AUTHORITY)
    }

    fn websocket_request_builder(uri: &str) -> axum::http::request::Builder {
        request_builder(Method::GET, uri)
            .header(header::CONNECTION, "upgrade")
            .header(header::UPGRADE, "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
    }

    fn make_unsecured_web_state() -> WebState {
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
            verification_policy: None,
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
            .with_listener_authority(TEST_AUTHORITY)
    }

    fn make_web_state() -> WebState {
        make_unsecured_web_state()
            .with_control_token(TEST_CONTROL_TOKEN)
            .with_privileged_token(TEST_PRIVILEGED_TOKEN)
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(uri)
                    .header(header::HOST, TEST_AUTHORITY)
                    .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                    .header("x-allthecodes-web-privileged-token", TEST_PRIVILEGED_TOKEN)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn get_public_json(app: Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn request_json(
        app: Router,
        method: Method,
        uri: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::HOST, TEST_AUTHORITY)
                    .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                    .header("x-allthecodes-web-privileged-token", TEST_PRIVILEGED_TOKEN)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&body).expect("body")))
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        (status, serde_json::from_slice(&body).expect("json"))
    }

    #[tokio::test]
    async fn root_probe_endpoints_are_available() {
        let app = build_router(make_web_state());

        for (uri, probe) in [
            ("/healthz", "healthz"),
            ("/readyz", "readyz"),
            ("/startupz", "startupz"),
        ] {
            let (status, body) = get_public_json(app.clone(), uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body["status"], json!("ok"));
            assert_eq!(body["probe"], json!(probe));
            assert_eq!(body["service"], json!("web"));
            assert!(body["pid"].as_u64().is_some());
            assert!(body["timestamp_ms"].as_u64().is_some());
        }
    }

    #[tokio::test]
    async fn security_static_assets_remain_public() {
        let response = build_router(make_web_state())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/assets/nonexistent.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(!matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));
    }

    #[tokio::test]
    async fn api_healthz_shape_is_unchanged() {
        let app = build_router(make_web_state());
        let (status, body) = get_json(app, "/api/healthz").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], json!("ok"));
        assert_eq!(body["db"], json!("connected"));
        assert!(body.get("version").is_some());
        assert!(body.get("probe").is_none());
        assert!(body.get("service").is_none());
        assert!(body.get("pid").is_none());
    }

    #[tokio::test]
    async fn terminal_healthz_reports_subsystem_without_starting_session() {
        let state = make_web_state();
        let app = build_router(state.clone());

        let (status, body) = get_json(app, "/api/terminal/healthz").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], json!("ok"));
        assert_eq!(body["subsystem"], json!("terminal"));
        assert_eq!(body["active_sessions"], json!(0));
        assert_eq!(body["can_spawn_profile"], json!(true));
        assert!(body.get("last_spawn_error").is_none());
        assert!(body.get("last_spawn_error_at").is_none());
        assert!(state.terminal_manager.list_sessions().is_empty());
    }

    #[tokio::test]
    async fn terminal_create_unknown_profile_returns_classified_error() {
        let app = build_router(make_web_state());

        let (status, body) = request_json(
            app,
            Method::POST,
            "/api/terminal/sessions",
            json!({ "profile": "missing-profile" }),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], json!("terminal_profile_not_found"));
    }

    #[tokio::test]
    async fn terminal_create_invalid_cwd_returns_classified_error() {
        let app = build_router(make_web_state());

        let (status, body) = request_json(
            app,
            Method::POST,
            "/api/terminal/sessions",
            json!({ "profile": "shell", "cwd": "/definitely/outside/allthecodes" }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], json!("terminal_cwd_invalid"));
    }

    #[tokio::test]
    async fn terminal_detail_missing_returns_terminal_not_found() {
        let app = build_router(make_web_state());

        let (status, body) = get_json(app, "/api/terminal/sessions/missing").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], json!("terminal_not_found"));
    }

    #[tokio::test]
    async fn security_loopback_api_without_configured_or_supplied_token_is_rejected() {
        let response = build_router(make_unsecured_web_state())
            .oneshot(
                request_builder(Method::GET, "/api/healthz")
                    .header(header::ORIGIN, format!("http://{TEST_AUTHORITY}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn security_attacker_controlled_host_and_origin_are_not_an_allowlist() {
        let app = build_router(make_web_state());
        let missing_host = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/healthz")
                    .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let attacker_agreement = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/healthz")
                    .header(header::HOST, "attacker.example:17322")
                    .header(header::ORIGIN, "http://attacker.example:17322")
                    .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(missing_host.status(), StatusCode::FORBIDDEN);
        assert_eq!(attacker_agreement.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn security_each_websocket_family_rejects_missing_token_and_bad_origin() {
        let app = build_router(make_web_state());
        let websocket_paths = [
            "/api/rpc/ws",
            "/api/ipc/ws?session_id=test-session",
            "/api/terminal/sessions/missing/ws",
            "/api/tui/ws",
        ];
        let mut missing_token_statuses = Vec::new();
        let mut bad_origin_statuses = Vec::new();

        for path in websocket_paths {
            let missing = app
                .clone()
                .oneshot(
                    websocket_request_builder(path)
                        .header(header::ORIGIN, format!("http://{TEST_AUTHORITY}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            missing_token_statuses.push(missing.status());

            let bad_origin = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(path)
                        .header(header::HOST, "attacker.example:17322")
                        .header(header::ORIGIN, "http://attacker.example:17322")
                        .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                        .header(header::CONNECTION, "upgrade")
                        .header(header::UPGRADE, "websocket")
                        .header("sec-websocket-version", "13")
                        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            bad_origin_statuses.push(bad_origin.status());
        }

        assert_eq!(missing_token_statuses, vec![StatusCode::UNAUTHORIZED; 4]);
        assert_eq!(bad_origin_statuses, vec![StatusCode::FORBIDDEN; 4]);
    }

    #[tokio::test]
    async fn security_each_websocket_family_requires_privileged_capability() {
        let app = build_router(make_web_state());
        let websocket_paths = [
            "/api/rpc/ws",
            "/api/ipc/ws?session_id=test-session",
            "/api/terminal/sessions/missing/ws",
            "/api/tui/ws",
        ];
        let mut control_only_statuses = Vec::new();
        let mut both_tokens_statuses = Vec::new();

        for path in websocket_paths {
            let control_only = app
                .clone()
                .oneshot(
                    websocket_request_builder(path)
                        .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            control_only_statuses.push(control_only.status());

            let both_tokens = app
                .clone()
                .oneshot(
                    websocket_request_builder(path)
                        .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN)
                        .header(&auth::PRIVILEGED_TOKEN_HEADER, TEST_PRIVILEGED_TOKEN)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            both_tokens_statuses.push(both_tokens.status());
        }

        assert_eq!(control_only_statuses, vec![StatusCode::FORBIDDEN; 4]);
        assert!(both_tokens_statuses
            .iter()
            .all(|status| !matches!(*status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)));
    }

    #[test]
    fn kairos_mutations_require_privileged_capability_but_status_does_not() {
        assert!(!requires_privileged_capability(&Method::GET, "/api/kairos"));
        assert!(requires_privileged_capability(
            &Method::PUT,
            "/api/kairos/config"
        ));
        for path in [
            "/api/kairos/start",
            "/api/kairos/stop",
            "/api/kairos/restart",
        ] {
            assert!(requires_privileged_capability(&Method::POST, path));
        }
    }

    #[test]
    fn memory_mutations_require_privileged_capability_but_reads_do_not() {
        for path in [
            "/api/memory",
            "/api/memory/dream",
            "/api/memory/proposals",
            "/api/memory/proposals/proposal-1",
        ] {
            assert!(!requires_privileged_capability(&Method::GET, path));
        }
        assert!(requires_privileged_capability(
            &Method::PATCH,
            "/api/memory/project:bWVtb3J5"
        ));
        assert!(requires_privileged_capability(
            &Method::POST,
            "/api/memory/proposals/proposal-1/approve"
        ));
        assert!(requires_privileged_capability(
            &Method::POST,
            "/api/memory/proposals/proposal-1/reject"
        ));
    }

    #[test]
    fn workflow_mutations_require_privileged_capability_but_reads_do_not() {
        for path in [
            "/api/workflows",
            "/api/workflows/release",
            "/api/workflow-runs",
            "/api/workflow-runs/workflow-run-1",
        ] {
            assert!(!requires_privileged_capability(&Method::GET, path));
        }
        for path in [
            "/api/workflows/release/runs",
            "/api/workflow-runs/workflow-run-1/advance",
            "/api/workflow-runs/workflow-run-1/cancel",
        ] {
            assert!(requires_privileged_capability(&Method::POST, path));
        }
    }

    async fn privileged_route_statuses(
        app: Router,
        privileged_token: Option<&str>,
    ) -> Vec<StatusCode> {
        let cases = [
            (Method::GET, "/api/terminal/sessions", None),
            (
                Method::POST,
                "/api/chat/permissions/tool-1/response",
                Some(json!({"session_id":"session-1","decision":"deny"})),
            ),
            (Method::PUT, "/api/files/write", Some(json!({}))),
            (Method::POST, "/api/plugins/enable", Some(json!({}))),
            (Method::POST, "/api/providers", Some(json!({}))),
            (Method::POST, "/api/auth/login", Some(json!({}))),
        ];
        let mut statuses = Vec::new();

        for (method, uri, body) in cases {
            let mut builder = request_builder(method, uri)
                .header(&auth::CONTROL_TOKEN_HEADER, TEST_CONTROL_TOKEN);
            if let Some(privileged_token) = privileged_token {
                builder = builder.header("x-allthecodes-web-privileged-token", privileged_token);
            }
            let body = body
                .map(|value| Body::from(serde_json::to_vec(&value).unwrap()))
                .unwrap_or_else(Body::empty);
            let response = app
                .clone()
                .oneshot(builder.body(body).unwrap())
                .await
                .unwrap();
            statuses.push(response.status());
        }
        statuses
    }

    #[tokio::test]
    async fn security_privileged_routes_reject_control_token_only() {
        let app = build_router(make_web_state());
        let missing_statuses = privileged_route_statuses(app.clone(), None).await;
        let wrong_statuses = privileged_route_statuses(app, Some("wrong-privileged-secret")).await;
        let disabled_app =
            build_router(make_unsecured_web_state().with_control_token(TEST_CONTROL_TOKEN));
        let disabled_statuses =
            privileged_route_statuses(disabled_app, Some(TEST_PRIVILEGED_TOKEN)).await;

        assert_eq!(missing_statuses, vec![StatusCode::FORBIDDEN; 6]);
        assert_eq!(wrong_statuses, vec![StatusCode::FORBIDDEN; 6]);
        assert_eq!(disabled_statuses, vec![StatusCode::FORBIDDEN; 6]);
    }

    #[tokio::test]
    async fn security_privileged_routes_accept_both_tokens() {
        let app = build_router(make_web_state());
        let statuses = privileged_route_statuses(app, Some(TEST_PRIVILEGED_TOKEN)).await;

        assert!(statuses
            .iter()
            .all(|status| !matches!(*status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)));
    }

    #[tokio::test]
    async fn api_rejects_cross_origin_browser_request() {
        let response = build_router(make_web_state())
            .oneshot(
                request_builder(Method::GET, "/api/healthz")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn terminal_websocket_reuses_cross_origin_policy() {
        let response = build_router(make_web_state())
            .oneshot(
                request_builder(Method::GET, "/api/terminal/sessions/missing/ws")
                    .header(header::ORIGIN, "https://evil.example")
                    .header(header::CONNECTION, "upgrade")
                    .header(header::UPGRADE, "websocket")
                    .header("sec-websocket-version", "13")
                    .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn configured_control_token_is_required_and_bearer_is_accepted() {
        let state = make_unsecured_web_state().with_control_token("test-secret");
        let app = build_router(state);
        let missing = app
            .clone()
            .oneshot(
                request_builder(Method::GET, "/api/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let accepted = app
            .oneshot(
                request_builder(Method::GET, "/api/healthz")
                    .header(header::AUTHORIZATION, "Bearer test-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cors_preflight_allows_only_same_origin_declared_surface() {
        let response = build_router(make_web_state())
            .oneshot(
                request_builder(Method::OPTIONS, "/api/healthz")
                    .header(header::ORIGIN, "http://127.0.0.1:17322")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&"http://127.0.0.1:17322".parse().unwrap())
        );
        let methods = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(!methods.contains('*'));
    }
}
