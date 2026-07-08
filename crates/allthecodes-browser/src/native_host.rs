//! Chrome native-messaging-host mode (`--chrome-native-host`).
//!
//! Launched by Chrome when a tab with the Anthropic extension tries to talk
//! to `com.anthropic.claude_code_browser_extension`. Chrome connects our
//! stdin/stdout to the extension via the Chrome native-messaging protocol:
//!
//! * Message framing: 4-byte little-endian length prefix, followed by UTF-8
//!   JSON of that many bytes. Max 1 MiB per message.
//! * Direction: bidirectional. Chrome writes to our stdin; we write to
//!   stdout. Chrome exits us by closing stdin.
//!
//! In addition to the Chrome side, this host runs a local socket server
//! (`src/browser/transport.rs`) so that a separate allthecodes process — the MCP
//! stdio bridge (`--claude-in-chrome-mcp`, see `mcp_bridge.rs`) — can connect
//! and forward tool calls. The native host fans tool_request messages from
//! the socket out to Chrome and fans tool_response messages back.
//!
//! This file ports `claude-code-bun/src/utils/claudeInChrome/chromeNativeHost.ts`
//! with minor adjustments (tokio I/O, Rust-style buffer handling).
//!
//! **Scope of #5**: the stdin↔stdout framing, the socket server accept loop,
//! and ping/pong + mcp_connected/disconnected signaling are fully wired.
//! The JSON-level protocol details for the tool_request / tool_response
//! messages mirror the bun reference but aren't verified against the real
//! extension in this session — see `development/reference/chrome-native-host.md`.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex as SyncMutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info, warn};

#[cfg(unix)]
use super::transport::{prepare_socket_dir, secure_socket_file};
use super::transport::{secure_socket_path, MAX_MESSAGE_SIZE};

/// Version string we advertise in `status_response`.
const NATIVE_HOST_VERSION: &str = "0.1.0";

// ---------------------------------------------------------------------------
// Wire protocol types
// ---------------------------------------------------------------------------

/// A message sent by Chrome to the native host, or by the native host back
/// to Chrome. `type` discriminates the variant; the rest of the fields vary.
///
/// We parse as untyped `Value` and dispatch on `type` so unknown fields
/// round-trip unchanged (forward compatible with extension updates).
#[derive(Debug, Clone, Deserialize)]
struct ChromeMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(flatten)]
    rest: HashMap<String, Value>,
}

/// Build an outgoing Chrome message as a JSON value we can serialize.
fn make_chrome_message(kind: &str, extra: Value) -> Value {
    // If `extra` is an object, merge fields in; otherwise put it under "data".
    if let Value::Object(mut map) = extra {
        map.insert("type".to_string(), Value::String(kind.to_string()));
        Value::Object(map)
    } else if matches!(extra, Value::Null) {
        serde_json::json!({ "type": kind })
    } else {
        serde_json::json!({ "type": kind, "data": extra })
    }
}

// ---------------------------------------------------------------------------
// Framing: 4-byte LE length prefix
// ---------------------------------------------------------------------------

/// Read one framed message from an async reader.
///
/// Returns `Ok(None)` when the stream closes cleanly at a frame boundary.
/// Returns `Err` for invalid lengths, oversize messages, or mid-frame EOF.
pub async fn read_framed<R: AsyncReadExt + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf);
    if len == 0 || len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid frame length {}", len),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

/// Write one framed message to an async writer.
pub async fn write_framed<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> io::Result<()> {
    let len = payload.len();
    if len > MAX_MESSAGE_SIZE as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload exceeds MAX_MESSAGE_SIZE",
        ));
    }
    let len_bytes = (len as u32).to_le_bytes();
    writer.write_all(&len_bytes).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Chrome-stdout writer (thread-safe)
// ---------------------------------------------------------------------------

/// Shared handle for writing frames to Chrome (stdout). Cloned into every
/// socket accept task so they can all push tool_response / notification
/// frames back to the extension.
#[derive(Clone)]
struct ChromeSink {
    // A single tokio::io::Stdout isn't Sync, so we wrap in Mutex.
    inner: Arc<Mutex<tokio::io::Stdout>>,
}

impl ChromeSink {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(tokio::io::stdout())),
        }
    }

    // Chrome stdout frames must be serialized through one async writer.
    #[allow(clippy::await_holding_invalid_type)]
    async fn send(&self, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec(value).context("serialize Chrome message")?;
        let mut guard = self.inner.lock().await;
        write_framed(&mut *guard, &bytes).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MCP client bookkeeping
// ---------------------------------------------------------------------------

/// Outgoing broadcast channel — every MCP socket task subscribes and
/// forwards tool_response / notification messages received from Chrome
/// to its connected client.
#[derive(Clone)]
struct McpBus {
    tx: broadcast::Sender<Vec<u8>>,
}

impl McpBus {
    fn new() -> Self {
        // Modest channel size — each frame fans out to a small number of
        // MCP clients (usually exactly 1).
        let (tx, _) = broadcast::channel(64);
        Self { tx }
    }

    fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.tx.subscribe()
    }

    fn publish(&self, payload: Vec<u8>) {
        // Send returns Err only when there are no receivers — benign, drop.
        let _ = self.tx.send(payload);
    }
}

