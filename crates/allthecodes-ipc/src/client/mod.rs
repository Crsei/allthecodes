//! IPC client helpers for headless/TUI-facing transports.
//!
//! This module owns client-side protocol parsing, frontend egress, callback
//! bridges, query-turn spawning helpers, and lossless/best-effort event
//! classification. Runtime orchestration remains in the host until the
//! follow-up `cc-ipc` extraction.

pub mod callbacks;
pub mod ingress;
pub mod query_runner;
pub mod requests;

pub use callbacks::{PendingPermissions, PendingQuestions};
pub use requests::{AppServerRequests, PendingRequest};
