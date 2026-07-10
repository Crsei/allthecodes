use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

type ContextBlockedCallback = Arc<dyn Fn(bool, &str) + Send + Sync>;

#[derive(Default)]
struct ContextBlockedState {
    last: Option<(bool, String)>,
    callback: Option<ContextBlockedCallback>,
}

static CONTEXT_BLOCKED_STATE: LazyLock<RwLock<ContextBlockedState>> =
    LazyLock::new(|| RwLock::new(ContextBlockedState::default()));
static PROACTIVE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn register_context_blocked_callback(callback: ContextBlockedCallback) {
    let last = {
        let mut state = CONTEXT_BLOCKED_STATE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.callback = Some(callback.clone());
        state.last.clone()
    };

    if let Some((true, reason)) = last {
        callback(true, &reason);
    }
}

pub fn set_context_blocked(blocked: bool, reason: &str) {
    let callback = {
        let mut state = CONTEXT_BLOCKED_STATE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.last = Some((blocked, reason.trim().to_string()));
        state.callback.clone()
    };

    if let Some(callback) = callback {
        callback(blocked, reason);
    }
}

pub fn is_context_blocked() -> bool {
    CONTEXT_BLOCKED_STATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last
        .as_ref()
        .is_some_and(|(blocked, _)| *blocked)
}

pub fn set_proactive_active(active: bool) {
    PROACTIVE_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn is_proactive_active() -> bool {
    PROACTIVE_ACTIVE.load(Ordering::SeqCst)
}
