//! MCP stdio bridge mode (`--claude-in-chrome-mcp`).
//!
//! Spawned as a subprocess of allthecodes's MCP manager when the first-party
//! Chrome subsystem is on. Acts as a stdio MCP server toward allthecodes, and as
//! a socket client toward the Chrome native host. Bridges the two:
//!
//! ```text
//!   allthecodes (MCP client on stdio)
//!       │  tools/call
//!       ▼
//!   --claude-in-chrome-mcp  (this file)
//!       │  framed JSON over socket
//!       ▼
//!   --chrome-native-host    (native_host.rs)
//!       │  native messaging (4-byte framed)
//!       ▼
//!   Chrome extension
//! ```
//!
//! The MCP server implementation here is a minimal, hand-rolled JSON-RPC 2.0
//! loop — just enough for `initialize`, `tools/list`, and `tools/call`.
//! We don't pull in a full MCP SDK because allthecodes's own MCP client only
//! uses the subset we need, and a third-party dep would duplicate work
//! already done in `src/mcp/`.
//!
//! **Scope**: wiring + protocol are wired; the tool definitions below are a
//! reasonable guess at what the Anthropic Chrome extension accepts based on
//! the bun reference (`BROWSER_TOOLS` lives in the proprietary
//! `@ant/claude-for-chrome-mcp` package, so names and schemas below may need
//! to be adjusted when connected to the real extension).

use std::time::Duration;

#[cfg(windows)]
use anyhow::Context;
use anyhow::Result;
use parking_lot::Mutex as SyncMutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use super::native_host::{read_framed, write_framed};
#[cfg(unix)]
use super::transport::all_socket_paths;
use super::transport::secure_socket_path;

/// How long to wait for a tool_response from Chrome before giving up.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------
// Tool catalogue
// ---------------------------------------------------------------------------
//
// These mirror the bun reference's core browser-automation tools. The
// JSON schemas are conservative (string inputs with permissive shapes) so
// the extension can evolve without a matching allthecodes release.

