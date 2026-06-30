use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use reqwest::header::{HeaderMap, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::StatusCode;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio_util::io::StreamReader;
use tracing::debug;
use url::Url;

use super::super::transport::{
    dispatch_response, notification_event, streamable_http_sse_reader_loop,
};
use super::super::{JsonRpcResponse, McpRuntimeContext, SharedMcpEventSink};

use super::http_utils::{
    handle_streamable_http_status, is_event_stream_response, reqwest_error_to_io,
    reqwest_header_map, validate_header_value, validate_session_id, www_authenticate_challenge,
};

use super::PendingRequests;

const STREAMABLE_HTTP_PROTOCOL_VERSION: &str = "2025-11-25";
const HEADER_MCP_SESSION_ID: &str = "mcp-session-id";
const HEADER_MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";

#[derive(Debug, Clone)]
pub(super) struct StreamableHttpTarget {
    pub(super) url: Url,
}

impl StreamableHttpTarget {
    pub(super) fn parse(url: &str) -> Result<Self> {
        let mut url = Url::parse(url).context("invalid Streamable HTTP URL")?;
        super::http_utils::validate_streamable_http_url(url.as_str())?;
        url.set_fragment(None);
        Ok(Self { url })
    }
}

#[derive(Clone)]
pub(crate) struct StreamableHttpSender {
    target: StreamableHttpTarget,
    headers: Vec<(String, String)>,
    server_name: String,
    http_client: reqwest::Client,
    pending: PendingRequests,
    session_id: Arc<Mutex<Option<String>>>,
    protocol_version: Arc<Mutex<String>>,
    runtime: McpRuntimeContext,
}

impl StreamableHttpSender {
    pub(super) fn new(
        target: StreamableHttpTarget,
        headers: Vec<(String, String)>,
        server_name: String,
        http_client: reqwest::Client,
        pending: PendingRequests,
        runtime: McpRuntimeContext,
    ) -> Self {
        Self {
            target,
            headers,
            server_name,
            http_client,
            pending,
            session_id: Arc::new(Mutex::new(None)),
            protocol_version: Arc::new(Mutex::new(STREAMABLE_HTTP_PROTOCOL_VERSION.to_string())),
            runtime,
        }
    }

    pub(super) fn set_event_sink(&mut self, event_sink: Option<SharedMcpEventSink>) {
        self.runtime.set_event_sink(event_sink);
    }

    pub(super) async fn set_protocol_version(&self, protocol_version: String) {
        *self.protocol_version.lock().await = protocol_version;
    }

    pub(super) async fn post_json(&self, body: &str) -> Result<()> {
        let expected_id = json_rpc_request_id(body);
        let request = self
            .http_client
            .post(self.target.url.clone())
            .headers(reqwest_header_map(&self.headers)?)
            .body(body.to_string());
        let request = self
            .with_streamable_headers(
                request,
                Some("application/json"),
                "application/json, text/event-stream",
            )
            .await?;

        let response = request.send().await.with_context(|| {
            format!(
                "failed to POST MCP Streamable HTTP JSON-RPC for server '{}'",
                self.server_name
            )
        })?;
        self.capture_session_id(response.headers()).await?;

        // Detect session expiry (404 with an active session)
        if response.status() == StatusCode::NOT_FOUND && self.session_id.lock().await.is_some() {
            *self.session_id.lock().await = None;
            self.runtime
                .emit_event(super::super::McpSubsystemEvent::ServerStateChanged {
                    server_name: self.server_name.clone(),
                    state: "session-expired".to_string(),
                    error: Some(format!(
                        "MCP Streamable HTTP server '{}' returned 404 for an active session — session expired",
                        self.server_name
                    )),
            });
            bail!(
                "MCP Streamable HTTP session expired for server '{}' (HTTP 404); reconnecting",
                self.server_name
            );
        }

        // Extract WWW-Authenticate before handling status, so we can include
        // it in the error message
        let challenge = www_authenticate_challenge(response.headers(), response.status());
        handle_streamable_http_status(
            response.status(),
            &self.server_name,
            &self.runtime,
            challenge,
        )?;

        if response.status() == StatusCode::ACCEPTED {
            if expected_id.is_some() {
                bail!(
                    "MCP Streamable HTTP server '{}' accepted request without returning a JSON-RPC response",
                    self.server_name
                );
            }
            return Ok(());
        }

        if is_event_stream_response(response.headers()) {
            let body_stream = response.bytes_stream().map_err(reqwest_error_to_io);
            let reader = BufReader::new(StreamReader::new(body_stream));
            process_streamable_http_event_stream(
                reader,
                self.pending.clone(),
                &self.server_name,
                expected_id,
                &self.runtime,
            )
            .await
        } else {
            let response_value = response.json::<Value>().await.with_context(|| {
                format!(
                    "failed to parse MCP Streamable HTTP JSON response for server '{}'",
                    self.server_name
                )
            })?;
            dispatch_streamable_http_json_message(
                response_value,
                self.pending.clone(),
                &self.server_name,
                expected_id,
                &self.runtime,
            )
            .await
            .map(|_| ())
        }
    }

    pub(super) async fn open_get_stream(&self) -> Result<Option<tokio::task::JoinHandle<()>>> {
        let request = self
            .http_client
            .get(self.target.url.clone())
            .headers(reqwest_header_map(&self.headers)?);
        let request = self
            .with_streamable_headers(request, None, "text/event-stream")
            .await?;
        let response = request.send().await.with_context(|| {
            format!(
                "failed to open MCP Streamable HTTP GET stream for server '{}'",
                self.server_name
            )
        })?;

        match response.status() {
            StatusCode::METHOD_NOT_ALLOWED => return Ok(None),
            StatusCode::NOT_FOUND if self.session_id.lock().await.is_some() => {
                *self.session_id.lock().await = None;
                bail!(
                    "MCP Streamable HTTP session expired for server '{}' while opening GET stream (HTTP 404)",
                    self.server_name
                );
            }
            StatusCode::NOT_FOUND => return Ok(None),
            status => {
                let challenge = www_authenticate_challenge(response.headers(), status);
                handle_streamable_http_status(status, &self.server_name, &self.runtime, challenge)?;
            }
        }

        if !is_event_stream_response(response.headers()) {
            bail!(
                "MCP Streamable HTTP server '{}' returned non-SSE GET content type",
                self.server_name
            );
        }

        let body_stream = response.bytes_stream().map_err(reqwest_error_to_io);
        let reader = BufReader::new(StreamReader::new(body_stream));
        let pending = self.pending.clone();
        let server_name = self.server_name.clone();
        let runtime = self.runtime.clone();
        let handle = tokio::spawn(async move {
            streamable_http_sse_reader_loop(reader, pending, server_name, runtime).await;
        });
        Ok(Some(handle))
    }

    pub(super) async fn terminate_session(&self) -> Result<()> {
        if self.session_id.lock().await.is_none() {
            return Ok(());
        }

        let request = self
            .http_client
            .delete(self.target.url.clone())
            .headers(reqwest_header_map(&self.headers)?);
        let request = self.with_streamable_headers(request, None, "*/*").await?;
        let response = request.send().await.with_context(|| {
            format!(
                "failed to terminate MCP Streamable HTTP session for server '{}'",
                self.server_name
            )
        })?;
        match response.status() {
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND => {}
            status => {
                let challenge = www_authenticate_challenge(response.headers(), status);
                handle_streamable_http_status(status, &self.server_name, &self.runtime, challenge)?;
            }
        }
        *self.session_id.lock().await = None;
        Ok(())
    }

    async fn with_streamable_headers(
        &self,
        request: reqwest::RequestBuilder,
        content_type: Option<&'static str>,
        accept: &'static str,
    ) -> Result<reqwest::RequestBuilder> {
        let protocol_version = self.protocol_version.lock().await.clone();
        validate_header_value(HEADER_MCP_PROTOCOL_VERSION, &protocol_version)?;
        let mut request = request
            .header(ACCEPT, accept)
            .header(
                reqwest::header::HeaderName::from_static(HEADER_MCP_PROTOCOL_VERSION),
                protocol_version,
            )
            .header(USER_AGENT, allthecodes_config::user_agent::mcp_user_agent());
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        if let Some(session_id) = self.session_id.lock().await.clone() {
            validate_session_id(&session_id)?;
            request = request.header(
                reqwest::header::HeaderName::from_static(HEADER_MCP_SESSION_ID),
                session_id,
            );
        }
        Ok(request)
    }

    async fn capture_session_id(&self, headers: &HeaderMap) -> Result<()> {
        if let Some(value) = headers.get(HEADER_MCP_SESSION_ID) {
            let value = value
                .to_str()
                .context("invalid MCP-Session-Id response header")?;
            validate_session_id(value)?;
            *self.session_id.lock().await = Some(value.to_string());
        }
        Ok(())
    }
}

async fn process_streamable_http_event_stream<R>(
    mut reader: R,
    pending: PendingRequests,
    server_name: &str,
    expected_id: Option<u64>,
    runtime: &McpRuntimeContext,
) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut event_name = String::new();
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    let matched = handle_streamable_http_sse_event(
                        server_name,
                        &pending,
                        &event_name,
                        &data_lines,
                        expected_id,
                        runtime,
                    )
                    .await?;
                    event_name.clear();
                    data_lines.clear();
                    if matched {
                        return Ok(());
                    }
                    continue;
                }

                if line.starts_with(':') {
                    continue;
                }

                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => event_name = value.to_string(),
                    "data" => data_lines.push(value.to_string()),
                    _ => {}
                }
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to read MCP Streamable HTTP event stream for server '{}'",
                        server_name
                    )
                });
            }
        }
    }

    if expected_id.is_some() {
        bail!(
            "MCP Streamable HTTP server '{}' closed event stream before returning a JSON-RPC response",
            server_name
        );
    }
    Ok(())
}

