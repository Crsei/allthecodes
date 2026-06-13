//! Reusable web runtime response helpers owned outside the root binary.

use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use allthecodes_types::sdk::SdkMessage;
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::{Stream, StreamExt};

pub const WEB_UI_UNBUNDLED_MESSAGE: &str =
    "Web UI assets are not bundled in this build. This server provides API/WS endpoints only; start the standalone allthecodes-web frontend and connect it to this backend.";

pub const WEB_UI_MISSING_ASSETS_MESSAGE: &str =
    "Web UI assets not found. Build the sibling frontend first: cd ../allthecodes-web && npm install && npm run build";

pub fn normalize_static_path(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        "index.html"
    } else {
        path
    }
}

pub fn static_asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, mime.as_ref().to_string())],
        bytes,
    )
        .into_response()
}

pub fn static_html_response(bytes: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html".to_string())],
        bytes,
    )
        .into_response()
}

pub fn missing_static_assets_response() -> Response {
    (StatusCode::NOT_FOUND, WEB_UI_MISSING_ASSETS_MESSAGE).into_response()
}

pub fn unbundled_static_assets_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/plain".to_string())],
        WEB_UI_UNBUNDLED_MESSAGE,
    )
        .into_response()
}

pub fn sdk_stream_to_sse(
    stream: Pin<Box<dyn Stream<Item = SdkMessage> + Send>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let sse_stream = stream.map(|msg| {
        let event_name = msg.event_name();
        let data = serde_json::to_string(&msg)
            .unwrap_or_else(|error| format!(r#"{{"error":"serialization failed: {error}"}}"#));
        Ok(Event::default().event(event_name).data(data))
    });
    Sse::new(sse_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_static_path_uses_spa_index_for_root() {
        assert_eq!(normalize_static_path("/"), "index.html");
        assert_eq!(normalize_static_path(""), "index.html");
        assert_eq!(normalize_static_path("/assets/app.js"), "assets/app.js");
    }

    #[test]
    fn unbundled_static_response_uses_service_unavailable() {
        assert_eq!(
            unbundled_static_assets_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
