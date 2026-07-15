//! Agent dispatch: normal and worktree execution modes.

use allthecodes_config::features::{self, Feature};
use allthecodes_types::agent_events::AgentCompletionStatus;
use anyhow::Result;
use serde_json::json;
use tracing::debug;

use crate::lifecycle::QueryEngine;
use crate::types::config::{QueryEngineConfig, QuerySource};
use crate::types::message::ToolResultContent;
use crate::types::tool::*;
use crate::utils::bash::validate_working_directory;

use super::{build_child_config, collect_stream_result, AgentInput, AgentRunUsage, AgentTool};

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

        let mut child_config = build_child_config(
            child_cwd,
            ctx,
            agent_id,
            params.subagent_type.as_deref(),
            agent_model,
            parent_model,
            current_depth,
        );
        child_config.verification_policy = params.verification_policy.clone();
        apply_coordinator_worker_turn_limit(&mut child_config, ctx, params);
        params.apply_fork_to_child_config(&mut child_config);

        let agent_tx = ctx.bg_agent_tx.as_ref();
        let owner = crate::agent_runtime::inherited_agent_owner_scope(
            ctx.agent_id.as_deref(),
            &ctx.langfuse_session_id,
            std::path::Path::new(&ctx.cwd),
        );

        // Register sync agent in tree and emit Spawned event
        {
            let fork_metadata = params.fork_metadata();
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
                fork_metadata: fork_metadata.clone(),
                children: vec![],
            };
            crate::agent_runtime::register_agent_node_for_owner(node, owner.clone());

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
                        fork_metadata,
                    },
                ));

                let roots = crate::agent_runtime::agent_tree_snapshot_for_owner(
                    &owner.parent_session_id,
                    &owner.canonical_workspace,
                );
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
        let (result_text, had_error, usage) = collect_stream_result(stream, ipc).await;
        params.close_live_channel(agent_id, if had_error { "failed" } else { "completed" });
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
                        completion_status: AgentCompletionStatus::from_had_error(had_error),
                        duration_ms,
                        total_tokens: usage.total_tokens,
                        output_tokens: usage.output_tokens,
                        tool_uses: usage.tool_uses,
                        agent_type: params.subagent_type.clone(),
                    },
                ));

                let roots = crate::agent_runtime::agent_tree_snapshot_for_owner(
                    &owner.parent_session_id,
                    &owner.canonical_workspace,
                );
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

        let model_content =
            if is_coordinator_parent(ctx) && is_worker_subagent(params.subagent_type.as_deref()) {
                Some(ToolResultContent::Text(
                    coordinator_worker_task_notification(
                        agent_id,
                        description,
                        &result_text,
                        AgentCompletionStatus::from_had_error(had_error),
                        duration_ms,
                        usage,
                    ),
                ))
            } else {
                None
            };

        Ok(ToolResult {
            data: json!(result_text),
            model_content,
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
                "fork_metadata": params.fork_metadata(),
            });
            let _ = ctx
                .hook_runner
                .run_event_hooks("SubagentStart", &payload, start_configs)
                .await;
        }

        let mut result = if use_worktree {
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

        if result.is_err() {
            params.close_live_channel(agent_id, "launch_failed");
        }

        // Fire SubagentStop hook
        if !stop_configs.is_empty() {
            let is_error = result.as_ref().is_err();
            let payload = json!({
                "agent_id": agent_id,
                "description": description,
                "is_error": is_error,
                "fork_metadata": params.fork_metadata(),
            });
            let _ = ctx
                .hook_runner
                .run_event_hooks("SubagentStop", &payload, stop_configs)
                .await;
        }

        if let Ok(tool_result) = &mut result {
            if tool_result.model_content.is_some() {
                return result;
            }
            let result_text = tool_result
                .data
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| tool_result.data.to_string());
            if is_coordinator_parent(ctx) && is_worker_subagent(params.subagent_type.as_deref()) {
                let notification = coordinator_worker_task_notification(
                    agent_id,
                    description,
                    &result_text,
                    AgentCompletionStatus::Completed,
                    0,
                    AgentRunUsage::default(),
                );
                tool_result.model_content = Some(ToolResultContent::Text(notification.clone()));
                tool_result.display_preview =
                    Some(format!("Agent '{}' completed for coordinator", description));
            }
        }

        result
    }
}

pub(super) fn is_worker_subagent(subagent_type: Option<&str>) -> bool {
    subagent_type
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("worker"))
}

pub(super) fn is_coordinator_parent(ctx: &ToolUseContext) -> bool {
    if !features::enabled(Feature::Coordinator) {
        return false;
    }
    (ctx.get_app_state)()
        .team_context
        .as_ref()
        .is_none_or(|team_context| allthecodes_types::teams::is_team_lead(Some(team_context)))
}

