//! Shared utilities extracted from the root crate in Phase 3 (issue #72).
//!
//! These modules are used throughout the codebase but have narrow deps:
//! only `cc-types` (message types) and `cc-config` (bash constants).

pub mod abort;
pub mod bash;
pub mod cwd;
// REMOVED: file_state_cache — duplicate of cc-tools::tool::FileStateCache.
// All callers use cc-tools::tool::FileStateCache (HashMap+RwLock based).
// The LRU-based version here had zero external callers.
pub mod git;
pub mod git_operation_tracking;
pub mod hash;
pub mod messages;
pub mod shell;
pub mod tokens;
