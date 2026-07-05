//! cc-tools: shared tool specs and pure registry helpers.
//!
//! This crate owns tool contracts and tool-facing implementations that can run
//! without depending on the engine, query loop, UI, or daemon crate. Domain
//! tools with heavier ownership boundaries remain in their target crates until
//! their dependencies can move cleanly.

#[cfg(feature = "full")]
mod common;
#[cfg(feature = "full")]
pub mod deferred_tools;
#[cfg(feature = "full")]
pub mod discovery_search;
#[cfg(feature = "full")]
pub mod exec;
#[cfg(feature = "full")]
pub mod fs;
#[cfg(feature = "full")]
pub mod goals;
#[cfg(feature = "full")]
pub mod hooks;
#[cfg(feature = "full")]
pub mod interaction;
#[cfg(feature = "full")]
pub mod media;
#[cfg(feature = "full")]
pub mod memory;
pub mod metadata;
#[cfg(feature = "full")]
pub mod network;
#[cfg(feature = "full")]
pub mod notifications;
#[cfg(feature = "full")]
pub mod plan_mode;
#[cfg(feature = "full")]
pub mod product;
#[cfg(feature = "full")]
pub mod registry;
#[cfg(feature = "full")]
pub mod result;
#[cfg(feature = "full")]
pub mod runtime;
pub mod runtime_capability;
#[cfg(feature = "full")]
pub mod session_search;
#[cfg(feature = "full")]
pub mod skills;
#[cfg(feature = "full")]
pub mod tasks;
pub mod tool;
#[cfg(feature = "full")]
pub mod workflow;
#[cfg(feature = "full")]
pub mod workflow_dynamic;

#[cfg(all(test, feature = "full"))]
mod semantic_tool_tests;
