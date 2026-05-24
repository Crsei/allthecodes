//! Root-owned worktree tool runtime.
//!
//! This remains in the root crate until worktree hook policy and status-line
//! session state have a shared owner.

pub mod tool;

pub use tool::get_current_worktree_session;
