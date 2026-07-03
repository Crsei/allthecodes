//! Web server module — Axum-based HTTP server for the chat UI.

pub mod api_dispatcher;
pub mod api_errors;
pub(crate) mod api_operation_registry;
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
    routing::{get, patch},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::state::WebState;

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
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Start the web server on the given port.
pub async fn start_server(state: WebState, port: u16, no_open: bool) -> anyhow::Result<()> {
    let app = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

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
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tower::ServiceExt;

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

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
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

    #[tokio::test]
    async fn root_probe_endpoints_are_available() {
        let app = build_router(make_web_state());

        for (uri, probe) in [
            ("/healthz", "healthz"),
            ("/readyz", "readyz"),
            ("/startupz", "startupz"),
        ] {
            let (status, body) = get_json(app.clone(), uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body["status"], json!("ok"));
            assert_eq!(body["probe"], json!(probe));
            assert_eq!(body["service"], json!("web"));
            assert!(body["pid"].as_u64().is_some());
            assert!(body["timestamp_ms"].as_u64().is_some());
        }
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
}
