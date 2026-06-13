//! Multi-session PTY terminal bridge for xterm.js.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

use crate::state::WebState;

const OUTPUT_BUFFER_LIMIT: usize = 256 * 1024;
const DETACHED_IDLE_TIMEOUT_MS: u64 = 30 * 60 * 1000;
const BRIDGE_CLI_PLUGIN_NAME: &str = "allthecodes-bridge-cli";
const BRIDGE_CLI_MCP_SERVER: &str = "allthecodes-bridge";
static NEXT_TERMINAL_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct TerminalManager {
    sessions: Arc<RwLock<HashMap<String, Arc<TerminalSession>>>>,
}

impl TerminalManager {
    pub fn create_session(
        &self,
        workspace_cwd: &Path,
        request: TerminalCreateRequest,
    ) -> Result<TerminalSessionSnapshot, String> {
        self.prune_idle_sessions();
        let profile = TerminalProfile::from_id(&request.profile)?;
        let cwd = resolve_cwd(workspace_cwd, request.cwd.as_deref())?;
        let resolved = match request.command {
            Some(command) => resolve_custom_command(command)?,
            None => resolve_profile_command(profile, request.session_id.as_deref())?,
        };
        let size = request.initial_size.unwrap_or_default().to_pty_size();
        let persist = request.persist.unwrap_or(true);
        let id = format!(
            "terminal-{}-{}",
            now_millis(),
            NEXT_TERMINAL_ID.fetch_add(1, Ordering::SeqCst)
        );
        let label = request
            .label
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| profile.default_label().to_string());
        let session =
            TerminalSession::spawn(id.clone(), label, profile, cwd, resolved, size, persist)?;
        let snapshot = session.snapshot();
        self.sessions.write().insert(id, Arc::new(session));
        Ok(snapshot)
    }

    pub fn list_sessions(&self) -> Vec<TerminalSessionSnapshot> {
        self.prune_idle_sessions();
        self.sessions
            .read()
            .values()
            .map(|session| session.snapshot())
            .collect()
    }

    pub fn get_session(&self, id: &str) -> Option<Arc<TerminalSession>> {
        self.prune_idle_sessions();
        self.sessions.read().get(id).cloned()
    }

    pub fn remove_session(&self, id: &str) -> Option<TerminalSessionSnapshot> {
        let session = self.sessions.write().remove(id)?;
        session.terminate();
        Some(session.snapshot())
    }

    pub fn latest_diagnostics(&self) -> PtyDiagnosticsSnapshot {
        self.list_sessions()
            .into_iter()
            .max_by_key(|session| session.updated_at)
            .map(PtyDiagnosticsSnapshot::from)
            .unwrap_or_default()
    }

    fn prune_idle_sessions(&self) {
        let now = now_millis() as u64;
        let stale_ids = self
            .sessions
            .read()
            .iter()
            .filter_map(|(id, session)| session.should_prune(now).then(|| id.clone()))
            .collect::<Vec<_>>();
        if stale_ids.is_empty() {
            return;
        }

        let mut sessions = self.sessions.write();
        for id in stale_ids {
            if let Some(session) = sessions.remove(&id) {
                session.terminate();
            }
        }
    }
}

pub struct TerminalSession {
    id: String,
    label: String,
    profile: TerminalProfile,
    cwd: PathBuf,
    command: String,
    persist: bool,
    pid: u64,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    child: Arc<Mutex<Option<Box<dyn Child + Send>>>>,
    status: Arc<RwLock<TerminalStatus>>,
    cols: Arc<AtomicU64>,
    rows: Arc<AtomicU64>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
    created_at: i64,
    updated_at: Arc<AtomicU64>,
    exit_code: Arc<Mutex<Option<i32>>>,
    error: Arc<Mutex<Option<String>>>,
    output_buffer: Arc<Mutex<String>>,
    attached_count: Arc<AtomicU64>,
    last_detached_at: Arc<AtomicU64>,
    output_tx: broadcast::Sender<TerminalOutputEvent>,
}

impl TerminalSession {
    fn spawn(
        id: String,
        label: String,
        profile: TerminalProfile,
        cwd: PathBuf,
        resolved: ResolvedCommand,
        size: PtySize,
        persist: bool,
    ) -> Result<Self, String> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(size)
            .map_err(|error| format!("failed to open PTY: {error}"))?;

