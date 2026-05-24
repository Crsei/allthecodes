//! Engine-level hook system — config management, session hooks, registration,
//! post-sampling hooks, file watcher, agent/prompt hook execution, and more.
//!
//! Port of TypeScript `src/utils/hooks/` modules.

pub mod agent_hook;
pub mod api_query_helper;
pub mod config_manager;
pub mod file_watcher;
pub mod post_sampling;
pub mod prompt_hook;
pub mod registration;
pub mod session_hooks;
pub mod skill_improvement;
