//! Embedded static file serving for the React SPA.
//!
//! Only compiles the embedded-asset path when the `web-ui` feature is enabled
//! (which pulls in `rust-embed`). Without the feature, the fallback responds
//! with a message explaining how to build the UI or enable the feature — the
//! Axum router stays valid either way.

use allthecodes_daemon::web::normalize_static_path;
#[cfg(not(feature = "web-ui"))]
use allthecodes_daemon::web::unbundled_static_assets_response;
#[cfg(feature = "web-ui")]
use allthecodes_daemon::web::{
    missing_static_assets_response, static_asset_response, static_html_response,
};
use axum::{http::Uri, response::IntoResponse};

#[cfg(feature = "web-ui")]
use rust_embed::Embed;

// The `folder` attribute is resolved relative to the crate's manifest dir.
// With the workspace split, the Next.js static export lives in the sibling
// frontend repository at `<workspace-parent>/allthecodes-web/dist`.
#[cfg(feature = "web-ui")]
#[derive(Embed)]
#[folder = "../../../allthecodes-web/dist"]
struct WebAssets;

/// Fallback handler: serve embedded static files or SPA index.html.
pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = normalize_static_path(uri.path());
    serve_embedded_file(path)
}

#[cfg(feature = "web-ui")]
fn serve_embedded_file(path: &str) -> axum::response::Response {
    match WebAssets::get(path) {
        Some(file) => static_asset_response(path, file.data.to_vec()),
        None => {
            // SPA fallback: return index.html for all non-file routes
            match WebAssets::get("index.html") {
                Some(index) => static_html_response(index.data.to_vec()),
                None => missing_static_assets_response(),
            }
        }
    }
}

#[cfg(not(feature = "web-ui"))]
fn serve_embedded_file(_path: &str) -> axum::response::Response {
    unbundled_static_assets_response()
}
