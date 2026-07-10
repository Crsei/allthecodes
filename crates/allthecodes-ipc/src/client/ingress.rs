//! Client-side ingress helpers shared by headless runtimes.

use super::callbacks::{PendingPermissions, PendingQuestions};
use allthecodes_types::callbacks::PermissionResponsePayload;

/// Complete a pending permission response by tool-use id.
pub fn complete_pending_permission(
    pending_permissions: &PendingPermissions,
    tool_use_id: &str,
    response: PermissionResponsePayload,
) -> bool {
    pending_permissions
        .lock()
        .remove(tool_use_id)
        .map(|tx| tx.send(response).is_ok())
        .unwrap_or(false)
}

/// Complete a pending question response by id.
pub fn complete_pending_question(
    pending_questions: &PendingQuestions,
    id: &str,
    text: String,
) -> bool {
    pending_questions
        .lock()
        .remove(id)
        .map(|tx| tx.send(text).is_ok())
        .unwrap_or(false)
}

/// Backward-compatible submit-prompt fallback for a pending AskUserQuestion.
pub fn try_answer_pending_question(
    pending_questions: &PendingQuestions,
    text: String,
) -> Option<String> {
    let mut pending = pending_questions.lock();
    if pending.len() != 1 {
        return None;
    }
    let pending_id = pending.keys().next().cloned()?;
    let tx = pending.remove(&pending_id)?;
    drop(pending);
    let _ = tx.send(text);
    Some(pending_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[test]
    fn routes_submit_prompt_to_pending_question() {
        let pending: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().insert("question-1".to_string(), tx);

        let routed = try_answer_pending_question(&pending, "my answer".to_string());

        assert_eq!(routed.as_deref(), Some("question-1"));
        assert!(pending.lock().is_empty());
        assert_eq!(rx.blocking_recv().unwrap(), "my answer");
    }

    #[test]
    fn completes_question_by_id() {
        let pending: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().insert("q-1".to_string(), tx);

        assert!(complete_pending_question(
            &pending,
            "q-1",
            "answer".to_string()
        ));
        assert_eq!(rx.blocking_recv().unwrap(), "answer");
    }
}
