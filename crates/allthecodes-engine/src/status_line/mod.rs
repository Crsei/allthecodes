//! Scriptable status-line subsystem.
//!
//! Implements the spec in `customize-status-line.md` + issue #11:
//!
//! 1. `payload` — [`StatusLinePayload`] and its nested structs (the JSON
//!    piped to the user's command on stdin).
//! 2. `runner` — [`StatusLineRunner`], the throttled / cancellable
//!    subprocess driver.
//!
//! The default terminal footer (see [`super::app::App::render_status_bar`])
//! is always used as the fallback when no custom statusLine is configured
//! or when the script errors / times out.

pub mod payload;
pub mod runner;

// Re-export the typed building blocks and the payload helpers used by the
// TUI and IPC layers to assemble one shared status snapshot shape.
pub use payload::{
    build_payload_from_snapshot, ContextWindowStatus, CostStatus, ModelInfo, StatusLinePayload,
    StatusLineSnapshot, VimStatus, WorkspaceStatus, WorktreeStatus,
};
pub use runner::{StatusLineOutput, StatusLineRunner};
