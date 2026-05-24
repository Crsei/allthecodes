//! Keybinding system — spec-aligned action vocabulary + JSON config +
//! hot-reload.
//!
//! Corresponds to: issue #10 +
//! `docs/claude-code-configuration/customize-keyboard-shortcuts.md`.
//!
//! # Layout
//!
//! - [`action`] — `namespace:action` vocabulary (shared with OpenTUI)
//! - [`context`] — [`Context`] enum (Global, Chat, Autocomplete, …)
//! - [`keystroke`] — parser for `ctrl+k`, chords, special keys, uppercase
//! - [`defaults`] — built-in default bindings (single source of truth)
//! - [`config`] — on-disk JSON shape + parser (supports `null` unbind)
//! - [`registry`] — merged default + user map with context-aware resolve
//!   and mtime-polled hot reload

pub mod action;
pub mod config;
pub mod context;
pub mod defaults;
pub mod keystroke;
pub mod registry;

// Re-export the primary handle consumed by AppState. The typed building
// blocks (Action, Chord, Context, …) are reached via `crate::keybindings::<mod>::…`
// from the few call sites that need them — no broad façade re-export.
pub use registry::KeybindingRegistry;
