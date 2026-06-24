//! Axum route handlers for the web chat API.
//!
//! This module is organized by feature group. Each submodule exposes its
//! handler functions and request/response types; `mod.rs` re-exports all
//! public items so that `handlers::chat_handler` and similar paths used in
//! the router builder continue to resolve.

pub mod activity_recorder;
pub mod admin;
pub mod agents;
pub mod appshots;
pub mod auth;
pub mod backend_services;
pub mod capabilities;
pub mod channels;
pub mod chat;
pub mod chat_modes;
pub mod chrome_relay;
pub mod commands;
pub mod computer_use;
pub mod credentials;
pub mod files;
pub mod gateways;
pub mod git;
pub mod group_chat;
pub mod handler_shared;
pub mod health;
pub mod hooks;
pub mod image_generate;
pub mod jobs;
pub mod kanban;
pub mod launchpad;
pub mod logs;
pub mod mcp_servers;
pub mod memory;
pub mod mentions;
pub mod messaging;
pub mod models;
pub mod people;
pub mod plugins;
pub mod profiles;
pub mod prompts;
pub mod providers;
pub mod proxy;
pub mod queue;
pub mod sessions;
pub mod settings_phase1;
pub mod sidebar;
pub mod skills;
pub mod tasks;
pub mod usage;
pub mod voice;
pub mod workspaces;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export all public items from each submodule so the router builder
// and external callers can still use `handlers::*` paths.
pub use crate::api_errors::{api_error_body, ApiError};
pub use activity_recorder::*;
pub use admin::*;
pub use agents::*;
pub use appshots::*;
pub use auth::*;
pub use backend_services::*;
pub use capabilities::*;
pub use channels::*;
pub use chat::*;
pub use chat_modes::*;
pub use chrome_relay::*;
pub(crate) use commands::get_all_commands;
pub use commands::set_command_provider;
pub use computer_use::*;
pub use credentials::*;
pub use files::*;
pub use gateways::*;
pub use git::*;
pub use group_chat::*;
pub(crate) use handler_shared::setting_bool;
pub use health::*;
pub use hooks::*;
pub use jobs::*;
pub use kanban::*;
pub use launchpad::*;
pub use logs::*;
pub use mcp_servers::*;
pub use memory::*;
pub use models::*;
pub use people::*;
pub use plugins::*;
pub use profiles::*;
pub use prompts::*;
pub use providers::*;
pub use proxy::*;
pub use sessions::*;
pub use settings_phase1::*;
pub use skills::*;
pub use usage::*;
pub use workspaces::*;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------
