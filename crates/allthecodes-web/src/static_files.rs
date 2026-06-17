//! Static file fallback for backend-only web builds.

use allthecodes_daemon::web::unbundled_static_assets_response;
use axum::{http::Uri, response::IntoResponse};

/// Fallback handler for static file requests.
pub async fn static_handler(_uri: Uri) -> impl IntoResponse {
    unbundled_static_assets_response()
}
