use std::io;

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::StatusCode;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use url::Url;

use super::super::McpRuntimeContext;
use super::super::McpServerConfig;
use super::auth_error::McpAuthNeededError;

use allthecodes_types::mcp::CONNECT_TIMEOUT_SECS;

// ---------------------------------------------------------------------------
// Config validation
// ---------------------------------------------------------------------------

pub(super) fn validate_sse_config(config: &McpServerConfig) -> Result<()> {
    let url = config
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("sse transport requires 'url' field"))?;
    validate_sse_url(url)?;

    if let Some(headers) = &config.headers {
        for (name, value) in headers {
            validate_header_name(name)?;
            validate_header_value(name, value)?;
        }
    }

    Ok(())
}

pub(super) fn validate_streamable_http_config(config: &McpServerConfig) -> Result<()> {
    let url = config
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("streamable-http transport requires 'url' field"))?;
    validate_streamable_http_url(url)?;

    if let Some(headers) = &config.headers {
        for (name, value) in headers {
            validate_header_name(name)?;
            validate_header_value(name, value)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP client construction
// ---------------------------------------------------------------------------

pub(super) fn streamable_http_client(target: &Url) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
    if is_loopback_url(target) {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .context("failed to build MCP Streamable HTTP client")
}

// ---------------------------------------------------------------------------
// Header utilities
// ---------------------------------------------------------------------------

pub(super) fn reqwest_header_map(headers: &[(String, String)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid MCP HTTP header name '{}'", name))?;
        let header_value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid MCP HTTP header value for '{}'", name))?;
        map.insert(header_name, header_value);
    }
    Ok(map)
}

pub(super) fn reqwest_error_to_io(error: reqwest::Error) -> io::Error {
    io::Error::other(error)
}

// ---------------------------------------------------------------------------
// URL / authority parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_authority(authority: &str) -> Result<(String, u16)> {
    if let Some(after_bracket) = authority.strip_prefix('[') {
        let (host, rest) = after_bracket
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("invalid IPv6 host in SSE URL"))?;
        let port = if let Some(port) = rest.strip_prefix(':') {
            parse_port(port)?
        } else if rest.is_empty() {
            80
        } else {
            bail!("invalid IPv6 authority in SSE URL");
        };
        return Ok((host.to_string(), port));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, parse_port(port)?)
        }
        _ => (authority, 80),
    };

    Ok((host.to_string(), port))
}

pub(super) fn parse_port(port: &str) -> Result<u16> {
    port.parse::<u16>()
        .with_context(|| format!("invalid port '{}' in SSE URL", port))
}

pub(super) fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub(super) fn strip_fragment(value: &str) -> &str {
    value.split('#').next().unwrap_or(value)
}

// ---------------------------------------------------------------------------
// Header normalization
// ---------------------------------------------------------------------------

pub(super) fn normalized_http_transport_headers(config: &McpServerConfig) -> Vec<(String, String)> {
    let mut headers = config
        .headers
        .as_ref()
        .map(|headers| {
            headers
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    headers.sort_by(|(left, _), (right, _)| left.cmp(right));
    headers
}

pub(super) async fn normalized_http_transport_headers_with_auth(
    config: &McpServerConfig,
) -> Result<Vec<(String, String)>> {
    let mut headers = normalized_http_transport_headers(config);
    let has_explicit_authorization = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
    if !has_explicit_authorization {
        if let Some(value) = super::super::auth::authorization_header(config).await? {
            headers.push(("Authorization".to_string(), value));
            headers.sort_by(|(left, _), (right, _)| left.cmp(right));
        }
    }
    Ok(headers)
}

// ---------------------------------------------------------------------------
// Raw HTTP request building (loopback SSE)
// ---------------------------------------------------------------------------

pub(super) fn build_sse_get_request(
    target: &super::sse::SseHttpTarget,
    headers: &[(String, String)],
) -> String {
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n",
        target.path_and_query, target.authority
    );
    append_user_headers(&mut request, headers);
    request.push_str("\r\n");
    request
}

pub(super) fn build_sse_post_request(
    target: &super::sse::SseHttpTarget,
    headers: &[(String, String)],
    body: &str,
) -> String {
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        target.path_and_query,
        target.authority,
        body.len()
    );
    append_user_headers(&mut request, headers);
    request.push_str("\r\n");
    request.push_str(body);
    request
}

fn append_user_headers(request: &mut String, headers: &[(String, String)]) {
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
}

pub(super) async fn read_http_response_head(reader: &mut BufReader<TcpStream>) -> Result<u16> {
    let mut status_line = String::new();
    if reader
        .read_line(&mut status_line)
        .await
        .context("failed to read HTTP status line")?
        == 0
    {
        bail!("HTTP server closed connection before status line");
    }

    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP status line: {}", status_line.trim()))?
        .parse::<u16>()
        .with_context(|| format!("invalid HTTP status line: {}", status_line.trim()))?;

    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .await
            .context("failed to read HTTP response header")?
            == 0
        {
            bail!("HTTP server closed connection before response headers completed");
        }
        if line.trim_end_matches(['\r', '\n']).is_empty() {
            break;
        }
    }

    Ok(status)
}

// ---------------------------------------------------------------------------
// Status code handling
// ---------------------------------------------------------------------------