fn tool_catalogue() -> Vec<Value> {
    vec![
        json!({
            "name": "navigate",
            "description": "Navigate the active tab to a URL.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "tab_id": {"type": "integer"}
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "tabs_context_mcp",
            "description": "Get information about the browser's current tabs.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "tabs_create_mcp",
            "description": "Create a new browser tab.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                }
            }
        }),
        json!({
            "name": "get_page_text",
            "description": "Read the visible text content of the active page.",
            "inputSchema": {
                "type": "object",
                "properties": { "tab_id": {"type": "integer"} }
            }
        }),
        json!({
            "name": "click",
            "description": "Click an element on the active page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "tab_id": {"type": "integer"}
                },
                "required": ["selector"]
            }
        }),
        json!({
            "name": "form_input",
            "description": "Fill a form field on the active page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "value": {"type": "string"},
                    "tab_id": {"type": "integer"}
                },
                "required": ["selector", "value"]
            }
        }),
        json!({
            "name": "javascript_tool",
            "description": "Execute JavaScript in the active page and return the result.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": {"type": "string"},
                    "tab_id": {"type": "integer"}
                },
                "required": ["script"]
            }
        }),
        json!({
            "name": "read_console_messages",
            "description": "Read console messages from the active tab.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab_id": {"type": "integer"},
                    "pattern": {"type": "string"}
                }
            }
        }),
        json!({
            "name": "read_network_requests",
            "description": "Read network requests from the active tab.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tab_id": {"type": "integer"},
                    "pattern": {"type": "string"}
                }
            }
        }),
    ]
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcRequest {
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum RpcOutgoing {
    Ok {
        jsonrpc: &'static str,
        id: Value,
        result: Value,
    },
    Err {
        jsonrpc: &'static str,
        id: Value,
        error: RpcError,
    },
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

fn rpc_ok(id: Value, result: Value) -> RpcOutgoing {
    RpcOutgoing::Ok {
        jsonrpc: "2.0",
        id,
        result,
    }
}

fn rpc_err(id: Value, code: i64, message: impl Into<String>) -> RpcOutgoing {
    RpcOutgoing::Err {
        jsonrpc: "2.0",
        id,
        error: RpcError {
            code,
            message: message.into(),
        },
    }
}

// ---------------------------------------------------------------------------
// Pending-request tracking
// ---------------------------------------------------------------------------
//
// When we forward a tool_request to the native host, we tag it with a
// request_id so the response — which comes back asynchronously — can be
// matched to the caller. Each entry in the map holds the oneshot sender the
// awaiter is blocked on.

type Pending = Arc<SyncMutex<HashMap<u64, oneshot::Sender<Value>>>>;

fn next_request_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(unix)]
type NativeStream = tokio::net::UnixStream;
#[cfg(windows)]
type NativeStream = tokio::net::windows::named_pipe::NamedPipeClient;

type NativeWriteHalf = tokio::io::WriteHalf<NativeStream>;

#[derive(Clone)]
struct NativeHostConnection {
    id: u64,
    writer: Arc<tokio::sync::Mutex<NativeWriteHalf>>,
    pending: Pending,
}

type NativeConnectionStore = Arc<tokio::sync::Mutex<Option<NativeHostConnection>>>;

fn next_connection_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

async fn clear_connection_if_current(connections: &NativeConnectionStore, connection_id: u64) {
    let mut guard = connections.lock().await;
    if guard.as_ref().map(|conn| conn.id) == Some(connection_id) {
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// Socket connection
// ---------------------------------------------------------------------------

/// Try to open a connection to any native host socket we can find. Walks
/// `all_socket_paths()` and returns the first success.
#[cfg(unix)]
async fn connect_native_host() -> Result<tokio::net::UnixStream> {
    let mut candidates = all_socket_paths();
    // Put the default path first so a fresh native host is tried before
    // stragglers.
    let preferred = secure_socket_path();
    if !candidates.contains(&preferred) {
        candidates.insert(0, preferred);
    }
    for path in &candidates {
        match tokio::net::UnixStream::connect(path).await {
            Ok(s) => {
                debug!(path = %path.display(), "mcp-bridge: connected to native host");
                return Ok(s);
            }
            Err(e) => {
                debug!(path = %path.display(), error = %e, "mcp-bridge: socket not available")
            }
        }
    }
    anyhow::bail!(
        "no Chrome native host socket accepting connections (checked {} paths); \
         is the Anthropic Chrome extension installed and Chrome running?",
        candidates.len()
    )
}

#[cfg(windows)]
async fn connect_native_host() -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let name = secure_socket_path();
    let name_str = name.to_string_lossy().to_string();
    let client = ClientOptions::new()
        .open(&name_str)
        .with_context(|| format!("open named pipe {}", name_str))?;
    debug!(path = %name_str, "mcp-bridge: connected to native host");
    Ok(client)
}

async fn ensure_native_host_connection(
    connections: &NativeConnectionStore,
) -> Result<NativeHostConnection> {
    if let Some(existing) = connections.lock().await.clone() {
        return Ok(existing);
    }

    let stream = connect_native_host().await?;
    let (read_half, write_half) = tokio::io::split(stream);
    let connection = NativeHostConnection {
        id: next_connection_id(),
        writer: Arc::new(tokio::sync::Mutex::new(write_half)),
        pending: Arc::new(SyncMutex::new(HashMap::new())),
    };

    {
        let mut guard = connections.lock().await;
        if let Some(existing) = guard.clone() {
            return Ok(existing);
        }
        *guard = Some(connection.clone());
    }

    let pending = Arc::clone(&connection.pending);
    let connection_id = connection.id;
    let connection_store = Arc::clone(connections);
    tokio::spawn(async move {
        socket_reader_task(read_half, pending, connection_store, connection_id).await;
    });

    Ok(connection)
}

// ---------------------------------------------------------------------------
// Socket reader — dispatches tool_response messages to pending awaiters.
// ---------------------------------------------------------------------------

async fn socket_reader_task<R: tokio::io::AsyncReadExt + Unpin>(
    mut reader: R,
    pending: Pending,
    connections: NativeConnectionStore,
    connection_id: u64,
) {
    loop {
        match read_framed(&mut reader).await {
            Ok(Some(bytes)) => {
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                // Native host strips `type` before forwarding, but the
                // extension includes `request_id` (per the wire protocol).
                // If this is a tool_response for a known request, deliver it.
                let request_id = value.get("request_id").and_then(|v| v.as_u64());
                if let Some(id) = request_id {
                    if let Some(sender) = pending.lock().remove(&id) {
                        let _ = sender.send(value);
                        continue;
                    }
                }
                debug!(
                    ?value,
                    "mcp-bridge: unsolicited socket message (no matching request_id)"
                );
            }
            Ok(None) => {
                warn!("mcp-bridge: native-host socket closed");
                break;
            }
            Err(e) => {
                warn!(error = %e, "mcp-bridge: socket read error");
                break;
            }
        }
    }
    // Fail any still-pending requests so callers unblock.
    let senders: Vec<_> = pending.lock().drain().map(|(_, tx)| tx).collect();
    for tx in senders {
        let _ = tx.send(json!({ "error": "native host socket closed" }));
    }
    clear_connection_if_current(&connections, connection_id).await;
}

// ---------------------------------------------------------------------------
// Tool call forwarding
// ---------------------------------------------------------------------------

// The native host protocol requires serialized writes to one async writer.
#[allow(clippy::await_holding_invalid_type)]
async fn forward_tool_call(
    connections: &NativeConnectionStore,
    method: &str,
    params: Value,
) -> Result<Value> {
    let connection = ensure_native_host_connection(connections).await?;
    let request_id = next_request_id();
    let (tx, rx) = oneshot::channel();
    connection.pending.lock().insert(request_id, tx);

    let payload = json!({
        "method": method,
        "params": params,
        "request_id": request_id,
    });
    let bytes = serde_json::to_vec(&payload)?;
    {
        let mut guard = connection.writer.lock().await;
        if let Err(e) = write_framed(&mut *guard, &bytes).await {
            connection.pending.lock().remove(&request_id);
            clear_connection_if_current(connections, connection.id).await;
            return Err(e.into());
        }
    }

    match tokio::time::timeout(TOOL_CALL_TIMEOUT, rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => {
            clear_connection_if_current(connections, connection.id).await;
            anyhow::bail!("native host dropped the response channel")
        }
        Err(_) => {
            connection.pending.lock().remove(&request_id);
            anyhow::bail!(
                "tool '{method}' timed out after {}s (is the Chrome extension connected?)",
                TOOL_CALL_TIMEOUT.as_secs()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run() -> Result<()> {
    info!("claude-in-chrome-mcp: starting stdio MCP bridge");

    // The native host may not exist yet on a fresh startup; keep the bridge
    // alive and defer the actual socket connection until the first tool call.
    let connections: NativeConnectionStore = Arc::new(tokio::sync::Mutex::new(None));
    if let Err(e) = ensure_native_host_connection(&connections).await {
        info!(
            error = %e,
            "claude-in-chrome-mcp: native host not available yet; will retry on tool call"
        );
    }

    // Main loop: read JSON-RPC requests from stdin, handle them, write
    // JSON-RPC responses to stdout.
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdin_lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = stdin_lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let request: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, line, "mcp-bridge: malformed JSON-RPC");
                continue;
            }
        };

        // Notifications have no id — dispatch silently.
        let Some(id) = request.id.clone() else {
            debug!(method = %request.method, "mcp-bridge: notification");
            continue;
        };

        let response = match request.method.as_str() {
            "initialize" => rpc_ok(
                id,
                json!({
                    "protocolVersion": allthecodes_types::mcp::PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "claude-in-chrome",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),
            "tools/list" => rpc_ok(id, json!({ "tools": tool_catalogue() })),
            "tools/call" => {
                let params = request.params.unwrap_or_else(|| json!({}));
                let tool_name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match forward_tool_call(&connections, &tool_name, args).await {
                    Ok(result) => rpc_ok(
                        id,
                        json!({
                            "content": [{"type": "text", "text": serde_json::to_string(&result).unwrap_or_default()}],
                            "isError": false,
                        }),
                    ),
                    Err(e) => rpc_ok(
                        id,
                        json!({
                            "content": [{"type": "text", "text": e.to_string()}],
                            "isError": true,
                        }),
                    ),
                }
            }
            other => rpc_err(id, -32601, format!("method not found: {other}")),
        };

        let response_bytes = serde_json::to_vec(&response)?;
        stdout.write_all(&response_bytes).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    info!("claude-in-chrome-mcp: stdin closed, exiting");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_catalogue_has_expected_tools() {
        let tools = tool_catalogue();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(names.contains(&"navigate"));
        assert!(names.contains(&"get_page_text"));
        assert!(names.contains(&"tabs_create_mcp"));
        assert!(names.contains(&"javascript_tool"));
        assert!(names.contains(&"read_console_messages"));
        assert!(names.contains(&"read_network_requests"));
    }

    #[test]
    fn tool_catalogue_has_input_schemas() {
        for tool in tool_catalogue() {
            assert!(
                tool.get("inputSchema").is_some(),
                "tool missing inputSchema: {:?}",
                tool
            );
        }
    }

    #[test]
    fn rpc_ok_serializes_with_jsonrpc_tag() {
        let v = rpc_ok(json!(1), json!({"hello": "world"}));
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["jsonrpc"], "2.0");
        assert_eq!(s["id"], 1);
        assert_eq!(s["result"]["hello"], "world");
    }

    #[test]
    fn rpc_err_has_code_and_message() {
        let v = rpc_err(json!(7), -32601, "nope");
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["error"]["code"], -32601);
        assert_eq!(s["error"]["message"], "nope");
    }
}
