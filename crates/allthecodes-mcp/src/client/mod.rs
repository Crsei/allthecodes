//! MCP client -- communicates with MCP servers over stdio, SSE, or Streamable HTTP.
//!
//! The stdio transport spawns a subprocess and exchanges line-delimited
//! JSON-RPC 2.0 messages over stdin/stdout. A background reader task
//! dispatches incoming responses to waiting request futures.
//!
//! Lifecycle:
//!   1. `McpClient::connect()` -- spawn process or prepare HTTP transport
//!   2. `McpClient::initialize()` -- JSON-RPC `initialize` + `notifications/initialized`
//!   3. `McpClient::list_tools()` / `call_tool()` / `list_resources()` / `read_resource()`
//!   4. `McpClient::disconnect()` -- graceful shutdown

mod auth_error;
mod http_utils;
mod sse;
mod stdio;
mod streamable_http;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};

use allthecodes_types::mcp::{
    CLIENT_NAME, CLIENT_VERSION, CONNECT_TIMEOUT_SECS, PROTOCOL_VERSION, TOOL_CALL_TIMEOUT_SECS,
};

use super::{
    CallToolResult, InitializeResult, JsonRpcNotification, JsonRpcRequest, ListResourcesResult,
    ListToolsResult, McpConnectionState, McpResource, McpRuntimeContext, McpServerConfig,
    McpToolDef, ReadResourceResult, ServerCapabilities, ServerInfo, SharedMcpEventSink,
    ToolCallContent,
};

pub use auth_error::is_auth_needed_error;

use http_utils::{
    normalized_http_transport_headers_with_auth, redact_url_for_log, streamable_http_client,
    validate_sse_config, validate_streamable_http_config,
};

use sse::{
    connect_loopback_sse_stream, connect_remote_https_sse_stream, receive_sse_endpoint,
    remote_sse_http_client, spawn_sse_reader, SseConnectTarget, SseHttpSender,
};

use streamable_http::{StreamableHttpSender, StreamableHttpTarget};

type PendingRequest = oneshot::Sender<Result<Value>>;
pub(crate) type PendingRequests = Arc<Mutex<HashMap<u64, PendingRequest>>>;
const STREAMABLE_HTTP_RETRY_DELAYS_MS: [u64; 2] = [250, 1_000];

// ---------------------------------------------------------------------------
// McpClient
// ---------------------------------------------------------------------------

/// MCP client for a single server connection.
///
/// Manages the subprocess lifecycle and JSON-RPC communication.
pub struct McpClient {
    /// Server configuration.
    pub config: McpServerConfig,
    /// Current connection state.
    pub state: McpConnectionState,
    /// Tools discovered from this server.
    pub tools: Vec<McpToolDef>,
    /// Resources discovered from this server.
    pub resources: Vec<McpResource>,
    /// Server capabilities (set after initialize).
    pub server_capabilities: ServerCapabilities,
    /// Server info (set after initialize).
    pub server_info: ServerInfo,
    /// Server instructions (set after initialize).
    pub instructions: Option<String>,

    // -- Internal state (stdio transport) ------------------------------------
    /// Stdin writer for the subprocess.
    pub(super) stdin_writer: Option<Arc<Mutex<tokio::process::ChildStdin>>>,
    /// Handle to the background reader task.
    pub(super) reader_handle: Arc<StdMutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Handle to the child process.
    pub(super) child: Option<tokio::process::Child>,
    /// HTTP POST sender for SSE transport.
    pub(crate) sse_sender: Option<SseHttpSender>,
    /// HTTP sender for MCP Streamable HTTP transport.
    pub(crate) streamable_http_sender: Option<StreamableHttpSender>,
    /// Monotonically increasing request ID counter.
    pub(crate) next_id: Arc<AtomicU64>,
    /// Pending requests: id -> oneshot sender for the response.
    pub(super) pending: PendingRequests,
    pub(super) runtime: McpRuntimeContext,
    pub(super) stderr_tail: Arc<StdMutex<VecDeque<String>>>,
    pub(super) stderr_tail_dropped_line_count: Arc<AtomicU64>,
    streamable_http_recovery_lock: Mutex<()>,
}

