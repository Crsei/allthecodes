//! Shared state for the web server layer.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::RwLock;

use allthecodes_config::paths;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_web_state::WebUiStore;

use crate::serialization::SerializationLayer;
pub use crate::serialization::{SessionOwner, SessionOwnership};
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
        Self {
            engine_slot: Arc::new(RwLock::new(engine)),
            is_streaming,
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
        *self.engine_slot.write() = engine;
    }

    pub fn ownership_snapshot(&self) -> SessionOwnership {
        self.serialization.ownership_snapshot()
    }

    pub fn try_claim_chat(&self, session_id: String) -> Result<(), SessionOwnership> {
        self.try_claim(SessionOwner::ChatStream, session_id)
    }

    pub fn try_claim_tui(&self, session_id: String) -> Result<(), SessionOwnership> {
        self.try_claim(SessionOwner::TuiPty, session_id)
    }

    pub fn try_claim(
        &self,
        owner: SessionOwner,
        session_id: String,
    ) -> Result<(), SessionOwnership> {
        self.serialization.try_claim_owner(owner, session_id)
    }

    pub fn release_owner(&self, owner: SessionOwner) {
        self.serialization.release_owner(owner);
    }
}
