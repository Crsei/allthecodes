//! Session persistence — extracted as a workspace crate in Phase 4
//! (issue #73).
//!
//! Writes conversation state to `~/.allthecodes/memory/` (path unchanged by
//! the split — the acceptance test in the issue explicitly calls that
//! out). Depends on cc-bootstrap (process state), cc-compact (for the
//! export pipeline's context-window helpers), cc-types (message types),
//! cc-utils (token estimates).

pub mod audit_export;
pub mod export;
pub mod fork;
pub mod memdir;
pub mod migrations;
pub mod record_replay;
pub mod request_snapshot;
pub mod resume;
pub mod session_export;
pub mod session_report;
pub mod storage;
pub mod transcript;
pub mod worktree_sessions;