impl McpClient {
    /// Create a new MCP client for the given server configuration.
    pub fn new(config: McpServerConfig) -> Self {
        Self::with_runtime(config, McpRuntimeContext::new())
    }

    pub fn with_runtime(config: McpServerConfig, runtime: McpRuntimeContext) -> Self {
        Self {
            config,
            state: McpConnectionState::Pending,
            tools: Vec::new(),
            resources: Vec::new(),
            server_capabilities: ServerCapabilities::default(),
            server_info: ServerInfo::default(),
            instructions: None,
            stdin_writer: None,
            reader_handle: Arc::new(StdMutex::new(None)),
            child: None,
            sse_sender: None,
            streamable_http_sender: None,
            next_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            runtime,
            stderr_tail: Arc::new(StdMutex::new(VecDeque::new())),
            stderr_tail_dropped_line_count: Arc::new(AtomicU64::new(0)),
            streamable_http_recovery_lock: Mutex::new(()),
        }
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail
            .lock()
            .map(|tail| tail.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn stderr_tail_dropped_line_count(&self) -> u64 {
        self.stderr_tail_dropped_line_count.load(Ordering::SeqCst)
    }

    pub fn set_event_sink(&mut self, event_sink: Option<SharedMcpEventSink>) {
        self.runtime.set_event_sink(event_sink.clone());
        if let Some(sender) = &mut self.sse_sender {
            sender.set_event_sink(event_sink.clone());
        }
        if let Some(sender) = &mut self.streamable_http_sender {
            sender.set_event_sink(event_sink);
        }
    }

    // -----------------------------------------------------------------------
    // Connection lifecycle
    // -----------------------------------------------------------------------

    /// Connect to the MCP server.
    pub async fn connect(&mut self) -> Result<()> {
        let result = match self.config.transport.as_str() {
            "stdio" => self.connect_stdio().await,
            "sse" => self.connect_sse().await,
            "streamable-http" => self.connect_streamable_http().await,
            other => bail!("unknown MCP transport type: '{}'", other),
        };

        if let Err(ref e) = result {
            let state = if is_auth_needed_error(e) {
                "auth-needed"
            } else {
                "error"
            };
            self.runtime
                .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                    server_name: self.config.name.clone(),
                    state: state.to_string(),
                    error: Some(e.to_string()),
                });
        }

        result
    }

