//! Telemetry bridge — allows the engine to emit telemetry spans without
//! depending on `cc-services` directly.
//!
//! The bridge is populated from `main.rs` (in `allthecodes`) via
//! [`install`] after the telemetry subsystem is initialized during Phase B
//! startup. Call sites in `submit_message.rs` use [`with_bridge`] to start
//! and end interaction / hook spans.
//!
//! All types are behind `#[cfg(feature = "telemetry")]` — when the feature
//! is disabled every function is a no-op.
//!
//! # Architecture (Phase 2, Serial Integration Lane)
//!
//! ```text
//! main.rs                              cc-engine
//!   │                                    │
//!   ├─ init_telemetry()                  │
//!   ├─ install(MyBridge { handle }) ────>│  telemetry_bridge::INSTANCE
//!   │                                    │
//!   │                          submit_message.rs
//!   │                            ├─ with_bridge(|b| b.start_submit(…))
//!   │                            └─ with_bridge(|b| b.end_submit(…))
//! ```

use std::sync::OnceLock;

/// Opaque span identifier returned by start callbacks.
pub type SpanId = u64;

/// Telemetry operations that the engine can call.
///
/// Implementors store the concrete `TelemetryHandle` (from
/// `cc-services::telemetry`) and manage span lifecycle internally.
pub trait EngineTelemetry: Send + Sync {
    /// Called when a user message submit begins.
    fn start_submit(&self, session_id: &str, submit_id: &str) -> SpanId;

    /// Called when a submit completes (success or error).
    fn end_submit(&self, span_id: SpanId, model: &str, input_tokens: u64, output_tokens: u64);

    /// Called before a hook (e.g. `UserPromptSubmit`) runs.
    fn start_hook(&self, hook_name: &str) -> SpanId;

    /// Called after a hook completes.
    fn end_hook(&self, span_id: SpanId, result: &str);
}

static INSTANCE: OnceLock<Box<dyn EngineTelemetry>> = OnceLock::new();

/// Install the global telemetry bridge.
///
/// Must be called once during Phase B startup, before any submit_message call.
/// Returns the supplied bridge when another bridge is already installed.
pub fn install(bridge: Box<dyn EngineTelemetry>) -> Result<(), Box<dyn EngineTelemetry>> {
    INSTANCE.set(bridge)
}

/// Access the bridge and call `f` with it.
///
/// Returns `None` when the bridge has not been installed (e.g. telemetry
/// feature is disabled or startup hasn't called `install` yet).
pub fn with_bridge<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&dyn EngineTelemetry) -> R,
{
    INSTANCE.get().map(|b| f(b.as_ref()))
}

/// True when a telemetry bridge has been installed.
pub fn is_active() -> bool {
    INSTANCE.get().is_some()
}
