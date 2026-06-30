use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use reqwest::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, USER_AGENT};
use reqwest::StatusCode;
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex};
use tokio_util::io::StreamReader;
use url::Url;

use allthecodes_types::mcp::CONNECT_TIMEOUT_SECS;

use super::super::transport::sse_reader_loop;
use super::super::{McpRuntimeContext, SharedMcpEventSink};

use super::http_utils::{
    build_sse_get_request, build_sse_post_request, handle_sse_event_stream_status,
    handle_sse_post_status, is_loopback_host, parse_authority, read_http_response_head,
    reqwest_error_to_io, reqwest_header_map, same_origin, strip_fragment,
    validate_remote_https_url, validate_sse_url, www_authenticate_challenge,
};

// ---------------------------------------------------------------------------
// SSE connection / post targets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) enum SseConnectTarget {
    Loopback(SseHttpTarget),
    RemoteHttps(RemoteSseHttpTarget),
}

impl SseConnectTarget {
    pub(super) fn parse(url: &str) -> Result<Self> {
        validate_sse_url(url)?;
        if url.starts_with("https://") {
            return Ok(Self::RemoteHttps(RemoteSseHttpTarget::parse(url)?));
        }
        Ok(Self::Loopback(SseHttpTarget::parse_loopback(url)?))
    }

    pub(super) fn resolve_endpoint(&self, endpoint: &str) -> Result<SsePostTarget> {
        match self {
            Self::Loopback(target) => target
                .resolve_endpoint(endpoint)
                .map(SsePostTarget::Loopback),
            Self::RemoteHttps(target) => target
                .resolve_endpoint(endpoint)
                .map(SsePostTarget::RemoteHttps),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum SsePostTarget {
    Loopback(SseHttpTarget),
    RemoteHttps(RemoteSseHttpTarget),
}

#[derive(Debug, Clone)]
pub(super) struct SseHttpTarget {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) authority: String,
    pub(super) path_and_query: String,
}

impl SseHttpTarget {
    fn parse_loopback(url: &str) -> Result<Self> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| anyhow::anyhow!("loopback SSE transport requires an http:// URL"))?;
        let rest = rest.split('#').next().unwrap_or(rest);
        let split_at = rest.find(['/', '?']).unwrap_or(rest.len());
        let authority = &rest[..split_at];
        if authority.is_empty() {
            bail!("sse url must include a host");
        }
        let suffix = &rest[split_at..];
        let path_and_query = if suffix.is_empty() {
            "/".to_string()
        } else if suffix.starts_with('/') {
            suffix.to_string()
        } else {
            format!("/{}", suffix)
        };

        let (host, port) = parse_authority(authority)?;
        if !is_loopback_host(&host) {
            bail!("sse transport requires https URLs unless the host is loopback");
        }

        Ok(Self {
            host,
            port,
            authority: authority.to_string(),
            path_and_query,
        })
    }

    fn socket_addr(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn resolve_endpoint(&self, endpoint: &str) -> Result<Self> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            bail!("MCP SSE endpoint event was empty");
        }
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return Self::parse_loopback(endpoint);
        }
        if endpoint.starts_with('/') {
            let mut target = self.clone();
            target.path_and_query = strip_fragment(endpoint).to_string();
            return Ok(target);
        }
        bail!(
            "MCP SSE endpoint event must be an absolute loopback URL or absolute path, got '{}'",
            endpoint
        );
    }
}

#[derive(Debug, Clone)]
pub(super) struct RemoteSseHttpTarget {
    url: Url,
}

impl RemoteSseHttpTarget {
    #[cfg(test)]
    pub(super) fn url(&self) -> &Url {
        &self.url
    }

    fn parse(url: &str) -> Result<Self> {
        let mut url = Url::parse(url).context("invalid remote SSE URL")?;
        validate_remote_https_url(&url)?;
        url.set_fragment(None);
        Ok(Self { url })
    }

    fn resolve_endpoint(&self, endpoint: &str) -> Result<Self> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            bail!("MCP SSE endpoint event was empty");
        }

        let mut resolved = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            Url::parse(endpoint).context("invalid MCP SSE endpoint event URL")?
        } else {
            self.url
                .join(endpoint)
                .context("invalid MCP SSE endpoint event path")?
        };
        resolved.set_fragment(None);
        validate_remote_https_url(&resolved)?;
        if !same_origin(&self.url, &resolved) {
            bail!("MCP SSE endpoint event must stay on the same https origin");
        }
        Ok(Self { url: resolved })
    }
}

// ---------------------------------------------------------------------------
// SSE HTTP sender
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct SseHttpSender {
    pub(super) target: SsePostTarget,
    pub(super) headers: Vec<(String, String)>,
    pub(super) server_name: String,
    pub(super) http_client: Option<reqwest::Client>,
    pub(super) runtime: McpRuntimeContext,
}

impl SseHttpSender {
    pub(super) fn set_event_sink(&mut self, event_sink: Option<SharedMcpEventSink>) {
        self.runtime.set_event_sink(event_sink);
    }