        let mut cmd = CommandBuilder::new(&resolved.executable);
        for arg in &resolved.args {
            cmd.arg(arg);
        }
        cmd.cwd(&cwd);
        configure_terminal_environment(&mut cmd);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| format!("failed to spawn {}: {error}", resolved.display))?;
        let pid = child.process_id().unwrap_or_default() as u64;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("failed to clone PTY reader: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("failed to take PTY writer: {error}"))?;
        let (output_tx, _) = broadcast::channel(512);
        let created_at = now_millis();

        let session = Self {
            id: id.clone(),
            label,
            profile,
            cwd,
            command: resolved.display,
            persist,
            pid,
            writer: Arc::new(Mutex::new(Some(writer))),
            master: Arc::new(Mutex::new(Some(pair.master))),
            child: Arc::new(Mutex::new(Some(child))),
            status: Arc::new(RwLock::new(TerminalStatus::Running)),
            cols: Arc::new(AtomicU64::new(size.cols as u64)),
            rows: Arc::new(AtomicU64::new(size.rows as u64)),
            bytes_in: Arc::new(AtomicU64::new(0)),
            bytes_out: Arc::new(AtomicU64::new(0)),
            created_at,
            updated_at: Arc::new(AtomicU64::new(created_at as u64)),
            exit_code: Arc::new(Mutex::new(None)),
            error: Arc::new(Mutex::new(None)),
            output_buffer: Arc::new(Mutex::new(String::new())),
            attached_count: Arc::new(AtomicU64::new(0)),
            last_detached_at: Arc::new(AtomicU64::new(0)),
            output_tx,
        };

        session.spawn_reader_task(&id, reader);
        Ok(session)
    }

    fn spawn_reader_task(&self, id: &str, mut reader: Box<dyn Read + Send>) {
        let session_id = id.to_string();
        let tx = self.output_tx.clone();
        let bytes_out = self.bytes_out.clone();
        let updated_at = self.updated_at.clone();
        let buffer = self.output_buffer.clone();
        let status = self.status.clone();
        let exit_code = self.exit_code.clone();
        let error_slot = self.error.clone();
        let child_slot = self.child.clone();

        tokio::task::spawn_blocking(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&chunk[..n]).to_string();
                        bytes_out.fetch_add(n as u64, Ordering::SeqCst);
                        updated_at.store(now_millis() as u64, Ordering::SeqCst);
                        {
                            let mut output = buffer.lock();
                            output.push_str(&data);
                            if output.len() > OUTPUT_BUFFER_LIMIT {
                                let split = output.len() - OUTPUT_BUFFER_LIMIT;
                                output.drain(..split);
                            }
                        }
                        let _ = tx.send(TerminalOutputEvent {
                            session_id: session_id.clone(),
                            data,
                        });
                    }
                    Err(error) => {
                        let message = format!("PTY read error: {error}");
                        warn!("{}", message);
                        *error_slot.lock() = Some(message);
                        break;
                    }
                }
            }

            *status.write() = TerminalStatus::Exited;
            updated_at.store(now_millis() as u64, Ordering::SeqCst);
            let code = child_slot
                .lock()
                .take()
                .and_then(|mut child| child.wait().ok())
                .map(|status| status.exit_code() as i32);
            if code.is_some() {
                *exit_code.lock() = code;
            }
        });
    }

    pub fn snapshot(&self) -> TerminalSessionSnapshot {
        TerminalSessionSnapshot {
            id: self.id.clone(),
            label: self.label.clone(),
            profile: self.profile.id().to_string(),
            cwd: self.cwd.display().to_string(),
            command: self.command.clone(),
            status: self.status.read().clone(),
            pid: self.pid,
            cols: self.cols.load(Ordering::SeqCst) as u16,
            rows: self.rows.load(Ordering::SeqCst) as u16,
            bytes_in: self.bytes_in.load(Ordering::SeqCst),
            bytes_out: self.bytes_out.load(Ordering::SeqCst),
            created_at: self.created_at,
            updated_at: self.updated_at.load(Ordering::SeqCst) as i64,
            exit_code: *self.exit_code.lock(),
            error: self.error.lock().clone(),
        }
    }

    pub fn output_buffer(&self) -> String {
        self.output_buffer.lock().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TerminalOutputEvent> {
        self.output_tx.subscribe()
    }

    pub fn mark_attached(&self) {
        self.attached_count.fetch_add(1, Ordering::SeqCst);
        self.last_detached_at.store(0, Ordering::SeqCst);
        self.updated_at.store(now_millis() as u64, Ordering::SeqCst);
    }

    pub fn mark_detached(&self) {
        let remaining = decrement_atomic_counter(&self.attached_count);
        if remaining == 0 {
            self.last_detached_at
                .store(now_millis() as u64, Ordering::SeqCst);
        }
        self.updated_at.store(now_millis() as u64, Ordering::SeqCst);
    }

    pub fn should_prune(&self, now: u64) -> bool {
        if *self.status.read() == TerminalStatus::Exited {
            return !self.persist;
        }
        if self.attached_count.load(Ordering::SeqCst) > 0 {
            return false;
        }
        let detached_at = self.last_detached_at.load(Ordering::SeqCst);
        if detached_at == 0 {
            return false;
        }
        if !self.persist {
            return true;
        }
        now.saturating_sub(detached_at) >= DETACHED_IDLE_TIMEOUT_MS
    }

    pub fn write_input(&self, data: &str) -> Result<(), String> {
        if *self.status.read() != TerminalStatus::Running {
            return Err("terminal session is not running".to_string());
        }
        let mut writer_guard = self.writer.lock();
        let writer = writer_guard
            .as_mut()
            .ok_or_else(|| "terminal writer is closed".to_string())?;
        writer
            .write_all(data.as_bytes())
            .map_err(|error| format!("PTY write error: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("PTY flush error: {error}"))?;
        self.bytes_in.fetch_add(data.len() as u64, Ordering::SeqCst);
        self.updated_at.store(now_millis() as u64, Ordering::SeqCst);
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.cols.store(cols as u64, Ordering::SeqCst);
        self.rows.store(rows as u64, Ordering::SeqCst);
        self.updated_at.store(now_millis() as u64, Ordering::SeqCst);
        if let Some(master) = self.master.lock().as_mut() {
            let _ = master.resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    pub fn terminate(&self) {
        *self.status.write() = TerminalStatus::Terminating;
        self.updated_at.store(now_millis() as u64, Ordering::SeqCst);
        *self.writer.lock() = None;
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.kill();
            if let Ok(status) = child.wait() {
                *self.exit_code.lock() = Some(status.exit_code() as i32);
            }
        }
        *self.status.write() = TerminalStatus::Exited;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalOutputEvent {
    pub session_id: String,
    pub data: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TerminalCreateRequest {
    pub profile: String,
    pub cwd: Option<String>,
    pub label: Option<String>,
    pub command: Option<TerminalCommandRequest>,
    pub session_id: Option<String>,
    pub persist: Option<bool>,
    pub initial_size: Option<TerminalSize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TerminalCommandRequest {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub display: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 34,
        }
    }
}

impl TerminalSize {
    fn to_pty_size(&self) -> PtySize {
        PtySize {
            cols: self.cols.max(20),
            rows: self.rows.max(5),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Running,
    Terminating,
    Exited,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalProfileSummary {
    pub id: String,
    pub label: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalProfilesResponse {
    pub profiles: Vec<TerminalProfileSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalSessionsResponse {
    pub sessions: Vec<TerminalSessionSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerminalSessionSnapshot {
    pub id: String,
    pub label: String,
    pub profile: String,
    pub cwd: String,
    pub command: String,
    pub status: TerminalStatus,
    pub pid: u64,
    pub cols: u16,
    pub rows: u16,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TuiWsParams {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub mode: Option<String>,
}

#[derive(Clone, Default)]
pub struct PtyDiagnostics {
    manager: TerminalManager,
}

impl PtyDiagnostics {
    pub fn new(manager: TerminalManager) -> Self {
        Self { manager }
    }

    pub fn snapshot(&self) -> PtyDiagnosticsSnapshot {
        self.manager.latest_diagnostics()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
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

impl From<TerminalSessionSnapshot> for PtyDiagnosticsSnapshot {
    fn from(session: TerminalSessionSnapshot) -> Self {
        Self {
            pid: session.pid,
            cols: session.cols as u64,
            rows: session.rows as u64,
            bytes_in: session.bytes_in,
            bytes_out: session.bytes_out,
            uptime_ms: None,
            last_resize: Some((session.cols, session.rows)),
            exit_code: session.exit_code,
            connected: session.status == TerminalStatus::Running,
            error: session.error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalProfile {
    Allthecodes,
    Codex,
    Claude,
    BridgeCli,
    Custom,
    Shell,
}

impl TerminalProfile {
    fn all() -> [Self; 5] {
        [
            Self::Allthecodes,
            Self::Codex,
            Self::Claude,
            Self::BridgeCli,
            Self::Shell,
        ]
    }

    fn from_id(id: &str) -> Result<Self, String> {
        match id {
            "allthecodes" | "tui" => Ok(Self::Allthecodes),
            "codex" => Ok(Self::Codex),
            "claude" | "claude-code" => Ok(Self::Claude),
            "allthecodes-bridge-cli" | "bridge-cli" => Ok(Self::BridgeCli),
            "custom" => Ok(Self::Custom),
            "shell" | "bash" => Ok(Self::Shell),
            _ => Err(format!("unknown terminal profile: {id}")),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Allthecodes => "allthecodes",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::BridgeCli => "allthecodes-bridge-cli",
            Self::Custom => "custom",
            Self::Shell => "shell",
        }
    }

    fn default_label(self) -> &'static str {
        match self {
            Self::Allthecodes => "Allthecodes",
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::BridgeCli => "Bridge CLI",
            Self::Custom => "Custom",
            Self::Shell => "Shell",
        }
    }
}

struct ResolvedCommand {
    executable: PathBuf,
    args: Vec<String>,
    display: String,
}

pub async fn profiles_handler() -> Response {
    Json(TerminalProfilesResponse {
        profiles: TerminalProfile::all()
            .into_iter()
            .map(|profile| match resolve_profile_command(profile, None) {
                Ok(command) => TerminalProfileSummary {
                    id: profile.id().to_string(),
                    label: profile.default_label().to_string(),
                    available: true,
                    command: Some(command.display),
                    error: None,
                },
                Err(error) => TerminalProfileSummary {
                    id: profile.id().to_string(),
                    label: profile.default_label().to_string(),
                    available: false,
                    command: None,
                    error: Some(error),
                },
            })
            .collect(),
    })
    .into_response()
}

pub async fn create_session_handler(
    State(state): State<WebState>,
    Json(request): Json<TerminalCreateRequest>,
) -> Response {
    let workspace_cwd = workspace_cwd(&state);
    match state
        .terminal_manager
        .create_session(&workspace_cwd, request)
    {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(error) => terminal_error(StatusCode::BAD_REQUEST, "terminal_create_failed", error),
    }
}

pub async fn list_sessions_handler(State(state): State<WebState>) -> Response {
    Json(TerminalSessionsResponse {
        sessions: state.terminal_manager.list_sessions(),
    })
    .into_response()
}

pub async fn session_detail_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.terminal_manager.get_session(&id) {
        Some(session) => Json(session.snapshot()).into_response(),
        None => terminal_error(
            StatusCode::NOT_FOUND,
            "terminal_not_found",
            "terminal session not found",
        ),
    }
}

pub async fn delete_session_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.terminal_manager.remove_session(&id) {
        Some(session) => Json(session).into_response(),
        None => terminal_error(
            StatusCode::NOT_FOUND,
            "terminal_not_found",
            "terminal session not found",
        ),
    }
}

pub async fn session_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(session) = state.terminal_manager.get_session(&id) else {
        return terminal_error(
            StatusCode::NOT_FOUND,
            "terminal_not_found",
            "terminal session not found",
        );
    };
    let manager = state.terminal_manager.clone();
    ws.on_upgrade(move |socket| attach_socket(socket, manager, session))
        .into_response()
}

pub async fn legacy_tui_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    Query(params): Query<TuiWsParams>,
) -> Response {
    if state.is_streaming.load(Ordering::SeqCst) {
        return (
            StatusCode::CONFLICT,
            "A chat query is already in progress for this session",
        )
            .into_response();
    }

    let workspace_cwd = workspace_cwd(&state);
    let request = TerminalCreateRequest {
        profile: "allthecodes".to_string(),
        cwd: params.cwd,
        label: Some("Allthecodes".to_string()),
        command: None,
        session_id: params.session_id,
        persist: Some(false),
        initial_size: Some(TerminalSize::default()),
    };
    let session = match state
        .terminal_manager
        .create_session(&workspace_cwd, request)
    {
        Ok(snapshot) => state.terminal_manager.get_session(&snapshot.id),
        Err(error) => {
            return terminal_error(StatusCode::BAD_REQUEST, "terminal_create_failed", error);
        }
    };
    let Some(session) = session else {
        return terminal_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "terminal_missing",
            "terminal session was not stored",
        );
    };
    let manager = state.terminal_manager.clone();
    ws.on_upgrade(move |socket| attach_socket(socket, manager, session))
        .into_response()
}

async fn attach_socket(socket: WebSocket, manager: TerminalManager, session: Arc<TerminalSession>) {
    session.mark_attached();
    let mut output_rx = session.subscribe();
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let snapshot = session.snapshot();
    let ready = serde_json::json!({
        "type": "ready",
        "session_id": snapshot.id,
        "profile": snapshot.profile,
        "pid": snapshot.pid,
        "status": snapshot.status,
    })
    .to_string();
    if ws_sender.send(Message::Text(ready.into())).await.is_err() {
        return;
    }

    let buffered = session.output_buffer();
    if !buffered.is_empty() {
        let output = serde_json::json!({"type":"output","data": buffered}).to_string();
        if ws_sender.send(Message::Text(output.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if handle_client_text(&session, &text, &mut ws_sender).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if let Err(error) = session.write_input(&String::from_utf8_lossy(&data)) {
                            send_error(&mut ws_sender, &error).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        warn!("terminal websocket error: {}", error);
                        break;
                    }
                }
            }
            output = output_rx.recv() => {
                match output {
                    Ok(event) => {
                        let msg = serde_json::json!({"type":"output","data": event.data}).to_string();
                        if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            else => break,
        }
    }

    session.mark_detached();
    if session.should_prune(now_millis() as u64) {
        let _ = manager.remove_session(&session.id);
    }

    let snapshot = session.snapshot();
    if snapshot.status == TerminalStatus::Exited {
        let msg =
            serde_json::json!({"type":"exit","code": snapshot.exit_code.unwrap_or(-1)}).to_string();
        let _ = ws_sender.send(Message::Text(msg.into())).await;
    }
    let _ = ws_sender.close().await;
}

async fn handle_client_text(
    session: &TerminalSession,
    text: &str,
    ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>,
) -> bool {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => match value["type"].as_str().unwrap_or_default() {
            "input" => {
                if let Some(data) = value["data"].as_str() {
                    if let Err(error) = session.write_input(data) {
                        send_error(ws_sender, &error).await;
                    }
                }
                false
            }
            "resize" => {
                let cols = value["cols"].as_u64().unwrap_or(80).clamp(20, 400) as u16;
                let rows = value["rows"].as_u64().unwrap_or(24).clamp(5, 200) as u16;
                session.resize(cols, rows);
                false
            }
            "terminate" | "close" => {
                session.terminate();
                let snapshot = session.snapshot();
                let msg =
                    serde_json::json!({"type":"exit","code": snapshot.exit_code.unwrap_or(-1)})
                        .to_string();
                let _ = ws_sender.send(Message::Text(msg.into())).await;
                true
            }
            "detach" => true,
            _ => false,
        },
        Err(_) => {
            if let Err(error) = session.write_input(text) {
                send_error(ws_sender, &error).await;
            }
            false
        }
    }
}

async fn send_error(ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>, message: &str) {
    let msg = serde_json::json!({"type":"error","message": message}).to_string();
    let _ = ws_sender.send(Message::Text(msg.into())).await;
}

fn terminal_error(status: StatusCode, code: &'static str, error: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": error.into(),
            "code": code,
        })),
    )
        .into_response()
}

fn workspace_cwd(state: &WebState) -> PathBuf {
    let engine = state.engine();
    PathBuf::from(engine.cwd())
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(engine.cwd()))
}

fn resolve_cwd(workspace_cwd: &Path, raw: Option<&str>) -> Result<PathBuf, String> {
    match raw.filter(|value| !value.trim().is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value)
                .canonicalize()
                .map_err(|error| format!("terminal cwd is invalid: {error}"))?;
            if path.starts_with(workspace_cwd) {
                Ok(path)
            } else {
                Err("terminal cwd must stay inside the current workspace".to_string())
            }
        }
        None => Ok(workspace_cwd.to_path_buf()),
    }
}

fn resolve_profile_command(
    profile: TerminalProfile,
    allthecodes_session_id: Option<&str>,
) -> Result<ResolvedCommand, String> {
    match profile {
        TerminalProfile::Allthecodes => {
            let executable = find_allthecodes_binary().ok_or_else(|| {
                "allthecodes binary was not found in target/debug or PATH".to_string()
            })?;
            let mut args = Vec::new();
            if let Some(session_id) = allthecodes_session_id.filter(|value| !value.is_empty()) {
                args.push("--continue".to_string());
                args.push(session_id.to_string());
            }
            let display = display_command(&executable, &args);
            Ok(ResolvedCommand {
                executable,
                args,
                display,
            })
        }
        TerminalProfile::Codex => resolve_path_command("codex", &[]),
        TerminalProfile::Claude => resolve_path_command("claude", &[]),
        TerminalProfile::BridgeCli => resolve_bridge_cli_command(),
        TerminalProfile::Custom => Err("custom terminal profile requires a command".to_string()),
        TerminalProfile::Shell => {
            if let Some(shell) = std::env::var_os("SHELL")
                .map(PathBuf::from)
                .filter(|path| path.exists())
            {
                let display = shell.display().to_string();
                return Ok(ResolvedCommand {
                    executable: shell,
                    args: Vec::new(),
                    display,
                });
            }
            resolve_path_command("bash", &[])
        }
    }
}

fn resolve_custom_command(command: TerminalCommandRequest) -> Result<ResolvedCommand, String> {
    let raw_executable = command.executable.trim();
    if raw_executable.is_empty() {
        return Err("custom terminal command executable is required".to_string());
    }

    let requested = PathBuf::from(raw_executable);
    let executable = if requested.is_absolute() {
        if !requested.exists() {
            return Err(format!(
                "custom terminal command was not found at {}",
                requested.display()
            ));
        }
        requested
    } else if raw_executable.contains('/') || raw_executable.contains('\\') {
        return Err(
            "custom terminal command executable must be absolute or available on PATH".to_string(),
        );
    } else {
        find_on_path(raw_executable)
            .ok_or_else(|| format!("{} command was not found in PATH", raw_executable))?
    };
    let args = command.args;
    let display = command
        .display
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| display_command(&executable, &args));
    Ok(ResolvedCommand {
        executable,
        args,
        display,
    })
}

fn resolve_bridge_cli_command() -> Result<ResolvedCommand, String> {
    let (plugin_id, config) = allthecodes_plugins::discover_plugin_mcp_servers_scoped()
        .into_iter()
        .find(|(plugin_id, config)| {
            config.name == BRIDGE_CLI_MCP_SERVER
                && plugin_id
                    .split('@')
                    .next()
                    .is_some_and(|name| name == BRIDGE_CLI_PLUGIN_NAME)
        })
        .ok_or_else(|| {
            format!(
                "Bridge CLI plugin is not installed or did not contribute MCP server '{}'",
                BRIDGE_CLI_MCP_SERVER
            )
        })?;
    if config.disabled == Some(true) {
        return Err(format!(
            "Bridge CLI MCP server '{}' is disabled",
            BRIDGE_CLI_MCP_SERVER
        ));
    }
    if config.transport != "stdio" {
        return Err(format!(
            "Bridge CLI MCP server '{}' must use stdio transport",
            BRIDGE_CLI_MCP_SERVER
        ));
    }
    let command = config.command.as_deref().ok_or_else(|| {
        format!(
            "Bridge CLI MCP server '{}' has no command",
            BRIDGE_CLI_MCP_SERVER
        )
    })?;
    let command_path = PathBuf::from(command);
    if command_path.is_absolute() && !command_path.exists() {
        return Err(format!(
            "Bridge CLI MCP command was not found at {}",
            command_path.display()
        ));
    }
    let plugin_root = allthecodes_plugins::find_plugin(&plugin_id)
        .and_then(|plugin| plugin.cache_path)
        .ok_or_else(|| {
            format!(
                "Bridge CLI plugin '{}' is registered without a plugin root",
                plugin_id
            )
        })?;
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| find_on_path("bash"))
        .ok_or_else(|| "shell command was not found in SHELL or PATH".to_string())?;
    let server_args = config.args.unwrap_or_else(|| vec!["serve".to_string()]);
    let args = vec![
        "-lc".to_string(),
        bridge_cli_shell_command(&plugin_root, command, &server_args),
    ];
    let display = display_command(&shell, &args);
    Ok(ResolvedCommand {
        executable: shell,
        args,
        display,
    })
}

fn bridge_cli_shell_command(plugin_root: &Path, command: &str, serve_args: &[String]) -> String {
    let serve_args = if serve_args.is_empty() {
        vec!["serve".to_string()]
    } else {
        serve_args.to_vec()
    };
    let exec_command = std::iter::once(shell_quote(command))
        .chain(serve_args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "cd {} && {} doctor && echo {} && exec {}",
        shell_quote(&plugin_root.to_string_lossy()),
        shell_quote(command),
        shell_quote("[bridge] starting MCP stdio server..."),
        exec_command
    )
}

fn shell_quote(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn resolve_path_command(command: &str, args: &[&str]) -> Result<ResolvedCommand, String> {
    let executable = find_on_path(command)
        .ok_or_else(|| format!("{} command was not found in PATH", command))?;
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let display = display_command(&executable, &args);
    Ok(ResolvedCommand {
        executable,
        args,
        display,
    })
}

fn display_command(executable: &Path, args: &[String]) -> String {
    let mut parts = vec![executable.display().to_string()];
    parts.extend(args.iter().cloned());
    parts.join(" ")
}

fn find_allthecodes_binary() -> Option<PathBuf> {
    for path in [
        "target/debug/allthecodes",
        "target/release/allthecodes",
        "../target/debug/allthecodes",
        "../target/release/allthecodes",
        "../../target/debug/allthecodes",
        "../../target/release/allthecodes",
    ] {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    find_on_path("allthecodes")
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(command);
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

fn decrement_atomic_counter(counter: &AtomicU64) -> u64 {
    loop {
        let current = counter.load(Ordering::SeqCst);
        if current == 0 {
            return 0;
        }
        let next = current - 1;
        if counter
            .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return next;
        }
    }
}

fn configure_terminal_environment(cmd: &mut CommandBuilder) {
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_cli_profile_id_resolves() {
        let profile = TerminalProfile::from_id("allthecodes-bridge-cli").unwrap();
        assert_eq!(profile, TerminalProfile::BridgeCli);
        assert_eq!(profile.id(), "allthecodes-bridge-cli");
        assert_eq!(profile.default_label(), "Bridge CLI");
    }

    #[test]
    fn custom_profile_id_resolves() {
        let profile = TerminalProfile::from_id("custom").unwrap();
        assert_eq!(profile, TerminalProfile::Custom);
        assert_eq!(profile.id(), "custom");
        assert_eq!(profile.default_label(), "Custom");
    }

    #[test]
    fn custom_command_uses_explicit_display_command() {
        let command = resolve_custom_command(TerminalCommandRequest {
            executable: "sh".to_string(),
            args: vec!["-lc".to_string(), "echo ok".to_string()],
            display: Some("plugin-provided command".to_string()),
        })
        .unwrap();

        assert!(command.executable.ends_with("sh"));
        assert_eq!(command.args, vec!["-lc", "echo ok"]);
        assert_eq!(command.display, "plugin-provided command");
    }

    #[test]
    fn terminal_environment_enables_color_output() {
        let mut cmd = CommandBuilder::new("sh");
        cmd.env("TERM", "dumb");
        cmd.env("COLORTERM", "");
        cmd.env("NO_COLOR", "1");

        configure_terminal_environment(&mut cmd);

        assert_eq!(
            cmd.get_env("TERM").and_then(|value| value.to_str()),
            Some("xterm-256color")
        );
        assert_eq!(
            cmd.get_env("COLORTERM").and_then(|value| value.to_str()),
            Some("truecolor")
        );
        assert!(cmd.get_env("NO_COLOR").is_none());
    }

    #[test]
    fn bridge_cli_shell_command_runs_doctor_then_plugin_server() {
        let command = bridge_cli_shell_command(
            Path::new("/plugins/allthecodes-bridge-cli"),
            "/plugins/allthecodes-bridge-cli/target/release/allthecodes-bridge-cli",
            &["serve".to_string()],
        );
        assert!(command.contains("cd /plugins/allthecodes-bridge-cli"));
        assert!(command.contains("allthecodes-bridge-cli doctor"));
        assert!(command.contains("[bridge] starting MCP stdio server..."));
        assert!(command.contains(
            "exec /plugins/allthecodes-bridge-cli/target/release/allthecodes-bridge-cli serve"
        ));
    }
}
