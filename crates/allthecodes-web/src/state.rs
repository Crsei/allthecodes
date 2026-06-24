//! Shared state for the web server layer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_config::paths;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_types::callbacks::PermissionResponsePayload;
use allthecodes_web_state::WebUiStore;

use crate::ipc_streams::IpcSessionHub;
use crate::serialization::SerializationLayer;
use crate::ws::terminal::{PtyDiagnostics, TerminalManager};

/// An in-memory queue entry used by the queue/edit/send-now API.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub id: String,
    pub text: String,
    pub session_id: String,
    pub created_at: String,
}

/// Shared state passed to all Axum handlers via State extractor.
///
/// The engine is held behind an `RwLock<Arc<QueryEngine>>` so the web layer
/// can swap it out when the user creates a new session or resumes an older
/// one. Individual handlers snapshot the engine via [`WebState::engine`] and
/// operate on that `Arc` for the duration of the request, so a mid-flight
/// swap cannot disturb an in-progress stream.
#[derive(Clone)]
pub struct WebState {
    /// Application version reported to Web API clients.
    app_version: Arc<str>,
    /// Current engine, swappable between turns.
    pub engine_slot: Arc<RwLock<Arc<QueryEngine>>>,
    /// Flag: is a query currently in progress?
    pub is_streaming: Arc<AtomicBool>,
    /// Session-scoped engines used by concurrent web chat turns.
    pub session_engines: Arc<RwLock<HashMap<String, Arc<QueryEngine>>>>,
    /// Session ids with an active streaming chat turn.
    pub streaming_sessions: Arc<RwLock<HashSet<String>>>,
    /// Session-scoped IPC WebSocket stream hubs.
    pub ipc_session_hubs: Arc<RwLock<HashMap<String, Arc<IpcSessionHub>>>>,
    /// Pending tool permission prompts owned by `/api/chat` SSE turns.
    chat_permissions: Arc<
        RwLock<HashMap<ChatPermissionKey, tokio::sync::oneshot::Sender<PermissionResponsePayload>>>,
    >,
    /// PTY diagnostics for the active TUI WebSocket connection.
    pub pty_diagnostics: PtyDiagnostics,
    /// Multi-session PTY manager used by the terminal panel.
    pub terminal_manager: TerminalManager,
    /// Request serialization and cross-transport ownership coordination.
    pub serialization: SerializationLayer,
    /// Profile-scoped persistence for browser-only UI state.
    pub web_ui_store: WebUiStore,
    /// In-memory prompt queue for the queue/edit/send-now API.
    pub queue: Arc<RwLock<VecDeque<QueueEntry>>>,
    /// Desktop account auth state shared by account auth endpoints.
    pub account_auth: Arc<Mutex<AccountAuthMemory>>,
}

#[derive(Debug, Clone, Default)]
pub struct AccountAuthMemory {
    pub pending: Option<PendingAccountLogin>,
    pub session: Option<AccountAuthSession>,
}

#[derive(Debug, Clone)]
pub struct PendingAccountLogin {
    pub account_site_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAuthSession {
    pub account_site_url: String,
    pub access_token: String,
    pub expires_at: String,
    pub user: Value,
    pub subscription: Value,
    pub entitlements: Value,
    pub credits: Value,
    pub agent_collaboration: Value,
}

impl WebState {
    /// Build a new `WebState` from an initial engine.
    pub fn new(engine: Arc<QueryEngine>, is_streaming: Arc<AtomicBool>) -> Self {
        Self::new_with_version(engine, is_streaming, env!("CARGO_PKG_VERSION"))
    }

    /// Build a new `WebState` with an explicit application version.
    pub fn new_with_version(
        engine: Arc<QueryEngine>,
        is_streaming: Arc<AtomicBool>,
        app_version: impl Into<String>,
    ) -> Self {
        let terminal_manager = TerminalManager::default();
        let current_session_id = engine.current_session_id().to_string();
        let mut session_engines = HashMap::new();
        session_engines.insert(current_session_id, engine.clone());
        let state = Self {
            app_version: Arc::from(app_version.into()),
            engine_slot: Arc::new(RwLock::new(engine)),
            is_streaming,
            session_engines: Arc::new(RwLock::new(session_engines)),
            streaming_sessions: Arc::new(RwLock::new(HashSet::new())),
            ipc_session_hubs: Arc::new(RwLock::new(HashMap::new())),
            chat_permissions: Arc::new(RwLock::new(HashMap::new())),
            pty_diagnostics: PtyDiagnostics::new(terminal_manager.clone()),
            terminal_manager,
            serialization: SerializationLayer::new(),
            web_ui_store: WebUiStore::new(paths::data_root().join("web").join("state.db")),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            account_auth: Arc::new(Mutex::new(AccountAuthMemory::default())),
        };
        install_plugin_mcp_hooks();
        state.install_plugin_account_token_provider();
        state
    }

    /// Application version reported to Web clients.
    pub fn app_version(&self) -> &str {
        self.app_version.as_ref()
    }

    /// Snapshot the current engine.
    pub fn engine(&self) -> Arc<QueryEngine> {
        self.engine_slot.read().clone()
    }

