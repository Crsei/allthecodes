//! Hook event system for broadcasting hook execution events.
//!
//! This module provides a generic event system that is separate from the
//! main message stream. Handlers can register to receive events and decide
//! what to do with them.
//!
//! Port of TypeScript `hookEvents.ts`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use allthecodes_types::hooks::{
    HookEvent, HookExecutionEvent, HookOutcome, HookResponseEvent, HookStartedEvent,
};

/// Hook events that are always emitted regardless of configuration.
const ALWAYS_EMITTED_EVENTS: &[HookEvent] = &[HookEvent::SessionStart, HookEvent::Setup];

const MAX_PENDING_EVENTS: usize = 100;

type HookEventHandler = Box<dyn Fn(HookExecutionEvent) + Send>;

static EVENT_HANDLER: LazyLock<Mutex<Option<HookEventHandler>>> =
    LazyLock::new(|| Mutex::new(None));

static ALL_HOOK_EVENTS_ENABLED: AtomicBool = AtomicBool::new(false);

static PENDING_EVENTS: LazyLock<Mutex<Vec<HookExecutionEvent>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Register a hook event handler. Replaces any previously registered handler.
/// If there are pending events, they are immediately forwarded to the new handler.
pub fn register_hook_event_handler(handler: Option<HookEventHandler>) {
    let mut guard = EVENT_HANDLER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = handler;

    if guard.is_some() {
        let mut pending = PENDING_EVENTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for event in pending.drain(..) {
            if let Some(ref h) = *guard {
                h(event);
            }
        }
    }
}

fn emit(event: HookExecutionEvent) {
    let handler = EVENT_HANDLER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(ref h) = *handler {
        h(event);
    } else {
        let mut pending = PENDING_EVENTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_EVENTS {
            pending.remove(0);
        }
        pending.push(event);
    }
}

fn should_emit(hook_event: &HookEvent) -> bool {
    if ALWAYS_EMITTED_EVENTS.contains(hook_event) {
        return true;
    }
    ALL_HOOK_EVENTS_ENABLED.load(Ordering::Relaxed)
}

/// Emit a hook started event.
pub fn emit_hook_started(hook_id: &str, hook_name: &str, hook_event: &HookEvent) {
    if !should_emit(hook_event) {
        return;
    }

    emit(HookExecutionEvent::Started(HookStartedEvent {
        hook_id: hook_id.to_string(),
        hook_name: hook_name.to_string(),
        hook_event: hook_event.to_string(),
    }));
}

/// Emit a hook response event.
pub struct HookResponseEmit<'a> {
    pub hook_id: &'a str,
    pub hook_name: &'a str,
    pub hook_event: &'a HookEvent,
    pub output: &'a str,
    pub stdout: &'a str,
    pub stderr: &'a str,
    pub exit_code: Option<i32>,
    pub outcome: HookOutcome,
}

pub fn emit_hook_response(event: HookResponseEmit<'_>) {
    emit(HookExecutionEvent::Response(HookResponseEvent {
        hook_id: event.hook_id.to_string(),
        hook_name: event.hook_name.to_string(),
        hook_event: event.hook_event.to_string(),
        output: event.output.to_string(),
        stdout: event.stdout.to_string(),
        stderr: event.stderr.to_string(),
        exit_code: event.exit_code,
        outcome: event.outcome,
    }));
}

/// Enable emission of all hook event types (beyond SessionStart and Setup).
pub fn set_all_hook_events_enabled(enabled: bool) {
    ALL_HOOK_EVENTS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Clear all hook event state.
pub fn clear_hook_event_state() {
    *EVENT_HANDLER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    PENDING_EVENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    ALL_HOOK_EVENTS_ENABLED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn test_register_handler_drains_pending() {
        clear_hook_event_state();

        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        // Emit before handler is registered
        emit_hook_started("id1", "test", &HookEvent::SessionStart);

        // Now register handler — should drain pending
        register_hook_event_handler(Some(Box::new(move |evt| {
            events_clone.lock().unwrap().push(evt);
        })));

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 1);
        match &captured[0] {
            HookExecutionEvent::Started(s) => {
                assert_eq!(s.hook_id, "id1");
            }
            _ => panic!("expected Started event"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_always_emitted_events_fire_without_enable() {
        clear_hook_event_state();

        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        register_hook_event_handler(Some(Box::new(move |evt| {
            events_clone.lock().unwrap().push(evt);
        })));

        // SessionStart should fire without enable
        emit_hook_started("id2", "test2", &HookEvent::SessionStart);

        // Stop should NOT fire without enable
        emit_hook_started("id3", "test3", &HookEvent::Stop);

        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    #[serial_test::serial]
    fn test_enable_allows_all_events() {
        clear_hook_event_state();
        set_all_hook_events_enabled(true);

        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        register_hook_event_handler(Some(Box::new(move |evt| {
            events_clone.lock().unwrap().push(evt);
        })));

        emit_hook_started("id", "test", &HookEvent::Stop);
        assert_eq!(events.lock().unwrap().len(), 1);
    }
}