pub(super) fn handle_sse_event_stream_status(status: StatusCode, server_name: &str) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(McpAuthNeededError {
            server_name: server_name.to_string(),
            status,
        }
        .into());
    }
    if status.is_redirection() {
        bail!(
            "MCP SSE server '{}' returned HTTP {}; redirects are disabled for SSE transport",
            server_name,
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!(
            "MCP SSE server '{}' returned HTTP {} for event stream",
            server_name,
            status.as_u16()
        );
    }
    Ok(())
}

pub(super) fn handle_sse_post_status(
    status: StatusCode,
    server_name: &str,
    runtime: &McpRuntimeContext,
) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        runtime.emit_event(super::super::McpSubsystemEvent::ServerStateChanged {
            server_name: server_name.to_string(),
            state: "auth-needed".to_string(),
            error: Some(
                McpAuthNeededError {
                    server_name: server_name.to_string(),
                    status,
                }
                .to_string(),
            ),
        });
        return Err(McpAuthNeededError {
            server_name: server_name.to_string(),
            status,
        }
        .into());
    }
    if status.is_redirection() {
        bail!(
            "MCP SSE server '{}' returned HTTP {}; redirects are disabled for JSON-RPC POST",
            server_name,
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!(
            "MCP SSE server '{}' returned HTTP {} for JSON-RPC POST",
            server_name,
            status.as_u16()
        );
    }
    Ok(())
}

pub(super) fn handle_streamable_http_status(
    status: StatusCode,
    server_name: &str,
    runtime: &McpRuntimeContext,
) -> Result<()> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        runtime.emit_event(super::super::McpSubsystemEvent::ServerStateChanged {
            server_name: server_name.to_string(),
            state: "auth-needed".to_string(),
            error: Some(
                McpAuthNeededError {
                    server_name: server_name.to_string(),
                    status,
                }
                .to_string(),
            ),
        });
        return Err(McpAuthNeededError {
            server_name: server_name.to_string(),
            status,
        }
        .into());
    }
    if status.is_redirection() {
        bail!(
            "MCP Streamable HTTP server '{}' returned HTTP {}; redirects are disabled",
            server_name,
            status.as_u16()
        );
    }
    if !status.is_success() {
        bail!(
            "MCP Streamable HTTP server '{}' returned HTTP {}",
            server_name,
            status.as_u16()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// URL validation
// ---------------------------------------------------------------------------

pub(super) fn validate_sse_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed != url {
        bail!("sse url must be a non-empty URL without surrounding whitespace");
    }
    if trimmed.starts_with("https://") {
        let parsed = Url::parse(trimmed).context("invalid remote SSE URL")?;
        validate_remote_https_url(&parsed)?;
        return Ok(());
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        let host_port = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        let host = host_port
            .strip_prefix('[')
            .and_then(|value| value.split(']').next())
            .or_else(|| host_port.split(':').next())
            .unwrap_or_default();
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Ok(());
        }
        bail!("sse transport requires https URLs unless the host is loopback");
    }
    bail!("sse transport requires an http:// or https:// URL");
}

pub(super) fn validate_streamable_http_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed != url {
        bail!("streamable-http url must be a non-empty URL without surrounding whitespace");
    }
    let parsed = Url::parse(trimmed).context("invalid Streamable HTTP URL")?;
    match parsed.scheme() {
        "https" => validate_remote_https_url(&parsed),
        "http" if is_loopback_url(&parsed) => Ok(()),
        "http" => {
            bail!("streamable-http transport requires https URLs unless the host is loopback")
        }
        _ => bail!("streamable-http transport requires an http:// or https:// URL"),
    }
}

fn validate_header_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| matches!(b, b'!' | b'#'..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'0'..=b'9' | b'A'..=b'Z' | b'^' | b'_' | b'`' | b'a'..=b'z' | b'|' | b'~'))
    {
        bail!("invalid SSE header name '{}'", name);
    }
    if is_reserved_http_transport_header(name) {
        bail!(
            "unsafe MCP HTTP header '{}' is managed by the MCP transport",
            name
        );
    }
    Ok(())
}

pub(super) fn validate_header_value(name: &str, value: &str) -> Result<()> {
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("invalid SSE header value for '{}'", name);
    }
    Ok(())
}

pub(super) fn validate_remote_https_url(url: &Url) -> Result<()> {
    if url.scheme() != "https" {
        bail!("remote MCP HTTP transport requires https URLs");
    }
    if url.host_str().is_none() {
        bail!("remote MCP HTTP URL must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("remote MCP HTTP URL must not include embedded credentials");
    }
    Ok(())
}

pub(super) fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("invalid MCP-Session-Id header value");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// URL comparison / helpers
// ---------------------------------------------------------------------------

pub(super) fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

pub(super) fn is_loopback_url(url: &Url) -> bool {
    url.host_str().map(is_loopback_host).unwrap_or(false)
}

fn is_reserved_http_transport_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept"
            | "connection"
            | "content-length"
            | "content-type"
            | "host"
            | "mcp-protocol-version"
            | "mcp-session-id"
            | "transfer-encoding"
    )
}

pub(super) fn is_event_stream_response(headers: &HeaderMap) -> bool {
    response_content_type(headers)
        .map(|content_type| content_type.eq_ignore_ascii_case("text/event-stream"))
        .unwrap_or(false)
}

fn response_content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
}

pub(super) fn redact_url_for_log(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            if parsed.query().is_some() {
                parsed.set_query(Some("redacted"));
            }
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => strip_fragment(url)
            .split('?')
            .next()
            .unwrap_or(url)
            .to_string(),
    }
}