    /// Connect via HTTP SSE transport.
    ///
    /// Loopback `http://` URLs keep the minimal raw TCP implementation. Remote
    /// endpoints must use `https://` and go through reqwest with redirects
    /// disabled so credentials are never forwarded to an unvalidated location.
    async fn connect_sse(&mut self) -> Result<()> {
        validate_sse_config(&self.config)?;
        let url = self
            .config
            .url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("sse transport requires 'url' field"))?;
        let base_target = SseConnectTarget::parse(url)?;
        let headers = normalized_http_transport_headers_with_auth(&self.config).await?;

        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "connecting".to_string(),
                error: None,
            });

        info!(
            server = %self.config.name,
            url = %redact_url_for_log(url),
            "MCP: connecting to SSE server"
        );

        let (reader_handle, endpoint_rx, remote_http_client) = match &base_target {
            SseConnectTarget::Loopback(target) => {
                let reader =
                    connect_loopback_sse_stream(target, &headers, &self.config.name).await?;
                let (handle, rx) = spawn_sse_reader(
                    reader,
                    self.pending.clone(),
                    self.config.name.clone(),
                    self.runtime.clone(),
                );
                (handle, rx, None)
            }
            SseConnectTarget::RemoteHttps(target) => {
                let http_client = remote_sse_http_client()?;
                let reader = connect_remote_https_sse_stream(
                    &http_client,
                    target,
                    &headers,
                    &self.config.name,
                )
                .await?;
                let (handle, rx) = spawn_sse_reader(
                    reader,
                    self.pending.clone(),
                    self.config.name.clone(),
                    self.runtime.clone(),
                );
                (handle, rx, Some(http_client))
            }
        };

        let endpoint_data =
            receive_sse_endpoint(&self.config.name, &reader_handle, endpoint_rx).await?;
        let post_target = base_target.resolve_endpoint(&endpoint_data)?;
        self.sse_sender = Some(SseHttpSender {
            target: post_target,
            headers,
            server_name: self.config.name.clone(),
            http_client: remote_http_client,
            runtime: self.runtime.clone(),
        });
        self.replace_reader_handle(Some(reader_handle));
        self.state = McpConnectionState::Connected;

        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "connected".to_string(),
                error: None,
            });

        debug!(server = %self.config.name, "MCP: SSE server connected");
        Ok(())
    }

    /// Connect via MCP Streamable HTTP transport.
    ///
    /// Streamable HTTP is request-oriented: initialization and subsequent
    /// JSON-RPC messages are POSTed to the configured endpoint. Servers may
    /// answer with a normal JSON response or an SSE response stream for that
    /// request. A long-lived GET stream is opened after initialization only
    /// when the server accepts it.
    async fn connect_streamable_http(&mut self) -> Result<()> {
        validate_streamable_http_config(&self.config)?;
        let url = self
            .config
            .url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("streamable-http transport requires 'url' field"))?;
        let target = StreamableHttpTarget::parse(url)?;
        let headers = normalized_http_transport_headers_with_auth(&self.config).await?;
        let http_client = streamable_http_client(&target.url)?;

        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "connecting".to_string(),
                error: None,
            });

        info!(
            server = %self.config.name,
            url = %redact_url_for_log(url),
            "MCP: preparing Streamable HTTP server"
        );

        self.streamable_http_sender = Some(StreamableHttpSender::new(
            target,
            headers,
            self.config.name.clone(),
            http_client,
            self.pending.clone(),
            self.runtime.clone(),
        ));
        self.state = McpConnectionState::Connected;

        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "connected".to_string(),
                error: None,
            });

        debug!(server = %self.config.name, "MCP: Streamable HTTP server ready");
        Ok(())
    }

    /// Initialize the MCP connection -- exchange capabilities with the server.
    pub async fn initialize(&mut self) -> Result<()> {
        if self.state != McpConnectionState::Connected {
            bail!("cannot initialize: not connected (state: {:?})", self.state);
        }

        let protocol_version = if self.config.transport == "streamable-http" {
            "2025-11-25"
        } else {
            PROTOCOL_VERSION
        };

        let params = initialize_params(protocol_version);

        let response = self
            .send_request("initialize", Some(params))
            .await
            .context("MCP initialize handshake failed")?;

        let init_result: InitializeResult =
            serde_json::from_value(response).context("failed to parse initialize response")?;

        self.server_capabilities = init_result.capabilities;
        self.server_info = init_result.server_info.clone();
        self.instructions = init_result.instructions;

        if let Some(sender) = &self.streamable_http_sender {
            sender
                .set_protocol_version(init_result.protocol_version.clone())
                .await;
        }

        info!(
            server = %self.config.name,
            protocol_version = %init_result.protocol_version,
            server_name = %init_result.server_info.name,
            server_version = %init_result.server_info.version,
            "MCP: initialized"
        );

        self.send_notification("notifications/initialized", None)
            .await?;

        if let Some(sender) = &self.streamable_http_sender {
            match sender.open_get_stream().await {
                Ok(Some(handle)) => {
                    self.replace_reader_handle(Some(handle));
                }
                Ok(None) => {}
                Err(error) => {
                    warn!(
                        server = %self.config.name,
                        error = %error,
                        "MCP: Streamable HTTP GET listener unavailable"
                    );
                }
            }
        }

        Ok(())
    }

    /// Disconnect from the MCP server.
    pub async fn disconnect(&mut self) {
        info!(server = %self.config.name, "MCP: disconnecting");

        self.stdin_writer.take();
        self.sse_sender.take();
        if let Some(sender) = self.streamable_http_sender.take() {
            if let Err(error) = sender.terminate_session().await {
                warn!(
                    server = %self.config.name,
                    error = %error,
                    "MCP: failed to terminate Streamable HTTP session"
                );
            }
        }

        self.abort_reader_handle();

        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }

        {
            let mut pending = self.pending.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(anyhow::anyhow!("MCP client disconnected")));
            }
        }

        self.state = McpConnectionState::Disconnected;

        self.runtime
            .emit_event(super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "disconnected".to_string(),
                error: None,
            });
    }

    // -----------------------------------------------------------------------
    // Tool operations
    // -----------------------------------------------------------------------

    /// List available tools from the MCP server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        if self.state != McpConnectionState::Connected {
            bail!("cannot list tools: not connected");
        }

        let response = self
            .send_request("tools/list", None)
            .await
            .context("tools/list request failed")?;

        let result: ListToolsResult =
            serde_json::from_value(response).context("failed to parse tools/list response")?;

        let mut tools = result.tools;
        for tool in &mut tools {
            tool.server_name = self.config.name.clone();
        }

        info!(
            server = %self.config.name,
            count = tools.len(),
            "MCP: discovered tools"
        );

        self.tools = tools.clone();

        self.runtime
            .emit_event(super::McpSubsystemEvent::ToolsDiscovered {
                server_name: self.config.name.clone(),
                tools: self
                    .tools
                    .iter()
                    .map(|t| super::McpToolInfo {
                        server_name: self.config.name.clone(),
                        tool_name: t.name.clone(),
                        description: t.description.clone(),
                    })
                    .collect(),
            });

        Ok(tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<CallToolResult> {
        if self.state != McpConnectionState::Connected {
            bail!("cannot call tool: not connected");
        }

        let params = json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let response = self
            .send_request_with_timeout("tools/call", Some(params), TOOL_CALL_TIMEOUT_SECS)
            .await
            .with_context(|| format!("tools/call '{}' failed", tool_name))?;

        let result: CallToolResult = serde_json::from_value(response)
            .with_context(|| format!("failed to parse tools/call '{}' response", tool_name))?;

        if result.is_error {
            let error_text = result
                .content
                .iter()
                .filter_map(|c| match c {
                    ToolCallContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            warn!(
                server = %self.config.name,
                tool = tool_name,
                error = %error_text,
                "MCP tool returned error"
            );
        }

        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Resource operations
    // -----------------------------------------------------------------------

    /// List resources from the MCP server.
    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>> {
        if self.state != McpConnectionState::Connected {
            bail!("cannot list resources: not connected");
        }

        let response = self
            .send_request("resources/list", None)
            .await
            .context("resources/list request failed")?;

        let result: ListResourcesResult =
            serde_json::from_value(response).context("failed to parse resources/list response")?;

        info!(
            server = %self.config.name,
            count = result.resources.len(),
            "MCP: discovered resources"
        );

        self.resources = result.resources.clone();

        self.runtime
            .emit_event(super::McpSubsystemEvent::ResourcesDiscovered {
                server_name: self.config.name.clone(),
                resources: self
                    .resources
                    .iter()
                    .map(|r| super::McpResourceInfo {
                        server_name: self.config.name.clone(),
                        uri: r.uri.clone(),
                        name: r.name.clone(),
                        description: None,
                        mime_type: r.mime_type.clone(),
                    })
                    .collect(),
            });

        Ok(result.resources)
    }

    /// Read a resource from the MCP server.
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        if self.state != McpConnectionState::Connected {
            bail!("cannot read resource: not connected");
        }

        let params = json!({ "uri": uri });

        let response = self
            .send_request("resources/read", Some(params))
            .await
            .with_context(|| format!("resources/read '{}' failed", uri))?;

        let result: ReadResourceResult = serde_json::from_value(response)
            .with_context(|| format!("failed to parse resources/read '{}' response", uri))?;

        Ok(result)
    }

    // -----------------------------------------------------------------------
    // JSON-RPC messaging
    // -----------------------------------------------------------------------

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        self.send_request_with_timeout(method, params, CONNECT_TIMEOUT_SECS)
            .await
    }

    /// Send a JSON-RPC request with a custom timeout.
    async fn send_request_with_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_secs: u64,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = JsonRpcRequest::new(id, method, params);
        let request_json =
            serde_json::to_string(&request).context("failed to serialize JSON-RPC request")?;

        debug!(
            server = %self.config.name,
            id = id,
            method = method,
            "MCP: sending request"
        );

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        if let Err(error) = self
            .write_line_with_timeout(&request_json, timeout_secs)
            .await
        {
            let mut pending = self.pending.lock().await;
            pending.remove(&id);
            return Err(error);
        }

        let timeout = std::time::Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                bail!(
                    "MCP server '{}' closed connection while waiting for response to '{}'",
                    self.config.name,
                    method
                );
            }
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                bail!(
                    "MCP request '{}' to server '{}' timed out after {}s",
                    method,
                    self.config.name,
                    timeout_secs
                );
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)
            .context("failed to serialize JSON-RPC notification")?;

        debug!(
            server = %self.config.name,
            method = method,
            "MCP: sending notification"
        );

        self.write_line(&json).await
    }

    /// Write a JSON-RPC line to the active transport.
    async fn write_line(&self, line: &str) -> Result<()> {
        self.write_line_with_timeout(line, CONNECT_TIMEOUT_SECS)
            .await
    }

    /// Write a JSON-RPC line to the active transport with an HTTP read bound.
    /// On session expiry for streamable-http, automatically re-initializes
    /// and reruns the current JSON-RPC operation.
    async fn write_line_with_timeout(&self, line: &str, timeout_secs: u64) -> Result<()> {
        if self.streamable_http_sender.is_some() {
            let method = extract_method_name(line);
            for (attempt, retry_delay_ms) in STREAMABLE_HTTP_RETRY_DELAYS_MS
                .iter()
                .copied()
                .map(Some)
                .chain(std::iter::once(None))
                .enumerate()
            {
                let result = self
                    .post_streamable_line_without_recovery(line, timeout_secs)
                    .await;
                match result {
                    Err(ref error) if is_session_expired_error(error) => {
                        return self.recover_session(line, timeout_secs).await;
                    }
                    Err(ref error)
                        if retry_delay_ms.is_some()
                            && is_retryable_streamable_http_error(method.as_deref(), error) =>
                    {
                        let delay_ms = retry_delay_ms.expect("checked is_some");
                        warn!(
                            server = %self.config.name,
                            method = ?method,
                            attempt = attempt + 1,
                            delay_ms = delay_ms,
                            error = %error,
                            "MCP: Streamable HTTP request failed with retryable status; retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    other => return other,
                }
            }
            unreachable!("streamable HTTP retry loop always returns on final attempt");
        }

        if let Some(sender) = &self.sse_sender {
            return sender.post_json(line).await;
        }

        let writer = self.stdin_writer.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "MCP transport writer not available for server '{}'",
                self.config.name
            )
        })?;

        let mut writer = writer.lock().await;
        writer
            .write_all(line.as_bytes())
            .await
            .context("failed to write to MCP server stdin")?;
        writer
            .write_all(b"\n")
            .await
            .context("failed to write newline to MCP server stdin")?;
        writer
            .flush()
            .await
            .context("failed to flush MCP server stdin")?;

        Ok(())
    }

    /// Attempt recovery from a Streamable HTTP session expiry.
    ///
    /// Reruns the current operation after recovery. This intentionally matches
    /// the upstream Streamable HTTP behavior, including tool calls.
    async fn recover_session(&self, failed_line: &str, timeout_secs: u64) -> Result<()> {
        let method = extract_method_name(failed_line);
        let _guard = self.streamable_http_recovery_lock.lock().await;

        if matches!(method.as_deref(), Some("tools/call")) {
            warn!(
                server = %self.config.name,
                method = ?method,
                "MCP: session expired; rerunning tool call after recovery"
            );
        }

        info!(
            server = %self.config.name,
            method = ?method,
            "MCP: session expired, reinitializing before retry"
        );

        let init_result = self
            .send_streamable_request_without_recovery(
                "initialize",
                Some(initialize_params("2025-11-25")),
                timeout_secs,
            )
            .await
            .context("MCP Streamable HTTP recovery initialize failed")?;
        let init_result: InitializeResult = serde_json::from_value(init_result)
            .context("failed to parse recovery initialize response")?;

        let sender = self.streamable_http_sender.as_ref().ok_or_else(|| {
            anyhow::anyhow!("MCP Streamable HTTP sender not available for recovery")
        })?;
        sender
            .set_protocol_version(init_result.protocol_version.clone())
            .await;

        self.send_streamable_notification_without_recovery(
            "notifications/initialized",
            None,
            timeout_secs,
        )
        .await
        .context("MCP Streamable HTTP recovery initialized notification failed")?;

        match sender.open_get_stream().await {
            Ok(Some(handle)) => self.replace_reader_handle(Some(handle)),
            Ok(None) => {}
            Err(error) => {
                warn!(
                    server = %self.config.name,
                    error = %error,
                    "MCP: Streamable HTTP GET listener unavailable after recovery"
                );
            }
        }

        match self
            .post_streamable_line_without_recovery(failed_line, timeout_secs)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if is_session_expired_error(&e) => {
                bail!(
                    "MCP Streamable HTTP session for server '{}' expired and recovery failed: {}",
                    self.config.name,
                    e
                );
            }
            Err(e) => Err(e),
        }
    }

    async fn send_streamable_request_without_recovery(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_secs: u64,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(id, method, params);
        let request_json =
            serde_json::to_string(&request).context("failed to serialize JSON-RPC request")?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        if let Err(error) = self
            .post_streamable_line_without_recovery(&request_json, timeout_secs)
            .await
        {
            let mut pending = self.pending.lock().await;
            pending.remove(&id);
            return Err(error);
        }

        let timeout = std::time::Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                bail!(
                    "MCP server '{}' closed connection while waiting for response to '{}'",
                    self.config.name,
                    method
                );
            }
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                bail!(
                    "MCP request '{}' to server '{}' timed out after {}s",
                    method,
                    self.config.name,
                    timeout_secs
                );
            }
        }
    }

    async fn send_streamable_notification_without_recovery(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_secs: u64,
    ) -> Result<()> {
        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)
            .context("failed to serialize JSON-RPC notification")?;
        self.post_streamable_line_without_recovery(&json, timeout_secs)
            .await
    }

    async fn post_streamable_line_without_recovery(
        &self,
        line: &str,
        timeout_secs: u64,
    ) -> Result<()> {
        let sender = self.streamable_http_sender.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "MCP Streamable HTTP sender not available for server '{}'",
                self.config.name
            )
        })?;

        match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            sender.post_json(line),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                bail!(
                    "MCP Streamable HTTP request to server '{}' timed out after {}s",
                    self.config.name,
                    timeout_secs
                );
            }
        }
    }

    fn replace_reader_handle(&self, handle: Option<tokio::task::JoinHandle<()>>) {
        let mut reader_handle = self
            .reader_handle
            .lock()
            .expect("MCP reader handle mutex poisoned");
        if let Some(existing) = reader_handle.take() {
            existing.abort();
        }
        *reader_handle = handle;
    }

    fn abort_reader_handle(&self) {
        self.replace_reader_handle(None);
    }

    // -----------------------------------------------------------------------
    // Capability checks
    // -----------------------------------------------------------------------

    /// Check whether the server advertises tool support.
    pub fn supports_tools(&self) -> bool {
        self.server_capabilities.tools.is_some()
    }

    /// Check whether the server advertises resource support.
    pub fn supports_resources(&self) -> bool {
        self.server_capabilities.resources.is_some()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.abort_reader_handle();
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

/// Check if an error indicates a Streamable HTTP session expiry (HTTP 404).
fn is_session_expired_error(error: &anyhow::Error) -> bool {
    let msg = format!("{:#}", error);
    msg.contains("session expired")
}

fn is_retryable_streamable_http_error(method: Option<&str>, error: &anyhow::Error) -> bool {
    matches!(
        method,
        Some("initialize" | "notifications/initialized" | "tools/list")
    ) && retryable_http_status_from_error(error).is_some()
}

fn retryable_http_status_from_error(error: &anyhow::Error) -> Option<u16> {
    let msg = format!("{:#}", error);
    [408_u16, 429, 500, 502, 503, 504]
        .into_iter()
        .find(|status| msg.contains(&format!("HTTP {}", status)))
}

/// Extract the JSON-RPC method name from a request body JSON string.
fn extract_method_name(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("method")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn initialize_params(protocol_version: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "roots": {}
        },
        "clientInfo": {
            "name": CLIENT_NAME,
            "version": CLIENT_VERSION
        }
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