pub(super) fn coordinator_worker_task_notification(
    agent_id: &str,
    description: &str,
    result_text: &str,
    status: AgentCompletionStatus,
    duration_ms: u64,
    usage: AgentRunUsage,
) -> String {
    let status = status.as_str();
    let summary = format!("Agent \"{}\" {}", description, status);
    build_task_notification_xml(
        agent_id,
        status,
        &summary,
        result_text,
        WorkerNotificationUsage {
            total_tokens: usage.total_tokens,
            tool_uses: usage.tool_uses,
            duration_ms: Some(duration_ms),
            retry_count: None,
        },
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WorkerNotificationUsage {
    total_tokens: Option<u64>,
    tool_uses: Option<u64>,
    duration_ms: Option<u64>,
    retry_count: Option<u64>,
}

fn build_task_notification_xml(
    task_id: &str,
    status: &str,
    summary: &str,
    result: &str,
    usage: WorkerNotificationUsage,
) -> String {
    let mut xml = String::from("<task-notification>");
    push_xml_tag(&mut xml, "task-id", task_id);
    push_xml_tag(&mut xml, "status", status);
    push_xml_tag(&mut xml, "summary", summary);
    push_xml_tag(&mut xml, "result", result);
    if usage.total_tokens.is_some()
        || usage.tool_uses.is_some()
        || usage.duration_ms.is_some()
        || usage.retry_count.is_some()
    {
        xml.push_str("<usage>");
        if let Some(total_tokens) = usage.total_tokens {
            push_xml_tag(&mut xml, "total_tokens", &total_tokens.to_string());
        }
        if let Some(tool_uses) = usage.tool_uses {
            push_xml_tag(&mut xml, "tool_uses", &tool_uses.to_string());
        }
        if let Some(duration_ms) = usage.duration_ms {
            push_xml_tag(&mut xml, "duration_ms", &duration_ms.to_string());
        }
        if let Some(retry_count) = usage.retry_count {
            push_xml_tag(&mut xml, "retry_count", &retry_count.to_string());
        }
        xml.push_str("</usage>");
    }
    xml.push_str("</task-notification>");
    xml
}

pub(super) fn apply_coordinator_worker_turn_limit(
    config: &mut QueryEngineConfig,
    ctx: &ToolUseContext,
    params: &AgentInput,
) {
    let requested = params
        .max_turns
        .filter(|turns| *turns > 0)
        .or(config.max_turns);
    config.max_turns =
        if is_coordinator_parent(ctx) && is_worker_subagent(params.subagent_type.as_deref()) {
            coordinator_worker_turn_limit(requested, coordinator_worker_policy_max_turns_from_env())
        } else {
            requested
        };
}

pub(super) fn coordinator_worker_turn_limit(
    requested: Option<usize>,
    policy: Option<usize>,
) -> Option<usize> {
    match (
        requested.filter(|turns| *turns > 0),
        policy.filter(|turns| *turns > 0),
    ) {
        (Some(requested), Some(policy)) => Some(requested.min(policy)),
        (Some(requested), None) => Some(requested),
        (None, Some(policy)) => Some(policy),
        (None, None) => None,
    }
}

fn coordinator_worker_policy_max_turns_from_env() -> Option<usize> {
    Some(
        std::env::var("ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(12),
    )
}

fn push_xml_tag(xml: &mut String, tag: &str, value: &str) {
    xml.push('<');
    xml.push_str(tag);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push('>');
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

    #[test]
    fn task_notification_model_content_for_coordinator_worker() {
        let text = super::coordinator_worker_task_notification(
            "agent-1",
            "check auth",
            "done <ok>",
            allthecodes_types::agent_events::AgentCompletionStatus::Completed,
            1234,
            super::AgentRunUsage::default(),
        );

        assert!(text.contains("<task-notification>"));
        assert!(text.contains("<task-id>agent-1</task-id>"));
        assert!(text.contains("<status>completed</status>"));
        assert!(text.contains("<summary>Agent &quot;check auth&quot; completed</summary>"));
        assert!(text.contains("done &lt;ok&gt;"));
        assert!(text.contains("<duration_ms>1234</duration_ms>"));
    }

    #[test]
    fn task_notification_skips_non_worker_type() {
        assert!(!super::is_worker_subagent(Some("Explore")));
    }

    #[test]
    fn coordinator_worker_turn_limit_clamps_to_policy_default() {
        assert_eq!(
            super::coordinator_worker_turn_limit(None, Some(12)),
            Some(12)
        );
        assert_eq!(
            super::coordinator_worker_turn_limit(Some(4), Some(12)),
            Some(4)
        );
        assert_eq!(
            super::coordinator_worker_turn_limit(Some(20), Some(12)),
            Some(12)
        );
        assert_eq!(super::coordinator_worker_turn_limit(Some(5), None), Some(5));
    }

    #[test]
    fn task_notification_includes_optional_usage_when_available() {
        let text = super::coordinator_worker_task_notification(
            "agent-1",
            "check usage",
            "done",
            allthecodes_types::agent_events::AgentCompletionStatus::Completed,
            1234,
            super::AgentRunUsage {
                total_tokens: Some(99),
                output_tokens: Some(30),
                tool_uses: Some(2),
            },
        );

        assert!(text.contains("<total_tokens>99</total_tokens>"));
        assert!(text.contains("<tool_uses>2</tool_uses>"));
        assert!(text.contains("<duration_ms>1234</duration_ms>"));
    }
}
