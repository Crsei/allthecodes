//! Web server module — Axum-based HTTP server for the chat UI.

pub mod api_dispatcher;
pub mod api_errors;
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

use axum::{routing::get, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::state::WebState;

/// Build the Axum router with all routes.
pub fn build_router(state: WebState) -> Router {
    let registry = handler_registry::all_api_handlers();
    let router = handler_registry::register_protocol_routes(Router::new(), &registry)
        .merge(web_state_routes::routes())
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
