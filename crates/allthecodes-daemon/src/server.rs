//! Daemon HTTP server startup.
//!
//! Binds to `127.0.0.1:{port}` and serves API routes, webhook stubs,
//! an SSE event stream, and a health endpoint.

use std::net::SocketAddr;

use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;

use super::{gateway_routes, routes, sse, state::DaemonState};

/// Build the Axum router with all daemon routes.
///
/// This is the canonical way to construct the daemon's route table.
/// Downstream code should call this function and then start the server
/// via [`allthecodes_server::ServerManager`].
pub fn build_router(state: DaemonState) -> Router {
    Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .merge(routes::team_memory_routes())
        .route("/health", axum::routing::get(routes::health))
        .route("/healthz", axum::routing::get(routes::healthz))
        .route("/readyz", axum::routing::get(routes::readyz))
        .route("/startupz", axum::routing::get(routes::startupz))
        .route("/events", axum::routing::get(sse::sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
        .merge(gateway_routes::gateway_routes())
}

/// Start the daemon HTTP server on the given port.
///
/// This is a legacy convenience wrapper around [`build_router`].
/// New code should use `build_router` + `allthecodes_server::ServerManager`.
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("daemon HTTP server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn make_daemon_state() -> DaemonState {
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
        DaemonState::new(engine, Arc::new(FeatureFlags::all_disabled()), 19836)
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
    async fn daemon_probe_endpoints_are_available() {
        let app = build_router(make_daemon_state());

        let (legacy_status, legacy_body) = get_json(app.clone(), "/health").await;
        assert_eq!(legacy_status, StatusCode::OK);
        assert_eq!(legacy_body, json!({ "status": "ok" }));

        for (uri, probe) in [
            ("/healthz", "healthz"),
            ("/readyz", "readyz"),
            ("/startupz", "startupz"),
        ] {
            let (status, body) = get_json(app.clone(), uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body["status"], json!("ok"));
            assert_eq!(body["probe"], json!(probe));
            assert_eq!(body["service"], json!("daemon"));
            assert!(body["pid"].as_u64().is_some());
            assert!(body["timestamp_ms"].as_u64().is_some());
        }
    }
}
