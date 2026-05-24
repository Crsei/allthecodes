//! Chrome / browser MCP bridge — extracted in Phase 4 (issue #73).
//!
//! **Partial extraction** — 9 of the 11 submodules moved cleanly; two keep
//! a hard dep on the root crate's `Tool` trait and stay behind until the
//! hub-cycle break in Phase 5:
//!
//! - `detection` — uses `Arc<dyn Tool>` to categorize the live tool list.
//! - `prompt` — inspects registered tools to decide whether the prompt
//!   preamble should include browser-automation instructions.

pub mod common;
pub mod detection;
pub mod mcp_bridge;
pub mod native_host;
pub mod permissions;
pub mod session;
pub mod setup;
pub mod state;
pub mod tool_rendering;
pub mod transport;
