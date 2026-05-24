//! cc-query - async streaming query loop.
//!
//! This crate owns the query-loop implementation. Root-crate callers import
//! directly from `allthecodes_query`.

pub mod deps;
pub(crate) mod loop_helpers;
pub mod loop_impl;
pub mod stop_hooks;
pub mod token_budget;
pub(crate) mod turn_context;

pub use allthecodes_engine::agent_runtime::{CompletedBackgroundAgent, PendingBackgroundResults};
pub use deps::QueryDeps;
pub use loop_impl::query;
