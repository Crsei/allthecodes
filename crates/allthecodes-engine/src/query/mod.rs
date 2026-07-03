//! Canonical production query loop.
//!
//! `crates/allthecodes-engine/src/query/` is the only production query loop
//! today. The previous independent `allthecodes-query` crate was removed
//! because it duplicated behavior; do not reintroduce it as a parallel
//! implementation. Any future extraction must leave one production
//! implementation and depend on stable typed boundaries rather than direct
//! engine-internal state.

pub mod deps;
pub mod goal_runtime;
pub mod loop_helpers;
pub mod loop_impl;
pub mod recovery;
pub mod stop_hooks;
pub mod token_budget;
pub mod turn_context;
pub mod turn_state;