    pub(super) async fn post_json(&self, body: &str) -> Result<()> {
        match &self.target {
            SsePostTarget::Loopback(target) => {
                post_loopback_sse_json(
                    target,
                    &self.headers,
                    &self.server_name,
                    body,
                    &self.runtime,
                )
                .await
            }
            SsePostTarget::RemoteHttps(target) => {
                let http_client = self.http_client.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "remote SSE HTTP client missing for server '{}'",
                        self.server_name
                    )
                })?;
                post_remote_https_sse_json(
                    http_client,
                    target,
                    &self.headers,
                    &self.server_name,
                    body,
                    &self.runtime,
                )
                .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SSE POST helpers
// ---------------------------------------------------------------------------

async fn post_loopback_sse_json(
    target: &SseHttpTarget,
    headers: &[(String, String)],
    server_name: &str,
    body: &str,
    runtime: &McpRuntimeContext,
) -> Result<()> {
    let mut stream = TcpStream::connect(target.socket_addr())
        .await
        .with_context(|| {
            format!(
                "failed to connect to MCP SSE endpoint for server '{}'",
                server_name
            )
        })?;
    let request = build_sse_post_request(target, headers, body);
    stream
        .write_all(request.as_bytes())
        .await
        .context("failed to send MCP SSE POST request")?;
    stream
        .flush()
        .await
        .context("failed to flush MCP SSE POST request")?;

    let mut reader = BufReader::new(stream);
    let status = read_http_response_head(&mut reader)
        .await
        .context("failed to read MCP SSE POST response headers")?;
    handle_sse_post_status(StatusCode::from_u16(status)?, server_name, runtime, None)?;

    Ok(())
}

async fn post_remote_https_sse_json(
    http_client: &reqwest::Client,
    target: &RemoteSseHttpTarget,
    headers: &[(String, String)],
    server_name: &str,
    body: &str,
    runtime: &McpRuntimeContext,
) -> Result<()> {
    let request = http_client
        .post(target.url.clone())
        .header(CONTENT_TYPE, "application/json")
        .headers(reqwest_header_map(headers)?)
        .body(body.to_string());
    let response = request.send().await.with_context(|| {
        format!(
            "failed to POST MCP SSE JSON-RPC for server '{}'",
            server_name
        )
    })?;
    let challenge = www_authenticate_challenge(response.headers(), response.status());
    handle_sse_post_status(response.status(), server_name, runtime, challenge)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// SSE stream connection
// ---------------------------------------------------------------------------

pub(super) async fn connect_loopback_sse_stream(
    target: &SseHttpTarget,
    headers: &[(String, String)],
    server_name: &str,
) -> Result<BufReader<TcpStream>> {
    let mut stream = TcpStream::connect(target.socket_addr())
        .await
        .with_context(|| format!("failed to connect to MCP SSE server '{}'", server_name))?;
    let request = build_sse_get_request(target, headers);
    stream
        .write_all(request.as_bytes())
        .await
        .context("failed to send MCP SSE GET request")?;
    stream
        .flush()
        .await
        .context("failed to flush MCP SSE GET request")?;

    let mut reader = BufReader::new(stream);
    let status = read_http_response_head(&mut reader)
        .await
        .context("failed to read MCP SSE response headers")?;
    handle_sse_event_stream_status(StatusCode::from_u16(status)?, server_name, None)?;

    Ok(reader)
}

pub(super) async fn connect_remote_https_sse_stream(
    http_client: &reqwest::Client,
    target: &RemoteSseHttpTarget,
    headers: &[(String, String)],
    server_name: &str,
) -> Result<
    BufReader<
        StreamReader<
            impl futures::Stream<Item = io::Result<bytes::Bytes>> + Send + 'static,
            bytes::Bytes,
        >,
    >,
> {
    let response = http_client
        .get(target.url.clone())
        .header(ACCEPT, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .header(USER_AGENT, allthecodes_config::user_agent::mcp_user_agent())
        .headers(reqwest_header_map(headers)?)
        .send()
        .await
        .with_context(|| format!("failed to connect to MCP SSE server '{}'", server_name))?;
    let challenge = www_authenticate_challenge(response.headers(), response.status());
    handle_sse_event_stream_status(response.status(), server_name, challenge)?;
    if let Some(content_type) = response.headers().get(CONTENT_TYPE) {
        let content_type = content_type.to_str().unwrap_or_default();
        if !content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("text/event-stream")
        {
            bail!(
                "MCP SSE server '{}' returned non-SSE content type '{}'",
                server_name,
                content_type
            );
        }
    }

    let body_stream = response.bytes_stream().map_err(reqwest_error_to_io);
    Ok(BufReader::new(StreamReader::new(body_stream)))
}

pub(super) fn remote_sse_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .build()
        .context("failed to build remote MCP SSE HTTP client")
}

// ---------------------------------------------------------------------------
// SSE reader + endpoint helpers
// ---------------------------------------------------------------------------

pub(super) fn spawn_sse_reader<R>(
    reader: R,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    server_name: String,
    runtime: McpRuntimeContext,
) -> (
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<Result<String>>,
)
where
    R: tokio::io::AsyncBufRead + Unpin + Send + 'static,
{
    let (endpoint_tx, endpoint_rx) = oneshot::channel();
    let reader_handle = tokio::spawn(async move {
        sse_reader_loop(reader, pending, server_name, Some(endpoint_tx), runtime).await;
    });
    (reader_handle, endpoint_rx)
}

pub(super) async fn receive_sse_endpoint(
    server_name: &str,
    reader_handle: &tokio::task::JoinHandle<()>,
    endpoint_rx: oneshot::Receiver<Result<String>>,
) -> Result<String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        endpoint_rx,
    )
    .await
    {
        Ok(Ok(Ok(endpoint))) => Ok(endpoint),
        Ok(Ok(Err(e))) => {
            reader_handle.abort();
            Err(e)
        }
        Ok(Err(_)) => {
            reader_handle.abort();
            bail!(
                "MCP SSE server '{}' closed before sending endpoint event",
                server_name
            );
        }
        Err(_) => {
            reader_handle.abort();
            bail!(
                "MCP SSE server '{}' did not send endpoint event within {}s",
                server_name,
                CONNECT_TIMEOUT_SECS
            );
        }
    }
}
