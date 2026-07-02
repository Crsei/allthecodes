//! Forked-agent execution path (issue #37 infrastructure).
//!
//! This module provides a lightweight fork primitive used by:
//! - `/btw`   — a tool-free, single-turn side-question agent.
//! - `/simplify` — multi-agent review when the full AgentTool is unavailable.
//! - `SkillContext::Fork` — runs a skill's prompt in a bounded sub-engine.
//!
//! Unlike [`super::AgentTool`] (which registers an agent in the global tree,
//! fires SubagentStart / SubagentStop hooks, and surfaces streaming events
//! through IPC), a "fork" is a self-contained child [`QueryEngine`]
//! invocation: no persistence, no session saving, no IPC tree registration.
//! The result is a single `String` delivered back to the caller, plus an
//! `had_error` flag.
//!
//! Prompt-cache safety: the caller may pass `parent_messages` so the child
//! engine reuses the same initial history as the parent, giving cache hits
//! for the leading messages. Without `parent_messages`, the child starts
//! fresh.

use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info};
use uuid::Uuid;

use crate::lifecycle::QueryEngine;
use crate::types::config::{QueryEngineConfig, QuerySource};
use crate::types::message::Message;
use crate::types::tool::{QueryChainTracking, Tools};

use super::collect_stream_result;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parameters for [`run_fork`].
#[derive(Clone)]
pub struct ForkParams {
    /// Optional stable agent id. When absent, a UUID is generated.
    pub agent_id: Option<String>,
    /// The prompt to submit to the forked agent.
    pub prompt: String,
    /// Working directory for the child engine.
    pub cwd: String,
    /// Explicit model for the fork. Falls back to `fallback_model` if the
    /// request fails.
    pub model: String,
    /// Fallback model for retries (typically the parent's main model).
    pub fallback_model: Option<String>,
    /// Tool set available to the forked agent. Pass an empty `Vec` for a
    /// tool-free side question (e.g. `/btw`).
    pub tools: Tools,
    /// Hard cap on turns. Defaults to 1 (no tool use) when `None`.
    pub max_turns: Option<usize>,
    /// Optional messages to seed the child engine with, enabling prompt-cache
    /// reuse against the parent conversation. Passing `None` means the fork
    /// starts with an empty history.
    pub parent_messages: Option<Vec<Message>>,
    /// Optional additional system-prompt fragment appended to the engine's
    /// built-in system prompt. Useful for giving the fork a narrower role.
    pub append_system_prompt: Option<String>,
    /// Custom system prompt that overrides the engine's default.
    pub custom_system_prompt: Option<String>,
    /// Hook runner to propagate into the child engine.
    pub hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    /// Command dispatcher to propagate.
    pub command_dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
}

/// Result of a forked-agent execution.
#[derive(Debug, Clone)]
pub struct ForkOutcome {
    /// Text output collected from the child engine.
    pub text: String,
    /// `true` if the child engine finished with an error result.
    pub had_error: bool,
    /// Wall-clock duration of the fork in milliseconds.
    pub duration_ms: u64,
    /// Identifier assigned to the fork agent (UUID v4).
    pub agent_id: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a forked agent: spawns an ephemeral child [`QueryEngine`], submits
/// `params.prompt`, and returns the collected text output.
///
/// The child is ephemeral — session persistence is disabled, nothing is
/// written to disk, and the conversation history of the parent is not
/// mutated. Pass `parent_messages` to reuse the parent's history for
/// cache-safe prompting.
pub async fn run_fork(params: ForkParams) -> Result<ForkOutcome> {
    let started = std::time::Instant::now();
    let agent_id = params
        .agent_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let chain_id = Uuid::new_v4().to_string();

    info!(
        agent_id = %agent_id,
        model = %params.model,
        tool_count = params.tools.len(),
        max_turns = params.max_turns.unwrap_or(1),
        "run_fork: spawning child engine"
    );

    let child_config = QueryEngineConfig {
        cwd: params.cwd,
        tools: params.tools,
        custom_system_prompt: params.custom_system_prompt,
        append_system_prompt: params.append_system_prompt,
        user_specified_model: Some(params.model.clone()),
        fallback_model: params.fallback_model,
        max_turns: Some(params.max_turns.unwrap_or(1)),
        max_budget_usd: None,
        task_budget: None,
        verbose: false,
        initial_messages: params.parent_messages,
        commands: vec![],
        thinking_config: None,
        json_schema: None,
        replay_user_messages: false,
        persist_session: false,
        resolved_model: Some(params.model.clone()),
        auto_save_session: false,
        agent_context: Some(crate::types::config::AgentContext {
            agent_id: agent_id.clone(),
            query_tracking: QueryChainTracking { chain_id, depth: 1 },
            langfuse_session_id: String::new(),
            agent_type: Some("fork".to_string()),
            team_context: None,
            tool_permission_context: None,
        }),
    };

    let mut child_engine = QueryEngine::new(child_config);
    child_engine.set_hook_runner(params.hook_runner);
    child_engine.set_command_dispatcher(params.command_dispatcher);

    let stream = child_engine.submit_message(&params.prompt, QuerySource::Agent(agent_id.clone()));
    let (text, had_error) = collect_stream_result(stream, None).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    debug!(
        agent_id = %agent_id,
        text_len = text.len(),
        had_error,
        duration_ms,
        "run_fork: completed"
    );

    Ok(ForkOutcome {
        text,
        had_error,
        duration_ms,
        agent_id,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> ForkParams {
        ForkParams {
            agent_id: None,
            prompt: "hello".to_string(),
            cwd: ".".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            fallback_model: None,
            tools: vec![],
            max_turns: None,
            parent_messages: None,
            append_system_prompt: None,
            custom_system_prompt: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
        }
    }

    #[test]
    fn fork_params_defaults_single_turn() {
        let p = default_params();
        assert_eq!(p.max_turns.unwrap_or(1), 1);
        assert!(p.tools.is_empty());
    }

    #[test]
    fn fork_params_max_turns_honored() {
        let mut p = default_params();
        p.max_turns = Some(5);
        assert_eq!(p.max_turns.unwrap_or(1), 5);
    }

    #[test]
    fn fork_params_parent_messages_seed_history() {
        let mut p = default_params();
        p.parent_messages = Some(vec![]);
        assert!(p.parent_messages.is_some());
    }

    #[test]
    fn fork_outcome_has_error_flag() {
        let outcome = ForkOutcome {
            text: "answer".into(),
            had_error: false,
            duration_ms: 42,
            agent_id: "abc".into(),
        };
        assert!(!outcome.had_error);
        assert_eq!(outcome.duration_ms, 42);
        assert_eq!(outcome.agent_id, "abc");
    }
}