async fn handle_streamable_http_sse_event(
    server_name: &str,
    pending: &PendingRequests,
    event_name: &str,
    data_lines: &[String],
    expected_id: Option<u64>,
    runtime: &McpRuntimeContext,
) -> Result<bool> {
    if data_lines.is_empty() {
        return Ok(false);
    }

    let data = data_lines.join("\n");
    match event_name {
        "" | "message" => {
            let value: Value = serde_json::from_str(&data).with_context(|| {
                format!(
                    "failed to parse MCP Streamable HTTP SSE JSON message from server '{}'",
                    server_name
                )
            })?;
            dispatch_streamable_http_json_message(
                value,
                pending.clone(),
                server_name,
                expected_id,
                runtime,
            )
            .await
        }
        other => {
            debug!(
                server = %server_name,
                event = other,
                "MCP: ignoring Streamable HTTP SSE event"
            );
            Ok(false)
        }
    }
}

async fn dispatch_streamable_http_json_message(
    value: Value,
    pending: PendingRequests,
    server_name: &str,
    expected_id: Option<u64>,
    runtime: &McpRuntimeContext,
) -> Result<bool> {
    if let Ok(response) = serde_json::from_value::<JsonRpcResponse>(value.clone()) {
        let matched = expected_id
            .map(|id| response.id.as_u64() == Some(id))
            .unwrap_or(false);
        dispatch_response(&pending, server_name, response).await;
        if expected_id.is_some() && !matched {
            bail!(
                "MCP Streamable HTTP server '{}' returned a response with an unexpected id",
                server_name
            );
        }
        return Ok(matched);
    }

    if let Some(event) = notification_event(server_name, &value) {
        debug!(
            server = %server_name,
            "MCP: routed Streamable HTTP server notification"
        );
        runtime.emit_event(event);
        return Ok(false);
    }

    if value
        .get("method")
        .and_then(|method| method.as_str())
        .is_some()
    {
        debug!(
            server = %server_name,
            "MCP: received Streamable HTTP server notification"
        );
        return Ok(false);
    }

    bail!(
        "MCP Streamable HTTP server '{}' returned malformed JSON-RPC message",
        server_name
    );
}

fn json_rpc_request_id(body: &str) -> Option<u64> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
}