// ---------------------------------------------------------------------------
// Chrome-side message handler
// ---------------------------------------------------------------------------

async fn handle_chrome_message(
    msg: ChromeMessage,
    chrome: &ChromeSink,
    mcp_bus: &McpBus,
    client_count: &SyncMutex<usize>,
) -> Result<()> {
    match msg.kind.as_str() {
        "ping" => {
            debug!("chrome-native-host: ping → pong");
            let now_ms = chrono::Utc::now().timestamp_millis();
            chrome
                .send(&make_chrome_message(
                    "pong",
                    serde_json::json!({ "timestamp": now_ms }),
                ))
                .await
        }

        "get_status" => {
            chrome
                .send(&make_chrome_message(
                    "status_response",
                    serde_json::json!({
                        "native_host_version": NATIVE_HOST_VERSION,
                        "mcp_client_count": *client_count.lock(),
                    }),
                ))
                .await
        }

        // Chrome → MCP client: forward payload (minus `type`) framed to
        // every connected bridge.
        "tool_response" | "notification" => {
            let mut payload = msg.rest;
            // Strip the `type` we discriminated on — downstream MCP clients
            // handle the stripped form.
            payload.remove("type");
            let bytes = serde_json::to_vec(&Value::Object(serde_json::Map::from_iter(
                payload.into_iter(),
            )))
            .context("serialize Chrome→MCP payload")?;
            mcp_bus.publish(bytes);
            Ok(())
        }

        other => {
            warn!(kind = other, "chrome-native-host: unknown message type");
            chrome
                .send(&make_chrome_message(
                    "error",
                    serde_json::json!({ "error": format!("Unknown message type: {}", other) }),
                ))
                .await
        }
    }
}

// ---------------------------------------------------------------------------
// Socket server
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
struct McpToolRequest {
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    request_id: Option<u64>,
}

fn chrome_tool_request_message(req: McpToolRequest) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "type".to_string(),
        Value::String("tool_request".to_string()),
    );
    payload.insert("method".to_string(), Value::String(req.method));
    payload.insert("params".to_string(), req.params.unwrap_or(Value::Null));
    if let Some(request_id) = req.request_id {
        payload.insert("request_id".to_string(), Value::from(request_id));
    }
    Value::Object(payload)
}

#[cfg(unix)]
async fn run_socket_server(
    chrome: ChromeSink,
    mcp_bus: McpBus,
    client_count: Arc<SyncMutex<usize>>,
) -> Result<()> {
    use tokio::net::UnixListener;
    prepare_socket_dir()?;
    let path = secure_socket_path();
    // Make sure stale file from a crashed predecessor doesn't block bind.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("bind unix socket {}", path.display()))?;
    let _ = secure_socket_file(&path);
    info!(path = %path.display(), "chrome-native-host: socket listening");

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "chrome-native-host: accept failed");
                continue;
            }
        };
        let chrome = chrome.clone();
        let mcp_bus = mcp_bus.clone();
        let count = Arc::clone(&client_count);
        tokio::spawn(async move {
            if let Err(e) = handle_mcp_client(stream, chrome, mcp_bus, count).await {
                debug!(error = %e, "chrome-native-host: client task ended");
            }
        });
    }
}

#[cfg(windows)]
async fn run_socket_server(
    chrome: ChromeSink,
    mcp_bus: McpBus,
    client_count: Arc<SyncMutex<usize>>,
) -> Result<()> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};
    let pipe_name = secure_socket_path();
    let pipe_name_str = pipe_name.to_string_lossy().to_string();
    info!(path = %pipe_name_str, "chrome-native-host: named pipe listening");

    loop {
        // Named pipes need a fresh server instance per connection.
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .first_pipe_instance(false)
            .create(&pipe_name_str)
            .context("create named pipe")?;
        match server.connect().await {
            Ok(()) => {
                let chrome = chrome.clone();
                let mcp_bus = mcp_bus.clone();
                let count = Arc::clone(&client_count);
                tokio::spawn(async move {
                    if let Err(e) = handle_mcp_client(server, chrome, mcp_bus, count).await {
                        debug!(error = %e, "chrome-native-host: client task ended");
                    }
                });
            }
            Err(e) => {
                warn!(error = %e, "chrome-native-host: pipe connect failed");
            }
        }
    }
}

