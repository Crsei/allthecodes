//! Pure leaf types extracted from `allthecodes::types`.
//!
//! This crate holds the subset of `src/types/` with no cross-module dependencies
//! on the main crate (teams / ui / config / ipc). The three modules below are
//! the truly pure leaves; `app_state`, `tool`, and `config` remain in the root
//! crate because they still reach into the not-yet-extracted subsystems.
//!
//! See issue #70 (`[workspace-split] Phase 1`) for the rationale behind this
//! partial split.
#[cfg(feature = "runtime-types")]
pub mod agent_channel;
pub mod agent_events;
pub mod agent_types;
pub mod callbacks;
pub mod commands;
#[cfg(feature = "runtime-types")]
pub mod hooks;
pub mod mcp;
pub mod message;
pub mod models;
pub mod output;
pub mod permission_events;
#[cfg(feature = "runtime-types")]
pub mod permissions;
pub mod plan_workflow;
pub mod query_host;
pub mod sdk;
pub mod state;
pub mod status_line;
pub mod teams;
pub mod tool_operation;
pub mod transitions;

pub use output::{
    EventSeq, OutputEvent, OutputLifecycleState, OutputReadBatch, OutputStream,
    DEFAULT_DETACH_RESUME_TTL, DEFAULT_EXITED_OUTPUT_RETENTION_TTL, DEFAULT_OUTPUT_RETENTION_BYTES,
};
