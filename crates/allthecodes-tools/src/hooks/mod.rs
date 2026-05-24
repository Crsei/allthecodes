//! Tool execution hooks — pre-tool, post-tool, stop, permission hooks,
//! SSRF guard, HTTP hooks, async hook registry, hook events, and helpers.
//!
//! Corresponds to: LIFECYCLE_STATE_MACHINE.md §6 (Phase E)
//!
//! Hooks are user-defined shell commands configured in settings.json under
//! the `hooks` key. Each hook event (PreToolUse, PostToolUse, Stop) contains
//! a list of HookEventConfig entries, each with an optional matcher and a
//! list of HookEntry commands to execute as subprocesses.
//!
//! The plain data types and the `HookRunner` trait live in `cc-types::hooks`.
//! This module provides the concrete shell-command runner (`ShellHookRunner`)
//! together with free functions that the rest of the crate uses directly.

mod async_registry;
mod execution;
mod hook_events;
mod hook_helpers;
mod http_hook;
mod post_tool;
mod pre_tool;
mod ssrf_guard;

pub use async_registry::{
    check_for_async_hook_responses, clear_all_async_hooks, complete_async_hook,
    finalize_pending_async_hooks, get_pending_async_hooks, register_pending_async_hook,
    remove_delivered_async_hooks,
};
pub use hook_events::{
    clear_hook_event_state, emit_hook_response, emit_hook_started, register_hook_event_handler,
    set_all_hook_events_enabled,
};
pub use hook_helpers::{add_arguments_to_prompt, get_hook_display_text, HookHelpers};
pub use http_hook::{exec_http_hook, HttpHookResult};
pub use post_tool::{
    fire_notification_hook, run_event_hooks, run_post_tool_failure_hooks, run_stop_hooks,
};
pub use pre_tool::run_pre_tool_hooks;
pub use ssrf_guard::{is_blocked_address, SsrfGuard};

// Re-export the plain data types from cc-types so existing
// `crate::hooks::{HookEventConfig, HookOutput, ...}` import paths keep
// working without changes.
pub use allthecodes_types::hooks::{
    HookEntry, HookEvent, HookEventConfig, HookEventMetadata, HookOutput, HookRunner, HookSource,
    HooksMap, IndividualHookConfig, MatcherMetadata, PermissionOverride, PostToolHookResult,
    PreToolHookResult,
};

use async_trait::async_trait;
use serde_json::Value;

// ---------------------------------------------------------------------------
// ShellHookRunner — concrete HookRunner backed by the shell-command impl
// ---------------------------------------------------------------------------

/// Shell-command-backed `HookRunner`.
///
/// Delegates every trait method to the free functions in this module so the
/// implementations (`execute_command_hook`, subprocess spawning, etc.) remain
/// in one place.
pub struct ShellHookRunner;

impl ShellHookRunner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShellHookRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HookRunner for ShellHookRunner {
    fn load_hook_configs(&self, hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig> {
        allthecodes_types::hooks::load_hook_configs(hooks_value, event_name)
    }

    async fn run_pre_tool_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult> {
        run_pre_tool_hooks(tool_name, input, hook_configs).await
    }

    async fn run_post_tool_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        tool_result_data: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        post_tool::run_post_tool_hooks_data(tool_name, input, tool_result_data, hook_configs).await
    }

    async fn run_post_tool_failure_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        error: &str,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()> {
        run_post_tool_failure_hooks(tool_name, input, error, hook_configs).await
    }

    async fn run_event_hooks(
        &self,
        event_name: &str,
        payload: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput> {
        run_event_hooks(event_name, payload, hook_configs).await
    }

    async fn run_stop_hooks(
        &self,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        run_stop_hooks(hook_configs).await
    }
}
