//! Daemon HTTP server startup.
//!
//! Binds to `127.0.0.1:{port}` and serves API routes, webhook stubs,
//! an SSE event stream, and a health endpoint.

use std::net::SocketAddr;

use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;

use super::{gateway_routes, routes, sse, state::DaemonState};

/// Start the daemon HTTP server on the given port.
///
/// This function runs until the server shuts down (i.e. it awaits
/// indefinitely).  Call it from within a `tokio::spawn` or similar.
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .merge(routes::team_memory_routes())
        .route("/health", axum::routing::get(routes::health))
        .route("/events", axum::routing::get(sse::sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
        .merge(gateway_routes::gateway_routes());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("daemon HTTP server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
