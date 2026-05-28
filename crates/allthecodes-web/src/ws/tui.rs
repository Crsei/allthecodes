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
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::{SessionOwner, WebState};

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

#[derive(Debug, Clone, Serialize)]
pub struct PtyDiagnosticsSnapshot {
    pub pid: u64,
    pub cols: u64,
    pub rows: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub uptime_ms: Option<u128>,
    pub last_resize: Option<(u16, u16)>,
    pub exit_code: Option<i32>,
    pub connected: bool,
    pub error: Option<String>,
}

impl PtyDiagnostics {
    pub fn snapshot(&self) -> PtyDiagnosticsSnapshot {
        PtyDiagnosticsSnapshot {
            pid: self.pid.load(Ordering::SeqCst),
            cols: self.cols.load(Ordering::SeqCst),
            rows: self.rows.load(Ordering::SeqCst),
            bytes_in: self.bytes_in.load(Ordering::SeqCst),
            bytes_out: self.bytes_out.load(Ordering::SeqCst),
            uptime_ms: self
                .start_time
                .lock()
                .map(|start| start.elapsed().as_millis()),
            last_resize: *self.last_resize.lock(),
            exit_code: *self.exit_code.lock(),
            connected: *self.connected.lock(),
            error: self.error.lock().clone(),
        }
    }
}

/// GET /api/tui/ws — Upgrade to WebSocket PTY bridge.
pub async fn tui_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    Query(params): Query<TuiWsParams>,
) -> axum::response::Response {
    info!(
        cwd = ?params.cwd,
        session_id = ?params.session_id,
        mode = ?params.mode,
        "GET /api/tui/ws — WebSocket upgrade"
    );

    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            "A chat query is already in progress for this session",
        )
            .into_response();
    }

    let engine = state.engine();
    let active_session_id = params
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| engine.current_session_id().to_string());

    if let Err(owner) = state.try_claim_tui(active_session_id) {
        return (
            StatusCode::CONFLICT,
            format!(
                "Session is currently owned by {:?}{}",
                owner.owner,
                owner
                    .session_id
                    .as_deref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ),
        )
            .into_response();
    }

    let diag = state.pty_diagnostics.clone();
    let workspace_cwd = std::path::PathBuf::from(engine.cwd())
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(engine.cwd()));

    ws.on_upgrade(move |socket| handle_tui_socket(socket, params, diag, state, workspace_cwd))
        .into_response()
}

