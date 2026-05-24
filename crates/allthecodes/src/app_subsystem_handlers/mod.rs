//! Subsystem command handlers and status snapshot builders.
//!
//! **Command handlers** respond to `FrontendMessage` commands for each subsystem
//! (LSP, MCP, Plugin, Skill).  Each handler returns a `Vec<BackendMessage>`
//! that the caller sends via the [`FrontendSink`].  Handlers never write to
//! stdout directly.
//!
//! **Status snapshot builders** assemble point-in-time status objects from
//! each subsystem's in-memory state.  These are used by `QueryStatus` commands
//! and the `SystemStatus` tool.

mod ide;
mod lsp;
mod mcp;
mod mcp_config;
mod plugin;
mod skill;
mod snapshot;

pub use ide::*;
pub use lsp::*;
pub use mcp::*;
pub use plugin::*;
pub use skill::*;
pub use snapshot::*;
