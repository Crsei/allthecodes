//! `/api/tui/ws` — WebSocket-based PTY bridge for xterm.js.
//!
//! This handler upgrades an HTTP connection to a WebSocket that acts as a
//! transparent PTY bridge. The browser runs xterm.js and the server runs
//! `allthecodes` (or the specified binary) inside a `portable-pty` PTY.
//!
//! ## Wire protocol
//!
//! **Client → Server**
//! ```json
//! {"type":"input","data":"..."}
//! {"type":"resize","cols":120,"rows":34}
//! {"type":"close"}
//! ```
//!
//! **Server → Client**
//! ```json
//! {"type":"output","data":"..."}
//! {"type":"exit","code":0}
//! {"type":"error","message":"..."}
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::WebState;

/// Query parameters for the TUI WebSocket endpoint.
#[derive(Deserialize, Default)]
pub struct TuiWsParams {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub mode: Option<String>,
}

/// PTY diagnostics — shared state exposed for the debug drawer.
#[derive(Clone, Default)]
pub struct PtyDiagnostics {
    pub pid: Arc<AtomicU64>,
    pub cols: Arc<AtomicU64>,
    pub rows: Arc<AtomicU64>,
    pub bytes_in: Arc<AtomicU64>,
    pub bytes_out: Arc<AtomicU64>,
    pub start_time: Arc<Mutex<Option<Instant>>>,
    pub last_resize: Arc<Mutex<Option<(u16, u16)>>>,
    pub exit_code: Arc<Mutex<Option<i32>>>,
    pub connected: Arc<Mutex<bool>>,
    pub error: Arc<Mutex<Option<String>>>,
}

/// GET /api/tui/ws — Upgrade to WebSocket PTY bridge.
pub async fn tui_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    Query(params): Query<TuiWsParams>,
) -> impl IntoResponse {
    info!(
        cwd = ?params.cwd,
        session_id = ?params.session_id,
        mode = ?params.mode,
        "GET /api/tui/ws — WebSocket upgrade"
    );

    let diag = state.pty_diagnostics.clone();

    ws.on_upgrade(move |socket| handle_tui_socket(socket, params, diag))
}

