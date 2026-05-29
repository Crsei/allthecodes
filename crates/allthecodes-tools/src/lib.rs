//! cc-tools: shared tool specs and pure registry helpers.
//!
//! This crate owns tool contracts and tool-facing implementations that can run
//! without depending on the engine, query loop, UI, or daemon crate. Domain
//! tools with heavier ownership boundaries remain in their target crates until
//! their dependencies can move cleanly.

#[cfg(feature = "full")]
pub mod ask_user;
#[cfg(feature = "full")]
pub mod brief;
#[cfg(feature = "full")]
pub mod config_tool;
#[cfg(feature = "full")]
pub mod deferred_tools;
#[cfg(feature = "full")]
pub mod exec;
#[cfg(feature = "full")]
pub mod fs;
#[cfg(feature = "full")]
pub mod hooks;
#[cfg(feature = "full")]
pub mod observable_input;
#[cfg(feature = "full")]
pub mod plan_mode;
#[cfg(feature = "full")]
pub mod plan_workflow;
#[cfg(feature = "full")]
pub mod product_tools;
#[cfg(feature = "full")]
pub mod registry;
#[cfg(feature = "full")]
pub mod result;
#[cfg(feature = "full")]
pub mod send_user_message;
#[cfg(feature = "full")]
pub mod sleep;
#[cfg(feature = "full")]
pub mod structured_output;
#[cfg(feature = "full")]
pub mod system_status;
#[cfg(feature = "full")]
pub mod task_specs;
#[cfg(feature = "full")]
pub mod tasks;
pub mod tool;
#[cfg(feature = "full")]
pub mod tool_search;
#[cfg(feature = "full")]
pub mod web_fetch;
#[cfg(feature = "full")]
pub mod web_search;