async fn handle_mcp_client<S>(
    stream: S,
    chrome: ChromeSink,
    mcp_bus: McpBus,
    client_count: Arc<SyncMutex<usize>>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    // Notify Chrome a bridge is attached.
    {
        let mut c = client_count.lock();
        *c += 1;
    }
    chrome
        .send(&make_chrome_message("mcp_connected", Value::Null))
        .await
        .ok();

    let (mut reader, mut writer) = tokio::io::split(stream);

    // Fan Chrome-side messages back to this MCP client.
    let mut rx = mcp_bus.subscribe();
    let fan_out = tokio::spawn(async move {
        while let Ok(bytes) = rx.recv().await {
            if write_framed(&mut writer, &bytes).await.is_err() {
                break;
            }
        }
    });

    // Read tool_request frames from the MCP client and forward to Chrome.
    loop {
        let frame = match read_framed(&mut reader).await {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                debug!(error = %e, "chrome-native-host: MCP client framing error");
                break;
            }
        };
        match serde_json::from_slice::<McpToolRequest>(&frame) {
            Ok(req) => {
                let _ = chrome.send(&chrome_tool_request_message(req)).await;
            }
            Err(e) => {
                warn!(error = %e, "chrome-native-host: bad tool_request from MCP client");
            }
        }
    }

    fan_out.abort();
    {
        let mut c = client_count.lock();
        *c = c.saturating_sub(1);
    }
    chrome
        .send(&make_chrome_message("mcp_disconnected", Value::Null))
        .await
        .ok();
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the Chrome native host. Reads framed Chrome messages on stdin, writes
/// framed responses on stdout, and accepts MCP bridge connections on the
/// per-OS socket. Returns when stdin closes (Chrome disconnects us).
pub async fn run() -> Result<()> {
    info!("chrome-native-host: starting (v{})", NATIVE_HOST_VERSION);

    let chrome = ChromeSink::new();
    let mcp_bus = McpBus::new();
    let client_count = Arc::new(SyncMutex::new(0usize));

    // Spawn the socket server; it runs until process exit.
    let socket_task = {
        let chrome = chrome.clone();
        let mcp_bus = mcp_bus.clone();
        let count = Arc::clone(&client_count);
        tokio::spawn(async move {
            if let Err(e) = run_socket_server(chrome, mcp_bus, count).await {
                warn!(error = %e, "chrome-native-host: socket server exited");
            }
        })
    };

    // Main loop: read framed messages from Chrome on stdin.
    let mut stdin = tokio::io::stdin();
    loop {
        match read_framed(&mut stdin).await {
            Ok(Some(bytes)) => match serde_json::from_slice::<ChromeMessage>(&bytes) {
                Ok(msg) => {
                    if let Err(e) =
                        handle_chrome_message(msg, &chrome, &mcp_bus, &client_count).await
                    {
                        warn!(error = %e, "chrome-native-host: handler failed");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "chrome-native-host: invalid Chrome JSON");
                    let _ = chrome
                        .send(&make_chrome_message(
                            "error",
                            serde_json::json!({ "error": "Invalid message format" }),
                        ))
                        .await;
                }
            },
            Ok(None) => {
                info!("chrome-native-host: Chrome closed stdin, exiting");
                break;
            }
            Err(e) => {
                warn!(error = %e, "chrome-native-host: stdin framing error");
                break;
            }
        }
    }

    socket_task.abort();

    // Best-effort cleanup of the socket file (Unix).
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(secure_socket_path());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn framing_round_trip() {
        let (mut a, mut b) = duplex(128);
        let payload = b"hello, world".to_vec();
        write_framed(&mut a, &payload).await.unwrap();
        let got = read_framed(&mut b).await.unwrap().unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn framing_rejects_oversize_length() {
        let (mut a, mut b) = duplex(16);
        // Forge a length header larger than MAX_MESSAGE_SIZE.
        let len_bytes = (MAX_MESSAGE_SIZE + 1).to_le_bytes();
        a.write_all(&len_bytes).await.unwrap();
        drop(a);
        let err = read_framed(&mut b).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn framing_clean_eof_yields_none() {
        let (a, mut b) = duplex(16);
        drop(a);
        let got = read_framed(&mut b).await.unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn make_chrome_message_merges_object() {
        let v = make_chrome_message("pong", serde_json::json!({"timestamp": 1}));
        assert_eq!(v["type"], "pong");
        assert_eq!(v["timestamp"], 1);
    }

    #[test]
    fn make_chrome_message_handles_null() {
        let v = make_chrome_message("mcp_connected", Value::Null);
        assert_eq!(v["type"], "mcp_connected");
        assert!(v.get("data").is_none());
    }

    #[test]
    fn tool_request_message_preserves_request_id() {
        let v = chrome_tool_request_message(McpToolRequest {
            method: "navigate".to_string(),
            params: Some(serde_json::json!({ "url": "https://example.com" })),
            request_id: Some(42),
        });
        assert_eq!(v["type"], "tool_request");
        assert_eq!(v["method"], "navigate");
        assert_eq!(v["request_id"], 42);
        assert_eq!(v["params"]["url"], "https://example.com");
    }
}
