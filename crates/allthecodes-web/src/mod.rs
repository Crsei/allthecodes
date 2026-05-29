//! Web server module — Axum-based HTTP server for the chat UI.

pub mod handlers;
pub mod state;
pub mod static_files;
pub mod ws;

use std::net::SocketAddr;

use axum::{
    routing::{any, get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::state::WebState;

/// Build the Axum router with all routes.
pub fn build_router(state: WebState) -> Router {
    Router::new()
        // API routes
        .route("/api/chat", post(handlers::chat_handler))
        .route("/api/abort", post(handlers::abort_handler))
        .route("/api/state", get(handlers::state_handler))
        // Phase 3: Settings and command endpoints
        .route("/api/settings", post(handlers::settings_handler))
        .route("/api/command", post(handlers::command_handler))
        .route("/api/debug/state", get(handlers::debug_state_handler))
        .route(
            "/api/debug/sessions/{id}/trace",
            get(handlers::debug_session_trace_handler),
        )
        .route(
            "/api/debug/actions/{*action}",
            post(handlers::debug_action_handler),
        )
        // Phase 2 of the web UI overhaul: session management
        .route("/api/sessions", get(handlers::sessions_list_handler))
        .route("/api/sessions/new", post(handlers::session_new_handler))
        .route("/api/sessions/{id}", get(handlers::session_detail_handler))
        .route(
            "/api/sessions/{id}/resume",
            post(handlers::session_resume_handler),
        )
        // Phase 4: xterm.js TUI WebSocket bridge
        .route("/api/tui/ws", any(ws::tui::tui_ws_handler))
        // Phase 5: IPC WebSocket bridge for FrontendMessage/BackendMessage
        .route("/api/ipc/ws", any(ws::ipc::ipc_ws_handler))
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