    /// Replace the current engine (used by new/resume session flows).
    pub fn replace_engine(&self, engine: Arc<QueryEngine>) {
        let session_id = engine.current_session_id().to_string();
        *self.engine_slot.write() = engine.clone();
        self.session_engines.write().insert(session_id, engine);
    }

    /// Return the cached engine for a session, if one has been built.
    pub fn engine_for_session(&self, session_id: &str) -> Option<Arc<QueryEngine>> {
        self.session_engines.read().get(session_id).cloned()
    }

    /// Cache an engine without making it the foreground UI engine.
    pub fn cache_session_engine(&self, engine: Arc<QueryEngine>) {
        let session_id = engine.current_session_id().to_string();
        self.session_engines.write().insert(session_id, engine);
    }

    /// Check whether a specific session has an active streaming turn.
    pub fn is_session_streaming(&self, session_id: &str) -> bool {
        self.streaming_sessions.read().contains(session_id)
    }

    /// Update a session's streaming state and keep the legacy global flag in sync.
    pub fn set_session_streaming(&self, session_id: &str, streaming: bool) {
        let any_streaming = {
            let mut sessions = self.streaming_sessions.write();
            if streaming {
                sessions.insert(session_id.to_string());
            } else {
                sessions.remove(session_id);
            }
            !sessions.is_empty()
        };
        self.is_streaming
            .store(any_streaming, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn ipc_session_hub(&self, session_id: &str) -> Arc<IpcSessionHub> {
        if let Some(hub) = self.ipc_session_hubs.read().get(session_id).cloned() {
            return hub;
        }

        let mut hubs = self.ipc_session_hubs.write();
        hubs.entry(session_id.to_string())
            .or_insert_with(|| IpcSessionHub::new(session_id))
            .clone()
    }

    pub fn insert_chat_permission(
        &self,
        session_id: &str,
        tool_use_id: &str,
        sender: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
    ) {
        self.chat_permissions
            .write()
            .insert(ChatPermissionKey::new(session_id, tool_use_id), sender);
    }

    pub fn resolve_chat_permission(
        &self,
        session_id: &str,
        tool_use_id: &str,
        response: PermissionResponsePayload,
    ) -> bool {
        self.chat_permissions
            .write()
            .remove(&ChatPermissionKey::new(session_id, tool_use_id))
            .map(|sender| sender.send(response).is_ok())
            .unwrap_or(false)
    }

    pub fn remove_chat_permission(&self, session_id: &str, tool_use_id: &str) {
        self.chat_permissions
            .write()
            .remove(&ChatPermissionKey::new(session_id, tool_use_id));
    }

    fn install_plugin_account_token_provider(&self) {
        let account_auth = self.account_auth.clone();
        allthecodes_plugins::set_plugin_account_token_provider(Some(Arc::new(move || {
            if let Some(session) = account_auth.lock().session.clone() {
                if account_auth_session_expired(&session) {
                    return Err(anyhow!("desktop account session is expired"));
                }
                return Ok(session.access_token);
            }

            stored_account_access_token()?.ok_or_else(|| anyhow!("no desktop account session"))
        })));
    }
}

fn account_auth_session_expired(session: &AccountAuthSession) -> bool {
    let Ok(expires_at) = DateTime::parse_from_rfc3339(&session.expires_at) else {
        return true;
    };
    Utc::now() >= expires_at.with_timezone(&Utc)
}

#[derive(Debug, Deserialize)]
struct StoredAccountAuthFile {
    auth_mode: String,
    tokens: StoredAccountAuthTokens,
}

#[derive(Debug, Deserialize)]
struct StoredAccountAuthTokens {
    access_token: String,
}

fn stored_account_access_token() -> anyhow::Result<Option<String>> {
    let path = paths::data_root().join("auth.json");
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let auth_file: StoredAccountAuthFile = serde_json::from_str(&raw)?;
    if auth_file.auth_mode != "allthecodes" {
        return Ok(None);
    }

    let token = auth_file.tokens.access_token.trim();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token.to_string()))
    }
}

/// Install plugin MCP discovery plus a startup-safe account token provider.
///
/// `WebState` installs a richer provider later that can use its in-memory
/// account session. During early process startup the MCP manager is created
/// before `WebState`, so it still needs discovery hooks and stored auth.
pub fn install_plugin_runtime_hooks() {
    install_plugin_mcp_hooks();
    install_stored_plugin_account_token_provider();
}

fn install_stored_plugin_account_token_provider() {
    allthecodes_plugins::set_plugin_account_token_provider(Some(Arc::new(|| {
        stored_account_access_token()?.ok_or_else(|| anyhow!("no desktop account session"))
    })));
}

fn install_plugin_mcp_hooks() {
    allthecodes_mcp::discovery::set_plugin_hook(allthecodes_plugins::discover_plugin_mcp_servers);
    allthecodes_mcp::discovery::set_scoped_plugin_hook(
        allthecodes_plugins::discover_plugin_mcp_servers_scoped,
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChatPermissionKey {
    session_id: String,
    tool_use_id: String,
}

impl ChatPermissionKey {
    fn new(session_id: &str, tool_use_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            tool_use_id: tool_use_id.to_string(),
        }
    }
}
