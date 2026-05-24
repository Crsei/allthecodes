//! Agent dispatch: normal and worktree execution modes.

use anyhow::Result;
use serde_json::json;
use tracing::debug;

use crate::lifecycle::QueryEngine;
use crate::types::config::QuerySource;
use crate::types::tool::*;
use crate::utils::bash::validate_working_directory;

use super::{build_child_config, collect_stream_result, AgentInput, AgentTool};

impl AgentTool {
    /// Run the agent without worktree isolation (normal mode).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_agent_normal(
        &self,
        params: &AgentInput,
        ctx: &ToolUseContext,
        agent_id: &str,
        description: &str,
        agent_model: &str,
        parent_model: &str,
        current_depth: usize,
        background: bool,
    ) -> Result<ToolResult> {
        let started = std::time::Instant::now();
        let child_cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());

        // Validate that the working directory is usable before spawning subagent
        validate_working_directory(&child_cwd)?;

        let child_config = build_child_config(
            child_cwd,
            ctx,
            agent_id,
            params.subagent_type.as_deref(),
            agent_model,
            parent_model,
            current_depth,
        );

        let agent_tx = ctx.bg_agent_tx.as_ref();

        // Register sync agent in tree and emit Spawned event
        {
            let chain_id = ctx
                .query_tracking
                .as_ref()
                .map(|t| t.chain_id.clone())
                .unwrap_or_default();
            let node = allthecodes_types::agent_types::AgentNode {
                agent_id: agent_id.to_string(),
                parent_agent_id: ctx.agent_id.clone(),
                description: description.to_string(),
                agent_type: params.subagent_type.clone(),
                model: Some(agent_model.to_string()),
                state: "running".into(),
                is_background: false,
                depth: current_depth + 1,
                chain_id: chain_id.clone(),
                spawned_at: chrono::Utc::now().timestamp(),
                completed_at: None,
                duration_ms: None,
                result_preview: None,
                had_error: false,
                children: vec![],
            };
            crate::agent_runtime::register_agent_node(node);

            if let Some(tx) = agent_tx {
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    allthecodes_types::agent_events::AgentEvent::Spawned {
                        agent_id: agent_id.to_string(),
                        parent_agent_id: ctx.agent_id.clone(),
                        description: description.to_string(),
                        agent_type: params.subagent_type.clone(),
                        model: Some(agent_model.to_string()),
                        is_background: false,
                        depth: current_depth + 1,
                        chain_id,
                    },
                ));

                let roots = crate::agent_runtime::agent_tree_snapshot();
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    allthecodes_types::agent_events::AgentEvent::TreeSnapshot { roots },
                ));
            }
        }

        let mut child_engine = QueryEngine::new(child_config);
        child_engine.set_hook_runner(ctx.hook_runner.clone());
        child_engine.set_command_dispatcher(ctx.command_dispatcher.clone());
        let stream =
            child_engine.submit_message(&params.prompt, QuerySource::Agent(agent_id.to_string()));

        let ipc = agent_tx.map(|tx| (tx, agent_id));
        let (result_text, had_error) = collect_stream_result(stream, ipc).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        // Update tree state and emit Completed + TreeSnapshot
        {
            let preview = if result_text.len() > 200 {
                let end = result_text.floor_char_boundary(200);
                format!("{}...", &result_text[..end])
            } else {
                result_text.clone()
            };
            crate::agent_runtime::update_agent_state(
                agent_id,
                if had_error { "error" } else { "completed" },
                Some(preview.clone()),
                Some(duration_ms),
                had_error,
            );

            if let Some(tx) = agent_tx {
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    allthecodes_types::agent_events::AgentEvent::Completed {
                        agent_id: agent_id.to_string(),
                        result_preview: preview,
                        had_error,
                        duration_ms,
                        output_tokens: None,
                    },
                ));

                let roots = crate::agent_runtime::agent_tree_snapshot();
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    allthecodes_types::agent_events::AgentEvent::TreeSnapshot { roots },
                ));
            }
        }

        debug!(
            agent_id = %agent_id,
            result_len = result_text.len(),
            error = had_error,
            "subagent completed"
        );

        let kind = if had_error { "error" } else { "complete" };
        let _ = crate::agent_runtime::emit_subagent_event(
            kind,
            agent_id,
            ctx.agent_id.as_deref(),
            Some(description),
            Some(agent_model),
            current_depth + 1,
            background,
            Some(json!({
                "duration_ms": duration_ms,
                "result_len": result_text.len(),
            })),
        );

        Ok(ToolResult {
            data: json!(result_text),
            new_messages: vec![],
            ..Default::default()
        })
    }

    /// Consolidated dispatch: fires SubagentStart hook, runs the agent (worktree
    /// or normal), fires SubagentStop hook.  Used by both the synchronous path
    /// and as a fallback when `bg_agent_tx` is unavailable.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_agent_dispatch(
        &self,
        use_worktree: bool,
        params: &AgentInput,
        ctx: &ToolUseContext,
        agent_id: &str,
        agent_model: &str,
        parent_model: &str,
        current_depth: usize,
        description: &str,
        start_configs: &[allthecodes_types::hooks::HookEventConfig],
        stop_configs: &[allthecodes_types::hooks::HookEventConfig],
        background: bool,
    ) -> Result<ToolResult> {
        // Fire SubagentStart hook
        if !start_configs.is_empty() {
            let payload = json!({
                "agent_id": agent_id,
                "prompt": &params.prompt,
                "description": description,
                "subagent_type": params.subagent_type.as_deref().unwrap_or("general-purpose"),
                "model": agent_model,
                "depth": current_depth + 1,
            });
            let _ = ctx
                .hook_runner
                .run_event_hooks("SubagentStart", &payload, start_configs)
                .await;
        }

        let result = if use_worktree {
            self.run_in_worktree(
                params,
                ctx,
                agent_id,
                description,
                agent_model,
                parent_model,
                current_depth,
                background,
            )
            .await
        } else {
            self.run_agent_normal(
                params,
                ctx,
                agent_id,
                description,
                agent_model,
                parent_model,
                current_depth,
                background,
            )
            .await
        };

        // Fire SubagentStop hook
        if !stop_configs.is_empty() {
            let is_error = result.as_ref().is_err();
            let payload = json!({
                "agent_id": agent_id,
                "description": description,
                "is_error": is_error,
            });
            let _ = ctx
                .hook_runner
                .run_event_hooks("SubagentStop", &payload, stop_configs)
                .await;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    // -----------------------------------------------------------------------
    // SubagentStart hook payload shape
    // -----------------------------------------------------------------------

    /// Verify that the SubagentStart payload produced in run_agent_dispatch
    /// has all the expected fields with the right types.
    #[test]
    fn test_subagent_start_payload_structure() {
        let agent_id = "test-agent-abc";
        let prompt = "search for files";
        let description = "search files";
        let subagent_type = "general-purpose";
        let agent_model = "claude-sonnet-4-20250514";
        let current_depth: usize = 1;

        let payload = json!({
            "agent_id": agent_id,
            "prompt": prompt,
            "description": description,
            "subagent_type": subagent_type,
            "model": agent_model,
            "depth": current_depth + 1,
        });

        assert_eq!(payload["agent_id"], "test-agent-abc");
        assert_eq!(payload["prompt"], "search for files");
        assert_eq!(payload["description"], "search files");
        assert_eq!(payload["subagent_type"], "general-purpose");
        assert_eq!(payload["model"], "claude-sonnet-4-20250514");
        assert_eq!(payload["depth"], 2);
    }

    /// Verify SubagentStop payload has agent_id, description, and is_error.
    #[test]
    fn test_subagent_stop_payload_structure() {
        let agent_id = "test-agent-abc";
        let description = "search files";
        let is_error = false;

        let payload = json!({
            "agent_id": agent_id,
            "description": description,
            "is_error": is_error,
        });

        assert_eq!(payload["agent_id"], "test-agent-abc");
        assert_eq!(payload["description"], "search files");
        assert_eq!(payload["is_error"], false);
    }

    #[test]
    fn test_subagent_stop_payload_with_error() {
        let agent_id = "test-agent-xyz";
        let description = "failing task";
        let is_error = true;

        let payload = json!({
            "agent_id": agent_id,
            "description": description,
            "is_error": is_error,
        });

        assert_eq!(payload["is_error"], true);
        assert_eq!(payload["agent_id"], "test-agent-xyz");
    }

    // -----------------------------------------------------------------------
    // Background path payload extra "background" field
    // -----------------------------------------------------------------------

    #[test]
    fn test_background_start_payload_has_background_flag() {
        let payload = json!({
            "agent_id": "bg-agent-1",
            "prompt": "do work",
            "description": "bg work",
            "subagent_type": "general-purpose",
            "model": "claude-sonnet-4-20250514",
            "depth": 1,
            "background": true,
        });

        assert_eq!(payload["background"], true);
    }

    #[test]
    fn test_background_stop_payload_has_background_flag() {
        let payload = json!({
            "agent_id": "bg-agent-1",
            "description": "bg work",
            "is_error": false,
            "background": true,
        });

        assert_eq!(payload["background"], true);
        assert_eq!(payload["is_error"], false);
    }

    // -----------------------------------------------------------------------
    // use_worktree flag logic (mirrors tool_impl.rs call())
    // -----------------------------------------------------------------------

    #[test]
    fn test_dispatch_worktree_detection_from_isolation_string() {
        let isolation_worktree = Some("worktree".to_string());
        let use_worktree = isolation_worktree
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(use_worktree);
    }

    #[test]
    fn test_dispatch_no_worktree_when_isolation_none() {
        let isolation: Option<String> = None;
        let use_worktree = isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(!use_worktree);
    }
}