/// Drive the PTY x WebSocket bridge for the lifetime of the connection.
async fn handle_tui_socket(mut socket: WebSocket, params: TuiWsParams, diag: PtyDiagnostics) {
    // Resolve working directory
    let cwd = params
        .cwd
        .filter(|s| !s.is_empty())
        .and_then(|s| std::path::PathBuf::from(s).canonicalize().ok())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Try to find the allthecodes binary — first check for a local binary,
    // then fall back to PATH.
    let binary = find_allthecodes_binary();
    let binary_for_display = binary.clone().unwrap_or_else(|| "allthecodes".into());

    // Mark connected
    *diag.connected.lock() = true;
    *diag.start_time.lock() = Some(Instant::now());

    // Spawn PTY via portable-pty
    let result = spawn_pty(binary, &cwd, &params).await;

    let (mut pty_writer, mut pty_reader, child_killer, mut child, master_pty) = match result {
        Ok(tuple) => tuple,
        Err(e) => {
            let err_msg = format!("PTY spawn failed: {}", e);
            warn!("{}", err_msg);
            *diag.connected.lock() = false;
            *diag.error.lock() = Some(err_msg.clone());
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"type":"error","message": err_msg}).to_string(),
                ))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    // Track PID
    if let Ok(pid) = child_kinder_pid(&*child_killer) {
        diag.pid.store(pid, Ordering::SeqCst);
    }

    info!("PTY spawned for TUI WebSocket (binary={})", binary_for_display);

    // Channels: WS reader → PTY writer, PTY reader → WS writer
    let (tx, mut rx) = mpsc::channel::<String>(256);

    // Task 1: Read from PTY stdout, send to WebSocket
    let diag_out = diag.clone();
    let pty_read_task = tokio::task::spawn_blocking(move || {
        let mut reader = pty_reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = &buf[..n];
                    diag_out.bytes_out.fetch_add(n as u64, Ordering::SeqCst);
                    // Send as JSON text frame
                    if let Ok(json) = serde_json::json!({"type":"output","data": String::from_utf8_lossy(data)}).to_string().as_str().to_string() {
                        let _ = tx.blocking_send(json);
                    }
                }
                Err(e) => {
                    warn!("PTY read error: {}", e);
                    break;
                }
            }
        }
    });

    // Task 2: Read from WebSocket, write to PTY stdin
    let diag_in = diag.clone();
    let ws_read_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            // Parse JSON message
                            match serde_json::from_str::<serde_json::Value>(&text) {
                                Ok(val) => {
                                    let msg_type = val["type"].as_str().unwrap_or("");
                                    match msg_type {
                                        "input" => {
                                            if let Some(data) = val["data"].as_str() {
                                                diag_in.bytes_in.fetch_add(data.len() as u64, Ordering::SeqCst);
                                                if let Err(e) = pty_writer.write_all(data.as_bytes()) {
                                                    warn!("PTY write error: {}", e);
                                                    break;
                                                }
                                                let _ = pty_writer.flush();
                                            }
                                        }
                                        "resize" => {
                                            let cols = val["cols"].as_u64().unwrap_or(80) as u16;
                                            let rows = val["rows"].as_u64().unwrap_or(24) as u16;
                                            *diag_in.last_resize.lock() = Some((cols, rows));
                                            diag_in.cols.store(cols as u64, Ordering::SeqCst);
                                            diag_in.rows.store(rows as u64, Ordering::SeqCst);
                                            let size = PtySize { cols, rows, pixel_width: 0, pixel_height: 0 };
                                            let _ = master_pty.resize(size);
                                        }
                                        "close" => {
                                            info!("TUI WebSocket close frame received");
                                            let _ = child_killer.kill();
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                                Err(_) => {
                                    // Raw text — treat as terminal input
                                    diag_in.bytes_in.fetch_add(text.len() as u64, Ordering::SeqCst);
                                    if let Err(e) = pty_writer.write_all(text.as_bytes()) {
                                        warn!("PTY write error: {}", e);
                                        break;
                                    }
                                    let _ = pty_writer.flush();
                                }
                            }
                        }
                        Some(Ok(Message::Binary(data))) => {
                            diag_in.bytes_in.fetch_add(data.len() as u64, Ordering::SeqCst);
                            if let Err(e) = pty_writer.write_all(&data) {
                                warn!("PTY write error: {}", e);
                                break;
                            }
                            let _ = pty_writer.flush();
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("TUI WebSocket closed by client");
                            let _ = child_killer.kill();
                            break;
                        }
                        Some(Err(e)) => {
                            warn!("TUI WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }
                // Task 2.5: Forward PTY output received via channel
                Some(data) = rx.recv() => {
                    if socket.send(Message::Text(data)).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    // Wait for either task to finish (PTY exit or WS disconnect)
    tokio::select! {
        _ = pty_read_task => {
            // PTY closed — wait for exit
            match child.wait() {
                Ok(status) => {
                    let code = status.exit_code();
                    *diag.exit_code.lock() = Some(code);
                    info!("PTY exited with code {}", code);
                    let _ = socket.send(Message::Text(
                        serde_json::json!({"type":"exit","code": code}).to_string()
                    )).await;
                }
                Err(e) => {
                    warn!("PTY wait error: {}", e);
                }
            }
        }
        _ = ws_read_task => {
            // WS disconnected — kill the child
            let _ = child_killer.kill();
        }
    }

    // Cleanup
    *diag.connected.lock() = false;
    let _ = socket.close().await;
    info!("TUI WebSocket connection closed");
}

/// Spawn a PTY running the allthecodes binary.
async fn spawn_pty(
    binary: Option<std::path::PathBuf>,
    cwd: &std::path::Path,
    params: &TuiWsParams,
) -> Result<
    (
        Box<dyn std::io::Write + Send>,
        Box<dyn std::io::Read + Send>,
        Box<dyn ChildKiller + Send>,
        Box<dyn Child + Send>,
        Box<dyn MasterPty + Send>,
    ),
    String,
> {
    let pty_system = NativePtySystem::default();

    let size = PtySize {
        cols: 120,
        rows: 34,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(size)
        .map_err(|e| format!("failed to open PTY: {}", e))?;

    // Determine the binary and arguments
    let cmd = match binary {
        Some(ref path) => {
            let mut cmd = CommandBuilder::new(path);
            if let Some(sid) = &params.session_id {
                cmd.arg("--continue");
                cmd.arg(sid);
            }
            cmd.cwd(cwd);
            cmd
        }
        None => {
            // Build command: just run `bash` so there's a working terminal
            // In production, this would be the path to the allthecodes binary.
            let mut cmd = CommandBuilder::new("bash");
            cmd.cwd(cwd);
            cmd
        }
    };

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to spawn command: {}", e))?;

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to clone PTY reader: {}", e))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("failed to take PTY writer: {}", e))?;

    Ok((writer, reader, child, Box::new(pair.master)))
}

/// Find the allthecodes binary: first check for a local debug binary, then
/// look in PATH.
fn find_allthecodes_binary() -> Option<std::path::PathBuf> {
    // Check for local debug binary
    let local_paths = [
        "target/debug/allthecodes",
        "../target/debug/allthecodes",
        "../../target/debug/allthecodes",
    ];

    for p in &local_paths {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // Fall back to PATH
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths).find_map(|dir| {
                let candidate = dir.join("allthecodes");
                if candidate.exists() { Some(candidate) } else { None }
            })
        })
}

/// Extract a numeric PID from a ChildKiller (portable-pty internal).
fn child_kinder_pid(killer: &dyn ChildKiller) -> Result<u64, ()> {
    // portable-pty doesn't expose PID directly, so we report 0.
    // The diagnostics can track the connection ID instead.
    Ok(0)
}
