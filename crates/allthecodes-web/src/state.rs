//! Shared state for the web server layer.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;

use allthecodes_engine::lifecycle::QueryEngine;

use crate::ws::tui::PtyDiagnostics;

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
    /// Session writer ownership. Chat SSE and TUI PTY must not both write to
    /// the same session at the same time.
    pub ownership: Arc<RwLock<SessionOwnership>>,
}

impl WebState {
    /// Build a new `WebState` from an initial engine.
    pub fn new(engine: Arc<QueryEngine>, is_streaming: Arc<AtomicBool>) -> Self {
        Self {
            engine_slot: Arc::new(RwLock::new(engine)),
            is_streaming,
            pty_diagnostics: PtyDiagnostics::default(),
            ownership: Arc::new(RwLock::new(SessionOwnership::default())),
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
        self.ownership.read().clone()
    }

    pub fn try_claim_chat(&self, session_id: String) -> Result<(), SessionOwnership> {
        self.try_claim(SessionOwner::ChatStream, session_id)
    }

    pub fn try_claim_tui(&self, session_id: String) -> Result<(), SessionOwnership> {
        self.try_claim(SessionOwner::TuiPty, session_id)
    }

    fn try_claim(&self, owner: SessionOwner, session_id: String) -> Result<(), SessionOwnership> {
        let mut current = self.ownership.write();
        if current.owner != SessionOwner::None {
            return Err(current.clone());
        }
        *current = SessionOwnership {
            owner,
            session_id: Some(session_id),
        };
        Ok(())
    }

    pub fn release_owner(&self, owner: SessionOwner) {
        let mut current = self.ownership.write();
        if current.owner == owner {
            *current = SessionOwnership::default();
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOwner {
    #[default]
    None,
    ChatStream,
    TuiPty,
    IpcWs,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionOwnership {
    pub owner: SessionOwner,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}
