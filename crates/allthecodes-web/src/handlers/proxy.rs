//! HTTP proxy endpoint used by the web preview right sidebar tab.

use std::net::IpAddr;
use std::time::Duration;

use axum::body::Body;
use axum::extract::Query;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use reqwest::Url;
use serde::Deserialize;

const MAX_REDIRECTS: usize = 10;
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
pub struct ProxyParams {
    pub url: String,
}

/// GET /api/proxy?url=https://example.com -- Fetch an external page for preview.
pub async fn proxy_handler(Query(params): Query<ProxyParams>) -> Response {
    let url = match Url::parse(params.url.trim()) {
        Ok(url) => url,
        Err(_) => return text_error(StatusCode::BAD_REQUEST, "Invalid URL"),
    };

    let client = match reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return text_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create proxy client: {err}"),
            );
        }
    };

    let response = match fetch_with_checked_redirects(&client, url).await {
        Ok(response) => response,
        Err(err) => return err.into_response(),
    };

    if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES as u64 {
        return text_error(StatusCode::PAYLOAD_TOO_LARGE, "Response too large");
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("text/html; charset=utf-8"));

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                return text_error(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to read proxied response: {err}"),
                );
            }
        };

        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return text_error(StatusCode::PAYLOAD_TOO_LARGE, "Response too large");
        }
        body.extend_from_slice(&chunk);
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        .body(Body::from(body))
        .unwrap_or_else(|err| text_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn fetch_with_checked_redirects(
    client: &reqwest::Client,
    mut url: Url,
) -> Result<reqwest::Response, ProxyError> {
    for redirects in 0..=MAX_REDIRECTS {
        validate_proxy_url(&url).await?;

        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|err| ProxyError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;

        if !response.status().is_redirection() {
            return Ok(response);
        }

        let Some(location) = response.headers().get(header::LOCATION) else {
            return Ok(response);
        };
        let location = location.to_str().map_err(|_| {
            ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Redirect location is not valid UTF-8",
            )
        })?;

        if redirects == MAX_REDIRECTS {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Too many redirects",
            ));
        }

        url = url
            .join(location)
            .map_err(|_| ProxyError::new(StatusCode::BAD_GATEWAY, "Invalid redirect URL"))?;
    }

    Err(ProxyError::new(
        StatusCode::BAD_GATEWAY,
        "Too many redirects",
    ))
}

async fn validate_proxy_url(url: &Url) -> Result<(), ProxyError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Only http/https URLs allowed",
        ));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Credentials in proxy URLs are not allowed",
        ));
    }

    let Some(host) = url.host_str() else {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "URL host is required",
        ));
    };

    let host_lc = host.trim_end_matches('.').to_ascii_lowercase();
    if host_lc == "localhost" || host_lc.ends_with(".localhost") {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Private network hosts are not allowed",
        ));
    }

    if let Ok(addr) = host.parse::<IpAddr>() {
        if is_blocked_ip(addr) {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Private network hosts are not allowed",
            ));
        }
        return Ok(());
    }

    let port = url.port_or_known_default().ok_or_else(|| {
        ProxyError::new(StatusCode::BAD_REQUEST, "URL port could not be resolved")
    })?;

    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| ProxyError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;

    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Private network hosts are not allowed",
            ));
        }
    }

    Ok(())
}

fn is_blocked_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => {
            addr.is_loopback()
                || addr.is_private()
                || addr.is_link_local()
                || addr.is_unspecified()
                || addr.is_broadcast()
                || addr.is_documentation()
        }
        IpAddr::V6(addr) => {
            addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_unique_local()
                || addr.is_unicast_link_local()
        }
    }
}

struct ProxyError {
    status: StatusCode,
    message: String,
}

impl ProxyError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn into_response(self) -> Response {
        text_error(self.status, self.message)
    }
}

fn text_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, message.into()).into_response()
}
