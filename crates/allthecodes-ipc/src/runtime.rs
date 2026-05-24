//! Headless IPC runtime state.

use std::collections::HashMap;
use std::sync::Arc;

use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_types::callbacks::PermissionResponsePayload;
use parking_lot::Mutex;
use tokio::sync::oneshot;

pub type PendingPermissions =
    Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponsePayload>>>>;
pub type PendingQuestions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScopedInteractionKey {
    session_id: String,
    turn_id: String,
    id: String,
}

impl ScopedInteractionKey {
    fn new(session_id: &str, turn_id: &str, id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            id: id.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct PendingInteractions {
    legacy_permissions: PendingPermissions,
    legacy_questions: PendingQuestions,
    scoped_permissions:
        Arc<Mutex<HashMap<ScopedInteractionKey, oneshot::Sender<PermissionResponsePayload>>>>,
    scoped_questions: Arc<Mutex<HashMap<ScopedInteractionKey, oneshot::Sender<String>>>>,
}

impl PendingInteractions {
    pub fn new() -> Self {
        Self {
            legacy_permissions: Arc::new(Mutex::new(HashMap::new())),
            legacy_questions: Arc::new(Mutex::new(HashMap::new())),
            scoped_permissions: Arc::new(Mutex::new(HashMap::new())),
            scoped_questions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn legacy_permissions(&self) -> PendingPermissions {
        self.legacy_permissions.clone()
    }

    pub fn legacy_questions(&self) -> PendingQuestions {
        self.legacy_questions.clone()
    }

    pub fn insert_scoped_permission(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_use_id: &str,
        sender: oneshot::Sender<PermissionResponsePayload>,
    ) {
        self.scoped_permissions.lock().insert(
            ScopedInteractionKey::new(session_id, turn_id, tool_use_id),
            sender,
        );
    }

    pub fn insert_scoped_question(
        &self,
        session_id: &str,
        turn_id: &str,
        id: &str,
        sender: oneshot::Sender<String>,
    ) {
        self.scoped_questions
            .lock()
            .insert(ScopedInteractionKey::new(session_id, turn_id, id), sender);
    }

    pub fn complete_permission(
        &self,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        tool_use_id: &str,
        response: PermissionResponsePayload,
    ) -> bool {
        if let (Some(session_id), Some(turn_id)) = (session_id, turn_id) {
            let key = ScopedInteractionKey::new(session_id, turn_id, tool_use_id);
            if let Some(tx) = self.scoped_permissions.lock().remove(&key) {
                return tx.send(response).is_ok();
            }
        }

        self.legacy_permissions
            .lock()
            .remove(tool_use_id)
            .map(|tx| tx.send(response).is_ok())
            .unwrap_or(false)
    }

    pub fn complete_question(
        &self,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        id: &str,
        text: String,
    ) -> bool {
        if let (Some(session_id), Some(turn_id)) = (session_id, turn_id) {
            let key = ScopedInteractionKey::new(session_id, turn_id, id);
            if let Some(tx) = self.scoped_questions.lock().remove(&key) {
                return tx.send(text).is_ok();
            }
        }

        self.legacy_questions
            .lock()
            .remove(id)
            .map(|tx| tx.send(text).is_ok())
            .unwrap_or(false)
    }

    pub fn try_answer_any_question(&self, text: String) -> Option<String> {
        let mut pending = self.legacy_questions.lock();
        let pending_id = pending.keys().next().cloned()?;
        let tx = pending.remove(&pending_id)?;
        drop(pending);
        let _ = tx.send(text);
        Some(pending_id)
    }
}

impl Default for PendingInteractions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TurnRuntime {
    pub session_id: String,
    pub turn_id: String,
    pub run_id: String,
}

#[derive(Clone)]
pub struct SessionRuntime {
    session_id: String,
    run_id: String,
    current_turn: Arc<Mutex<Option<TurnRuntime>>>,
    pending: PendingInteractions,
}

impl SessionRuntime {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            current_turn: Arc::new(Mutex::new(None)),
            pending: PendingInteractions::new(),
        }
    }

    pub fn from_ready_message(message: &BackendMessage) -> Self {
        let session_id = match message {
            BackendMessage::Ready { session_id, .. } => session_id.clone(),
            _ => "legacy-session".to_string(),
        };
        Self::new(session_id)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn pending_interactions(&self) -> &PendingInteractions {
        &self.pending
    }

    pub fn begin_turn(&self, turn_id: impl Into<String>) -> TurnRuntime {
        let turn = TurnRuntime {
            session_id: self.session_id.clone(),
            turn_id: turn_id.into(),
            run_id: self.run_id.clone(),
        };
        *self.current_turn.lock() = Some(turn.clone());
        turn
    }

    pub fn current_turn(&self) -> Option<TurnRuntime> {
        self.current_turn.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_permission_matches_session_and_turn() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending.insert_scoped_permission("session-1", "turn-1", "tool-1", tx);

        assert!(!pending.complete_permission(
            Some("session-1"),
            Some("wrong-turn"),
            "tool-1",
            PermissionResponsePayload::decision("deny"),
        ));
        assert!(pending.complete_permission(
            Some("session-1"),
            Some("turn-1"),
            "tool-1",
            PermissionResponsePayload::decision("allow"),
        ));
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PermissionResponsePayload::decision("allow")
        );
    }

    #[test]
    fn legacy_permission_fallback_still_resolves() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending
            .legacy_permissions()
            .lock()
            .insert("tool-1".to_string(), tx);

        assert!(pending.complete_permission(
            None,
            None,
            "tool-1",
            PermissionResponsePayload::decision("allow")
        ));
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PermissionResponsePayload::decision("allow")
        );
    }

    #[test]
    fn scoped_question_matches_session_and_turn() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending.insert_scoped_question("session-1", "turn-1", "question-1", tx);

        assert!(pending.complete_question(
            Some("session-1"),
            Some("turn-1"),
            "question-1",
            "yes".to_string(),
        ));
        assert_eq!(rx.blocking_recv().unwrap(), "yes");
    }

    #[test]
    fn session_runtime_tracks_current_turn() {
        let runtime = SessionRuntime::new("session-1");
        let turn = runtime.begin_turn("turn-1");

        assert_eq!(turn.session_id, "session-1");
        assert_eq!(runtime.current_turn().unwrap().turn_id, "turn-1");
        assert_eq!(runtime.session_id(), "session-1");
        assert!(!runtime.run_id().is_empty());
    }
}
