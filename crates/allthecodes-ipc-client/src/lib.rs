//! IPC client helpers for headless/TUI-facing transports.
//!
//! This crate owns client-side protocol parsing, frontend egress, callback
//! bridges, query-turn spawning helpers, and lossless/best-effort event
//! classification. Runtime orchestration remains in the host until the
//! follow-up `cc-ipc` extraction.

pub mod callbacks;
pub mod event_class;
pub mod ingress;
pub mod query_runner;
pub mod requests;
pub mod sdk_mapping;
pub mod sink;
pub mod transport;

pub use callbacks::{PendingPermissions, PendingQuestions};
pub use event_class::{EventClass, QueuePressureDiagnostic};
pub use requests::{AppServerRequests, PendingRequest};
pub use sink::FrontendSink;
