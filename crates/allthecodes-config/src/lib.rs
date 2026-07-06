//! Configuration management — extracted as a workspace crate in Phase 3
//! (issue #72).
//!
//! Owns:
//! - `settings.json` loader + effective-settings merge layer
//! - `AGENTS.md` / `CLAUDE.md` discovery + injection
//! - Data-root path helpers (`~/.allthecodes/` or `$ALLTHECODES_HOME`)
//! - Feature-gate system (`FEATURE_*` env vars)
//! - Config validation warnings
//! - `runtime_settings::SettingsJson` — the runtime projection of effective
//!   settings previously in `types::app_state::SettingsJson` (moved here to
//!   let `config::validation` read it without a reverse dep back into the
//!   root crate).

#![recursion_limit = "512"]
#![allow(deprecated)] // claude_md legacy functions retained for migration

pub mod change_detector;

/// Primary module name for project instruction file discovery.
///
/// Provides `find_agents_md_files`, `build_agents_md_context`, etc.
/// The underlying implementation lives in `claude_md` (legacy name) but all
/// new code should use `agents_md` or the functions in `claude_md` that
/// start with `agents_md_` / `build_agents_md_` / `load_agents_md_`.
pub use crate::claude_md as agents_md;

pub mod claude_md;
pub mod constants;
pub mod features;
pub mod internal_writes;
pub mod mdm;
pub mod paths;
pub mod permission_validation;
pub mod proactive_sleep;
pub mod proactive_state;
pub mod runtime_settings;
pub mod settings;
pub mod user_agent;
pub mod validation;
pub mod validation_tips;
