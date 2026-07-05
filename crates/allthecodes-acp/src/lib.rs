//! # allthecodes-acp
//!
//! Agent Client Protocol v2 server for allthecodes. Implements JSON-RPC 2.0
//! over newline-delimited stdio, session management via `QueryEngine`,
//! streaming `session/update` notifications, permission bridging,
//! configuration options, slash-command advertising, authentication,
//! MCP server integration, and session deletion.
//!
//! ## Entry Point
//!
//! The public entry point is [`run_stdio`], called from the root binary when
//! `--acp` is passed. It owns the main I/O loop: read JSON-RPC frames from
//! stdin, dispatch to method handlers, write responses and notifications to
//! stdout.

pub mod auth;
pub mod capabilities;
pub mod commands;
pub mod config_options;
pub mod content;
pub mod engine_factory;
pub mod errors;
pub mod jsonrpc;
pub mod mcp;
pub mod permissions;
pub mod runtime;
pub mod session;
pub mod tool_calls;
pub mod transport;
pub mod updates;

/// Re-export types needed by permissions, tool_calls, and other modules.
pub use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionRequestPayload, PermissionResponsePayload,
};
pub use allthecodes_types::sdk::{
    SdkAssistantMessage, SdkMessage, SdkResult, SdkStreamEvent, SdkUserReplay,
};

use std::sync::Arc;

/// Configuration for the ACP stdio runtime.
#[derive(Clone)]
pub struct AcpRuntimeConfig {
    /// Resolved model identifier to use for sessions.
    pub model: String,
    /// Current working directory.
    pub cwd: std::path::PathBuf,
    /// Tool list (Vec<Arc<dyn Tool>>) discovered at startup.
    pub tools: allthecodes_engine::types::tool::Tools,
    /// Template app state for per-session engines.
    pub app_state_template: allthecodes_engine::types::app_state::AppState,
    /// Merged effective settings.
    pub merged_config: allthecodes_config::settings::EffectiveSettings,
    /// CLI overrides forwarded from the root binary.
    pub cli_overrides: AcpCliOverrides,
    /// Factory for creating per-session engines.
    pub engine_factory: Arc<dyn AcpEngineFactory>,
}

/// CLI-level overrides extracted from the root binary's `Cli` struct.
#[derive(Debug, Clone, Default)]
pub struct AcpCliOverrides {
    pub max_turns: Option<usize>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub permission_mode: Option<String>,
    pub verbose: bool,
    pub no_network: bool,
}

impl AcpCliOverrides {
    /// Extract overrides from the root CLI struct (available only when built
    /// as part of the `allthecodes` binary).
    #[cfg(feature = "root_crate")]
    pub fn from_cli(cli: &crate::cli::Cli) -> Self {
        Self {
            max_turns: cli.max_turns,
            model: cli.model.clone(),
            system_prompt: cli.system_prompt.clone(),
            append_system_prompt: cli.append_system_prompt.clone(),
            permission_mode: cli.permission_mode.clone(),
            verbose: cli.verbose,
            no_network: cli.no_network,
        }
    }
}

/// Parameters for creating a single ACP session engine.
#[derive(Debug, Clone)]
pub struct AcpEngineParams {
    /// Optional session id to set on the engine (for session/load and session/resume).
    pub session_id: Option<String>,
    /// Working directory for the session.
    pub cwd: std::path::PathBuf,
    /// Additional directories to grant tool access to.
    pub additional_directories: Vec<std::path::PathBuf>,
    /// Messages to pre-load into the engine (for session/load/replay).
    pub initial_messages: Option<Vec<allthecodes_types::message::Message>>,
}

/// Trait for creating per-session engines. Implemented by the root binary
/// bridge (`acp_runtime_bridge.rs`) to inject root-owned dependencies.
pub trait AcpEngineFactory: Send + Sync {
    /// Create a new QueryEngine configured for one ACP session.
    fn create_engine(
        &self,
        params: AcpEngineParams,
    ) -> anyhow::Result<Arc<allthecodes_engine::lifecycle::QueryEngine>>;
}

/// Helper to disable `from_cli` when not built inside the root `allthecodes`
/// crate. The root crate provides its own definition that calls `cli::Cli`.
#[cfg(not(feature = "root_crate"))]
/// Run the ACP stdio runtime: reads JSON-RPC 2.0 messages from stdin and
/// writes responses/notifications to stdout.
///
/// # Errors
///
/// Returns an error only on unrecoverable setup failures (I/O, initialization).
/// Runtime JSON-RPC errors are sent as error responses, not propagated.
pub async fn run_stdio(config: AcpRuntimeConfig) -> anyhow::Result<()> {
    // Delegate to the runtime module.
    let ctx = runtime::RuntimeContext {
        model: config.model,
        cwd: config.cwd,
        tools: config.tools,
        app_state_template: config.app_state_template,
        merged_config: config.merged_config,
        cli_overrides: config.cli_overrides,
        engine_factory: config.engine_factory,
    };
    runtime::run_runtime(ctx).await
}
