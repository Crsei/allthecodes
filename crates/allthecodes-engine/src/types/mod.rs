//! Engine-level shared types.
//!
//! Moved from `crates/allthecodes/src/types/{app_state, tool, config}.rs`
//! in Phase 6. The pure leaf types (`message`, `state`, `transitions`,
//! `permissions`, `hooks`, `commands`, `agent_*`, `teams`) live in
//! `cc-types`; the three modules here depend on them plus a
//! handful of runtime-bound sibling crates (`cc-keybindings`, `cc-config`) and
//! this crate's own `status_line` module.
//!
//! The root crate re-exports these via `src/types/mod.rs` so existing
//! `crate::types::{app_state, tool, config}` import paths keep working.

pub mod app_state;
pub mod config;
pub mod tool;

// Re-export the pure-data modules from cc-types so a consumer doing
// `use allthecodes_engine::types::message::*` or `use allthecodes_engine::types::state::*`
// lines up with the pre-move shape of `crate::types::*`.
pub use allthecodes_types::{message, permissions, state, transitions};
