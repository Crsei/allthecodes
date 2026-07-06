use std::sync::{Arc, LazyLock};

use std::sync::RwLock;

type ContextBlockedCallback = Arc<dyn Fn(bool, &str) + Send + Sync>;

#[derive(Default)]
struct ContextBlockedState {
    last: Option<(bool, String)>,
    callback: Option<ContextBlockedCallback>,
}

static CONTEXT_BLOCKED_STATE: LazyLock<RwLock<ContextBlockedState>> =
    LazyLock::new(|| RwLock::new(ContextBlockedState::default()));

pub fn register_context_blocked_callback(callback: ContextBlockedCallback) {
    let last = {
        let mut state = CONTEXT_BLOCKED_STATE
            .write()
            .expect("context state poisoned");
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
            .expect("context state poisoned");
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
        .expect("context state poisoned")
        .last
        .as_ref()
        .is_some_and(|(blocked, _)| *blocked)
}
