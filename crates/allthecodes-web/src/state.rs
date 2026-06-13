//! Shared state for the web server layer.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::RwLock;

use allthecodes_config::paths;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_types::callbacks::PermissionResponsePayload;
use allthecodes_web_state::WebUiStore;

use crate::serialization::SerializationLayer;
use crate::ws::terminal::{PtyDiagnostics, TerminalManager};

/// Shared state passed to all Axum handlers via State extractor.
///
/// The engine is held behind an `RwLock<Arc<QueryEngine>>` so the web layer
/// can swap it out when the user creates a new session or resumes an older
/// one. Individual handlers snapshot the engine via [`WebState::engine`] and
/// operate on that `Arc` for the duration of the request, so a mid-flight
/// swap cannot disturb an in-progress stream.
#[derive(Clone)]
pub struct WebState {
    /// Current engine, swappable between turns.
    pub engine_slot: Arc<RwLock<Arc<QueryEngine>>>,
    /// Flag: is a query currently in progress?
    pub is_streaming: Arc<AtomicBool>,
    /// Session-scoped engines used by concurrent web chat turns.
    pub session_engines: Arc<RwLock<HashMap<String, Arc<QueryEngine>>>>,
    /// Session ids with an active streaming chat turn.
    pub streaming_sessions: Arc<RwLock<HashSet<String>>>,
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
}

impl WebState {
    /// Build a new `WebState` from an initial engine.
    pub fn new(engine: Arc<QueryEngine>, is_streaming: Arc<AtomicBool>) -> Self {
        let terminal_manager = TerminalManager::default();
        let current_session_id = engine.current_session_id().to_string();
        let mut session_engines = HashMap::new();
        session_engines.insert(current_session_id, engine.clone());
        Self {
            engine_slot: Arc::new(RwLock::new(engine)),
            is_streaming,
            session_engines: Arc::new(RwLock::new(session_engines)),
            streaming_sessions: Arc::new(RwLock::new(HashSet::new())),
            chat_permissions: Arc::new(RwLock::new(HashMap::new())),
            pty_diagnostics: PtyDiagnostics::new(terminal_manager.clone()),
            terminal_manager,
            serialization: SerializationLayer::new(),
            web_ui_store: WebUiStore::new(paths::data_root().join("web").join("state.db")),
        }
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
