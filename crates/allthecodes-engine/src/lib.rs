//! cc-engine — QueryEngine, Agent tool, query-loop driver, and engine-level
//! shared state (Phase 6 — in progress).
//!
//! Issue #75 (`[workspace-split] Phase 6`): this crate is the target
//! destination for `crates/allthecodes/src/engine/` plus the engine-level
//! shared structures (`AppState`, `Tool` trait, `ToolUseContext`) and the
//! status-line runner.
//!
//! See `docs/superpowers/specs/2026-04-20-workspace-split-design.md`.

extern crate self as allthecodes_engine;

pub mod agent;
pub mod agent_runtime;
pub mod browser_tool;
pub mod codex_exec;
pub mod command_runtime;
pub mod effort;
pub mod hooks;
pub mod input_processing;
pub mod ipc_compat;
pub mod lifecycle;
pub mod mcp_resource_tools;
pub mod mcp_tool_adapter;
pub mod output_style;
pub mod prompt_sections;
pub mod query;
pub mod result;
pub mod runtime_services;
pub mod security;
pub mod services;
pub mod skill_tool;
pub mod status_line;
pub mod system_prompt;
pub mod tool_runtime;
pub mod tools;
pub mod types;
pub mod verification;
pub mod worktree_hooks;

#[cfg(feature = "telemetry")]
pub mod telemetry_bridge;

pub use allthecodes_bootstrap as bootstrap;
pub use allthecodes_compact as compact;
pub use allthecodes_config as config;
pub use allthecodes_observability as observability;
pub use allthecodes_permissions as permissions;
pub use allthecodes_sandbox as sandbox;
pub use allthecodes_session as session;
pub use allthecodes_skills as skills;
pub use allthecodes_utils as utils;

// Re-export from cc-types so consumers can eventually write
// `use allthecodes_engine::{HookRunner, CommandDispatcher}` once the engine types
// land here too.
pub use allthecodes_types::hooks::{HookRunner, NoopHookRunner};