/// Drive the PTY x WebSocket bridge for the lifetime of the connection.
async fn handle_tui_socket(
    mut socket: WebSocket,
    params: TuiWsParams,
    diag: PtyDiagnostics,
    state: WebState,
    workspace_cwd: std::path::PathBuf,
) {
    // Resolve working directory
    let cwd = match params.cwd.clone().filter(|s| !s.is_empty()) {
        Some(raw) => match std::path::PathBuf::from(raw).canonicalize() {
            Ok(path) if path.starts_with(&workspace_cwd) => path,
            Ok(_) => {
                let err_msg = "PTY cwd must stay inside the current workspace".to_string();
                *diag.error.lock() = Some(err_msg.clone());
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({"type":"error","message": err_msg})
                            .to_string()
                            .into(),
                    ))
                    .await;
                let _ = socket.close().await;
                state.release_owner(SessionOwner::TuiPty);
                return;
            }
            Err(e) => {
                let err_msg = format!("PTY cwd is invalid: {}", e);
                *diag.error.lock() = Some(err_msg.clone());
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({"type":"error","message": err_msg})
                            .to_string()
                            .into(),
                    ))
                    .await;
                let _ = socket.close().await;
                state.release_owner(SessionOwner::TuiPty);
                return;
            }
        },
        None => workspace_cwd,
    };

    // Try to find the allthecodes binary — first check for a local binary,
    // then fall back to PATH.
    let binary = find_allthecodes_binary();
    let binary_for_display = binary.clone().unwrap_or_else(|| "allthecodes".into());

    // Mark connected
    *diag.connected.lock() = true;
    *diag.start_time.lock() = Some(Instant::now());

    // Spawn PTY via portable-pty
    let result = spawn_pty(binary, &cwd, &params).await;

    let (pty_writer, pty_reader, child, master_pty) = match result {
        Ok(tuple) => tuple,
        Err(e) => {
            let err_msg = format!("PTY spawn failed: {}", e);
            warn!("{}", err_msg);
            *diag.connected.lock() = false;
            *diag.error.lock() = Some(err_msg.clone());
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"type":"error","message": err_msg})
                        .to_string()
                        .into(),
                ))
                .await;
            let _ = socket.close().await;
            state.release_owner(SessionOwner::TuiPty);
            return;
        }
    };

    // Track PID
    if let Ok(pid) = child_kinder_pid(&*child) {
        diag.pid.store(pid, Ordering::SeqCst);
    }

    info!(
        "PTY spawned for TUI WebSocket (binary={})",
        binary_for_display.display()
    );

    // Split WebSocket into sender/receiver parts
    let (mut ws_sender, ws_receiver) = socket.split();

    // Wrap child in Arc<Mutex> for sharing between tasks
    let child = Arc::new(std::sync::Mutex::new(child));

    // Channels: PTY reader → channel → WS sender
    let (tx, rx) = mpsc::channel::<String>(256);

    // Task 1: Read from PTY stdout, send to channel
    let diag_out = diag.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader = pty_reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = &buf[..n];
                    diag_out.bytes_out.fetch_add(n as u64, Ordering::SeqCst);
                    let json =
                        serde_json::json!({"type":"output","data": String::from_utf8_lossy(data)})
                            .to_string();
                    let _ = tx.blocking_send(json);
                }
                Err(e) => {
                    warn!("PTY read error: {}", e);
                    break;
                }
            }
        }
    });

    // Task 2: Main I/O bridge — WS ↔ PTY, with PTY exit handling
    let diag_in = diag.clone();
    let child_io = child.clone();
    let ws_io_task = tokio::spawn(async move {
        let mut pty_writer = pty_writer;
        let mut rx = rx;
        let mut ws_receiver = ws_receiver;
        let master_pty = master_pty;

        loop {
            tokio::select! {
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
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
                                            let _ = child_io.lock().unwrap().kill();
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                                Err(_) => {
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
                            let _ = child_io.lock().unwrap().kill();
                            break;
                        }
                        Some(Ok(_)) => {
                            // Ping/Pong — ignore
                        }
                        Some(Err(e)) => {
                            warn!("TUI WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }
                data = rx.recv() => {
                    match data {
                        Some(json) => {
                            if ws_sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                else => break,
            }
        }

        // The MVP policy is to terminate the PTY process when the WebSocket
        // disconnects, even if the browser did not send an explicit close.
        let _ = child_io.lock().unwrap().kill();

        // Wait for child process and send exit code
        let exit_code = {
            let mut child_guard = child_io.lock().unwrap();
            match child_guard.wait() {
                Ok(status) => {
                    let code = status.exit_code() as i32;
                    *diag_in.exit_code.lock() = Some(code);
                    info!("PTY exited with code {}", code);
                    code
                }
                Err(e) => {
                    warn!("PTY wait error: {}", e);
                    -1
                }
            }
        };

        // Send exit message
        let _ = ws_sender
            .send(Message::Text(
                serde_json::json!({"type":"exit","code": exit_code})
                    .to_string()
                    .into(),
            ))
            .await;

        // Cleanup
        *diag_in.connected.lock() = false;
        let _ = ws_sender.close().await;
        info!("TUI WebSocket connection closed");
    });

    // Wait for the I/O task to complete
    let _ = ws_io_task.await;

    // Update connected state in outer diagnostics
    *diag.connected.lock() = false;
    state.release_owner(SessionOwner::TuiPty);
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
            return Err("allthecodes binary was not found in target/debug or PATH".into());
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

    Ok((writer, reader, child, pair.master))
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
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join("allthecodes");
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

/// Extract a numeric PID from a ChildKiller (portable-pty internal).
fn child_kinder_pid(_killer: &dyn ChildKiller) -> Result<u64, ()> {
    // portable-pty doesn't expose PID directly, so we report 0.
    // The diagnostics can track the connection ID instead.
    Ok(0)
}
