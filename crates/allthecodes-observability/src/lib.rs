//! Observability subsystem — runtime audit event logging.
//!
//! This module provides the single runtime fact source for the traceable
//! logging system. All other recording surfaces (transcript, session snapshot,
//! audit export) are derived views.
//!
//! # Architecture
//!
//! ```text
//! AuditContext  ──emit()──>  AuditSink  ──write──>  events.ndjson
//!     │                                                  │
//!     ├─ session_id                                 append-only
//!     ├─ submit_id                                  NDJSON format
//!     ├─ turn_id                                    crash-safe prefix
//!     ├─ request_id
//!     └─ tool_use_id
//! ```
//!
//! # Usage
//!
//! 1. At process start, call `AuditSink::init()` to create the writer.
//! 2. Create `AuditContext::new()` with the session ID and sink.
//! 3. At each submit, call `ctx.with_submit()` for a new submit scope.
//! 4. Thread the context through query turns, model calls, tool executions.
//! 5. At shutdown, call `sink.sync()` for durability.
//!
//! See: docs/traceable-logging-plan.md

pub mod context;
pub mod event;
pub mod perfetto;
pub mod session_tracing;
pub mod sink;

// Re-export primary types for ergonomic use
pub use context::{AuditContext, AuditEmitInput};
pub use event::{AuditEvent, AuditLevel, EventKind, Outcome, SessionMeta, Stage};
pub use session_tracing::SessionTracingBridge;
pub use sink::{AuditConfig, AuditSink};

#[cfg(feature = "telemetry")]
pub use session_tracing::TelemetrySink;
