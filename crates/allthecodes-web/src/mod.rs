//! Web server module — Axum-based HTTP server for the chat UI.

pub mod handlers;
pub mod state;
pub mod static_files;
pub mod workspace_metadata;
pub mod ws;

use std::net::SocketAddr;

use axum::{
    Router,
    routing::{any, delete, get, patch, post, put},
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
        .route("/api/capabilities", get(handlers::capabilities_handler))
        .route("/api/chat-modes", get(handlers::chat_modes_list_handler))
        .route(
            "/api/chat-modes/resources",
            get(handlers::chat_modes_resources_handler),
        )
        .route(
            "/api/chat-modes/{id}",
            put(handlers::chat_modes_upsert_handler).delete(handlers::chat_modes_delete_handler),
        )
        .route(
            "/api/agents",
            get(handlers::agents_list_handler).post(handlers::agents_create_handler),
        )
        .route(
            "/api/agents/{name}",
            get(handlers::agents_detail_handler)
                .patch(handlers::agents_update_handler)
                .delete(handlers::agents_delete_handler),
        )
        .route(
            "/api/agents/{name}/restore",
            post(handlers::agents_restore_handler),
        )
        .route(
            "/api/people",
            get(handlers::people_list_handler).post(handlers::people_create_handler),
        )
        .route(
            "/api/people/{id}",
            get(handlers::people_detail_handler)
                .patch(handlers::people_update_handler)
                .delete(handlers::people_delete_handler),
        )
        .route(
            "/api/hooks",
            get(handlers::hooks_list_handler).post(handlers::hooks_create_handler),
        )
        .route("/api/hooks/test", post(handlers::hooks_test_handler))
        .route(
            "/api/hooks/{event}",
            get(handlers::hooks_detail_handler)
                .patch(handlers::hooks_update_handler)
                .delete(handlers::hooks_delete_handler),
        )
        .route(
            "/api/prompts",
            get(handlers::prompts_list_handler).post(handlers::prompts_create_handler),
        )
        .route(
            "/api/prompts/{id}",
            get(handlers::prompts_detail_handler)
                .patch(handlers::prompts_update_handler)
                .delete(handlers::prompts_delete_handler),
        )
        .route(
            "/api/mcp-servers",
            get(handlers::mcp_servers_list_handler).post(handlers::mcp_servers_create_handler),
        )
        .route(
            "/api/mcp-servers/marketplace",
            get(handlers::mcp_servers_marketplace_handler),
        )
        .route(
            "/api/mcp-servers/{name}",
            get(handlers::mcp_servers_detail_handler)
                .patch(handlers::mcp_servers_update_handler)
                .delete(handlers::mcp_servers_delete_handler),
        )
        .route("/api/plugins", get(handlers::plugins_list_handler))
        .route(
            "/api/plugins/marketplace",
            get(handlers::plugins_marketplace_handler),
        )
        .route(
            "/api/plugins/install",
            post(handlers::plugins_install_handler),
        )
        .route(
            "/api/plugins/{id}/uninstall",
            post(handlers::plugins_uninstall_handler),
        )
        .route("/api/channels", get(handlers::channels_list_handler))
        .route(
            "/api/channels/capabilities",
            get(handlers::channels_capabilities_handler),
        )
        .route(
            "/api/channels/{provider}/connect",
            post(handlers::channels_connect_handler),
        )
        .route(
            "/api/channels/{provider}/test",
            post(handlers::channels_test_handler),
        )
        .route(
            "/api/computer-use/status",
            get(handlers::computer_use_status_handler),
        )
        .route(
            "/api/computer-use/permissions/{permission}/request",
            post(handlers::computer_use_permission_request_handler),
        )
        .route(
            "/api/computer-use/test",
            post(handlers::computer_use_test_handler),
        )
        .route(
            "/api/appshots/status",
            get(handlers::appshots_status_handler),
        )
        .route(
            "/api/appshots/capture",
            post(handlers::appshots_capture_handler),
        )
        .route(
            "/api/chrome-relay/status",
            get(handlers::chrome_relay_status_handler),
        )
        .route(
            "/api/chrome-relay/launch",
            post(handlers::chrome_relay_launch_handler),
        )
        .route(
            "/api/chrome-relay/token/regenerate",
            post(handlers::chrome_relay_token_regenerate_handler),
        )
        .route(
            "/api/activity-recorder/status",
            get(handlers::activity_recorder_status_handler),
        )
        .route(
            "/api/activity-recorder/sessions",
            get(handlers::activity_recorder_sessions_handler),
        )
        .route(
            "/api/activity-recorder/clear",
            post(handlers::activity_recorder_clear_handler),
        )
        // Phase 3: Settings and command endpoints
        .route("/api/settings", post(handlers::settings_handler))
        .route("/api/command", post(handlers::command_handler))
        .route(
            "/api/memory/config",
            get(handlers::memory_config_get_handler).patch(handlers::memory_config_patch_handler),
        )
        .route("/api/speech/models", get(handlers::speech_models_handler))
        .route(
            "/api/speech/models/download",
            post(handlers::speech_model_download_handler),
        )
        .route(
            "/api/speech/models/{id}",
            delete(handlers::speech_model_delete_handler),
        )
        .route(
            "/api/search/cookies/export",
            post(handlers::search_cookies_export_handler),
        )
        .route(
            "/api/search/cookies/import",
            post(handlers::search_cookies_import_handler),
        )
        .route(
            "/api/search/cookies/clear",
            post(handlers::search_cookies_clear_handler),
        )
        .route("/api/data/export", post(handlers::data_export_handler))
        .route("/api/data/import", post(handlers::data_import_handler))
        .route("/api/token-savings", get(handlers::token_savings_handler))
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
        .route(
            "/api/sessions/{id}/archive",
            post(handlers::session_archive_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/branch",
            post(handlers::session_message_branch_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/feedback",
            post(handlers::session_message_feedback_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/delete",
            post(handlers::session_message_delete_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/regenerate/prepare",
            post(handlers::session_message_regenerate_prepare_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/edit/prepare",
            post(handlers::session_message_edit_prepare_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/rollback/preview",
            post(handlers::session_message_rollback_preview_handler),
        )
        .route(
            "/api/sessions/{id}/messages/{message_id}/rollback",
            post(handlers::session_message_rollback_handler),
        )
        .route("/api/workspaces", get(handlers::workspaces_list_handler))
        .route(
            "/api/workspaces/{workspace_key}",
            patch(handlers::workspace_patch_handler),
        )
        .route(
            "/api/workspaces/{workspace_key}/open",
            post(handlers::workspace_open_handler),
        )
        .route(
            "/api/workspaces/{workspace_key}/sessions/archive",
            post(handlers::workspace_sessions_archive_handler),
        )
        // Auth endpoints
        .route("/api/auth/status", get(handlers::auth_status_handler))
        .route("/api/auth/login", post(handlers::auth_login_handler))
        .route("/api/auth/logout", post(handlers::auth_logout_handler))
        .route("/api/auth/refresh", post(handlers::auth_refresh_handler))
        // Profile endpoints
        .route(
            "/api/profiles",
            get(handlers::profiles_list_handler).post(handlers::profiles_create_handler),
        )
        .route(
            "/api/profiles/import",
            post(handlers::profiles_import_handler),
        )
        .route(
            "/api/profiles/{id}",
            get(handlers::profiles_detail_handler)
                .patch(handlers::profiles_update_handler)
                .delete(handlers::profiles_delete_handler),
        )
        .route(
            "/api/profiles/{id}/switch",
            post(handlers::profiles_switch_handler),
        )
        .route(
            "/api/profiles/{id}/export",
            get(handlers::profiles_export_handler),
        )
        // Provider endpoints
        .route(
            "/api/providers",
            get(handlers::providers_list_handler).post(handlers::providers_create_handler),
        )
        .route(
            "/api/providers/{id}",
            patch(handlers::providers_update_handler).delete(handlers::providers_delete_handler),
        )
        .route(
            "/api/providers/{id}/models/refresh",
            post(handlers::providers_refresh_models_handler),
        )
        // Model endpoints
        .route("/api/models", get(handlers::models_list_handler))
        .route("/api/models/{id}", patch(handlers::models_update_handler))
        .route(
            "/api/models/default",
            post(handlers::models_set_default_handler),
        )
        // Credential & OAuth endpoints
        .route("/api/credentials", get(handlers::credentials_handler))
        .route(
            "/api/oauth/{provider}/start",
            post(handlers::oauth_start_handler),
        )
        .route(
            "/api/oauth/{provider}/poll",
            post(handlers::oauth_poll_handler),
        )
        // Phase 4: xterm.js TUI WebSocket bridge
        .route("/api/tui/ws", any(ws::tui::tui_ws_handler))
        // Phase 5: IPC WebSocket bridge for FrontendMessage/BackendMessage
        .route("/api/ipc/ws", any(ws::ipc::ipc_ws_handler))
        // Right sidebar: git history, file changes, and web preview proxy
        .route("/api/git/log", get(handlers::git_log_handler))
        .route("/api/git/diff", get(handlers::git_diff_handler))
        .route("/api/proxy", get(handlers::proxy_handler))
        // API catch-all: unregistered /api/* paths return JSON 501
        .route(
            "/api/{*path}",
            get(handlers::api_fallback_handler).post(handlers::api_fallback_handler),
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
