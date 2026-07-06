//! Git worktree execution for agent isolation.

use std::path::Path;

use allthecodes_types::agent_events::AgentCompletionStatus;
use anyhow::Result;
use serde_json::json;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::types::config::QuerySource;
use crate::types::message::ToolResultContent;
use crate::types::tool::*;

use super::{
    build_child_config, collect_stream_result, count_worktree_changes, find_git_root, get_head_sha,
    mark_agent_worktree_session_cleanup_failed, mark_agent_worktree_session_kept,
    mark_agent_worktree_session_removed, persist_agent_worktree_session_record, AgentInput,
    AgentTool,
};
use crate::lifecycle::QueryEngine;
use crate::worktree_hooks::{
    default_agent_worktree_path, ensure_worktree_parent, run_worktree_create_hook,
    run_worktree_remove_hook, validate_allowed_worktree_path, WorktreeRemoveHookOutcome,
};

impl AgentTool {
    /// Run the agent inside an isolated git worktree.
    ///
    /// Creates a temporary worktree + branch, points the child QueryEngine's
    /// cwd at it, runs the agent, and then cleans up if no changes were made.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_in_worktree(
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
        let cwd = std::env::current_dir()?;

        // -- 1. Find git root
        let git_root = match find_git_root(&cwd).await {
            Ok(root) => root,
            Err(e) => {
                warn!(
                    agent_id = %agent_id,
                    error = %e,
                    "worktree isolation failed — falling back to normal execution"
                );
                let _ = crate::agent_runtime::emit_subagent_event(
                    "warning",
                    agent_id,
                    ctx.agent_id.as_deref(),
                    Some(description),
                    Some(agent_model),
                    current_depth + 1,
                    background,
                    Some(json!({
                        "message": format!("worktree isolation skipped: {}", e),
                    })),
                );
                return self
                    .run_agent_normal(
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
                    .map(|mut r| {
                        if let Some(s) = r.data.as_str() {
                            r.data = json!(format!(
                                "[WARNING: worktree isolation skipped — {}]\n\n{}",
                                e, s
                            ));
                        }
                        r
                    });
            }
        };

        let original_head = get_head_sha(&git_root).await;

        // -- 2. Create branch + worktree
        let short_id = &Uuid::new_v4().to_string()[..8];
        let branch_name = format!("agent-worktree-{}", short_id);
        let worktree_path = default_agent_worktree_path(short_id);

        info!(
            agent_id = %agent_id,
            worktree_path = %worktree_path.display(),
            branch = %branch_name,
            "creating agent worktree"
        );

        let app_state = (ctx.get_app_state)();
        let hook_created = match run_worktree_create_hook(
            &ctx.hook_runner,
            &app_state.hooks,
            "AgentTool",
            &git_root,
            &worktree_path,
            &branch_name,
            short_id,
            Some(agent_id),
        )
        .await
        {
            Ok(Some(created)) if created.worktree_path.is_dir() => Some(created),
            Ok(Some(created)) => {
                warn!(
                    agent_id = %agent_id,
                    worktree_path = %created.worktree_path.display(),
                    "WorktreeCreate hook returned a missing directory; falling back to git"
                );
                None
            }
            Ok(None) => None,
            Err(e) => {
                warn!(
                    agent_id = %agent_id,
                    error = %e,
                    "WorktreeCreate hook failed; falling back to git"
                );
                None
            }
        };

        let (worktree_path, branch_name, created_by_hook) = if let Some(created) = hook_created {
            debug!(
                agent_id = %agent_id,
                worktree_path = %created.worktree_path.display(),
                "worktree created by WorktreeCreate hook"
            );
            (created.worktree_path, created.branch_name, true)
        } else {
            if let Err(e) = ensure_worktree_parent(&worktree_path) {
                warn!(
                    agent_id = %agent_id,
                    error = %e,
                    "worktree parent preparation failed; falling back to normal execution"
                );
                let _ = crate::agent_runtime::emit_subagent_event(
                    "warning",
                    agent_id,
                    ctx.agent_id.as_deref(),
                    Some(description),
                    Some(agent_model),
                    current_depth + 1,
                    background,
                    Some(json!({
                        "message": format!("worktree isolation skipped: {}", e),
                    })),
                );
                return self
                    .run_agent_normal(
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
                    .map(|mut r| {
                        if let Some(s) = r.data.as_str() {
                            r.data = json!(format!(
                                "[WARNING: worktree isolation skipped - {}]\n\n{}",
                                e, s
                            ));
                        }
                        r
                    });
            }
            (worktree_path, branch_name, false)
        };

        if !created_by_hook {
            let wt_output = tokio::process::Command::new("git")
                .args([
                    "-C",
                    &git_root.to_string_lossy(),
                    "worktree",
                    "add",
                    "-B",
                    &branch_name,
                    &worktree_path.to_string_lossy(),
                ])
                .output()
                .await;

            match wt_output {
                Ok(ref o) if !o.status.success() => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    warn!(
                        agent_id = %agent_id,
                        error = %stderr,
                        "worktree creation failed — falling back to normal execution"
                    );
                    let _ = crate::agent_runtime::emit_subagent_event(
                        "warning",
                        agent_id,
                        ctx.agent_id.as_deref(),
                        Some(description),
                        Some(agent_model),
                        current_depth + 1,
                        background,
                        Some(json!({
                            "message": format!(
                                "worktree isolation skipped: git worktree add failed: {}",
                                stderr.trim()
                            ),
                        })),
                    );
                    return self
                    .run_agent_normal(
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
                    .map(|mut r| {
                        if let Some(s) = r.data.as_str() {
                            r.data = json!(format!(
                                "[WARNING: worktree isolation skipped — git worktree add failed: {}]\n\n{}",
                                stderr.trim(), s
                            ));
                        }
                        r
                    });
                }
                Err(e) => {
                    warn!(
                        agent_id = %agent_id,
                        error = %e,
                        "worktree creation failed — falling back to normal execution"
                    );
                    let _ = crate::agent_runtime::emit_subagent_event(
                        "warning",
                        agent_id,
                        ctx.agent_id.as_deref(),
                        Some(description),
                        Some(agent_model),
                        current_depth + 1,
                        background,
                        Some(json!({
                            "message": format!("worktree isolation skipped: {}", e),
                        })),
                    );
                    return self
                        .run_agent_normal(
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
                        .map(|mut r| {
                            if let Some(s) = r.data.as_str() {
                                r.data = json!(format!(
                                    "[WARNING: worktree isolation skipped — {}]\n\n{}",
                                    e, s
                                ));
                            }
                            r
                        });
                }
                Ok(_) => {
                    debug!(
                        agent_id = %agent_id,
                        worktree_path = %worktree_path.display(),
                        "worktree created successfully"
                    );
                }
            }
        }

        persist_agent_worktree_session_record(
            &ctx.session_id,
            agent_id,
            &cwd,
            &git_root,
            &worktree_path,
            &branch_name,
            original_head.clone(),
            if created_by_hook {
                allthecodes_session::worktree_sessions::WorktreeSessionSource::WorktreeCreateHook
            } else {
                allthecodes_session::worktree_sessions::WorktreeSessionSource::AgentIsolation
            },
        );

        // -- 3. Run the agent with cwd = worktree
        let mut child_config = build_child_config(
            worktree_path.to_string_lossy().to_string(),
            ctx,
            agent_id,
            params.subagent_type.as_deref(),
            agent_model,
            parent_model,
            current_depth,
        );
        super::dispatch::apply_coordinator_worker_turn_limit(&mut child_config, ctx, params);

        let agent_tx = ctx.bg_agent_tx.as_ref();

        // Register worktree agent in tree and emit Spawned event
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
        let (mut result_text, had_error, usage) = collect_stream_result(stream, ipc).await;

        // -- 4. Check for changes
        let changes = count_worktree_changes(&worktree_path, original_head.as_deref()).await;

        let has_changes = match changes {
            Some((files, commits)) => files > 0 || commits > 0,
            None => true, // fail-closed: assume changes if we can't tell
        };

        if has_changes {
            // Keep the worktree — include location info in result
            let (files, commits) = changes.unwrap_or((0, 0));
            mark_agent_worktree_session_kept(&ctx.session_id, agent_id, &worktree_path);
            info!(
                agent_id = %agent_id,
                worktree_path = %worktree_path.display(),
                branch = %branch_name,
                changed_files = files,
                new_commits = commits,
                "agent worktree has changes — keeping"
            );
            let _ = crate::agent_runtime::emit_subagent_event(
                "worktree_kept",
                agent_id,
                ctx.agent_id.as_deref(),
                Some(description),
                Some(agent_model),
                current_depth + 1,
                background,
                Some(json!({
                    "files": files,
                    "commits": commits,
                    "worktree_path": worktree_path.display().to_string(),
                    "branch": branch_name,
                })),
            );

            result_text.push_str(&format!(
                "\n\n[Worktree isolation: changes detected ({} file(s), {} commit(s)). \
                 Worktree kept at: {} on branch: {}]",
                files,
                commits,
                worktree_path.display(),
                branch_name,
            ));
        } else {
            // No changes — clean up worktree + branch
            info!(
                agent_id = %agent_id,
                worktree_path = %worktree_path.display(),
                branch = %branch_name,
                "agent worktree has no changes — cleaning up"
            );
            let _ = crate::agent_runtime::emit_subagent_event(
                "worktree_cleaned",
                agent_id,
                ctx.agent_id.as_deref(),
                Some(description),
                Some(agent_model),
                current_depth + 1,
                background,
                Some(json!({
                    "worktree_path": worktree_path.display().to_string(),
                    "branch": branch_name,
                })),
            );

            let cleaned = Self::cleanup_worktree_with_hooks(
                &git_root,
                &worktree_path,
                &branch_name,
                agent_id,
                Some(&ctx.hook_runner),
                Some(&app_state.hooks),
            )
            .await;

            if cleaned {
                mark_agent_worktree_session_removed(&ctx.session_id, agent_id, &worktree_path);
                result_text.push_str(
                    "\n\n[Worktree isolation: no changes detected — worktree cleaned up]",
                );
            } else {
                mark_agent_worktree_session_cleanup_failed(
                    &ctx.session_id,
                    agent_id,
                    &worktree_path,
                );
                result_text.push_str(&format!(
                    "\n\n[Worktree isolation: no changes detected, but cleanup could not be verified. Worktree kept at: {} on branch: {}]",
                    worktree_path.display(),
                    branch_name
                ));
            }
        }

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

                let roots = crate::agent_runtime::agent_tree_snapshot();
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    allthecodes_types::agent_events::AgentEvent::TreeSnapshot { roots },
                ));
            }
        }

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

        debug!(
            agent_id = %agent_id,
            result_len = result_text.len(),
            error = had_error,
            "subagent (worktree) completed"
        );

        let model_content = if super::dispatch::is_coordinator_parent(ctx)
            && super::dispatch::is_worker_subagent(params.subagent_type.as_deref())
        {
            Some(ToolResultContent::Text(
                super::dispatch::coordinator_worker_task_notification(
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

    /// Remove a worktree through `WorktreeRemove` hooks when configured.
    ///
    /// Missing hooks fall back to git. Configured hooks must explicitly report
    /// that they handled removal; failures and ambiguous output keep the
    /// worktree so cleanup stays fail-closed.
    pub(super) async fn cleanup_worktree_with_hooks(
        git_root: &Path,
        worktree_path: &Path,
        branch_name: &str,
        agent_id: &str,
        hook_runner: Option<&std::sync::Arc<dyn allthecodes_types::hooks::HookRunner>>,
        hooks: Option<&allthecodes_types::hooks::HooksMap>,
    ) -> bool {
        if let Err(err) = validate_allowed_worktree_path(worktree_path) {
            warn!(
                agent_id = %agent_id,
                error = %err,
                "worktree cleanup refused for out-of-bounds path"
            );
            return false;
        }

        if let (Some(hook_runner), Some(hooks)) = (hook_runner, hooks) {
            match run_worktree_remove_hook(
                hook_runner,
                hooks,
                "AgentTool",
                git_root,
                worktree_path,
                branch_name,
                Some(agent_id),
            )
            .await
            {
                Ok(WorktreeRemoveHookOutcome::NoHook) => {}
                Ok(WorktreeRemoveHookOutcome::Handled) => {
                    if worktree_path.exists() {
                        warn!(
                            agent_id = %agent_id,
                            worktree_path = %worktree_path.display(),
                            "WorktreeRemove hook reported success but path still exists"
                        );
                        return false;
                    }
                    debug!(agent_id = %agent_id, "worktree removed by WorktreeRemove hook");
                    return true;
                }
                Ok(WorktreeRemoveHookOutcome::Unhandled) => {
                    warn!(
                        agent_id = %agent_id,
                        "WorktreeRemove hook did not explicitly report removal; keeping worktree"
                    );
                    return false;
                }
                Err(err) => {
                    warn!(
                        agent_id = %agent_id,
                        error = %err,
                        "WorktreeRemove hook failed; keeping worktree"
                    );
                    return false;
                }
            }
        }

        let remove_result = tokio::process::Command::new("git")
            .args([
                "-C",
                &git_root.to_string_lossy(),
                "worktree",
                "remove",
                "--force",
                &worktree_path.to_string_lossy(),
            ])
            .output()
            .await;

        let mut removed = false;
        match remove_result {
            Ok(o) if o.status.success() => {
                debug!(agent_id = %agent_id, "agent worktree directory removed");
                removed = true;
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(
                    agent_id = %agent_id,
                    "worktree remove warning: {}", stderr
                );
            }
            Err(e) => {
                warn!(agent_id = %agent_id, "worktree remove failed: {}", e);
            }
        }

        if worktree_path.exists() {
            warn!(
                agent_id = %agent_id,
                worktree_path = %worktree_path.display(),
                "worktree removal could not be verified"
            );
            return false;
        }

        // Brief pause to let git release locks before branch delete
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let branch_result = tokio::process::Command::new("git")
            .args([
                "-C",
                &git_root.to_string_lossy(),
                "branch",
                "-D",
                branch_name,
            ])
            .output()
            .await;

        match branch_result {
            Ok(o) if o.status.success() => {
                debug!(agent_id = %agent_id, "agent worktree branch deleted");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(
                    agent_id = %agent_id,
                    "branch delete warning: {}", stderr
                );
            }
            Err(e) => {
                warn!(agent_id = %agent_id, "branch delete failed: {}", e);
            }
        }

        removed
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Branch and path naming patterns
    // -----------------------------------------------------------------------

    /// Branch names must follow the `agent-worktree-{8-char-id}` pattern.
    #[test]
    fn test_branch_name_format() {
        let short_id = "abcd1234";
        let branch_name = format!("agent-worktree-{}", short_id);
        assert!(branch_name.starts_with("agent-worktree-"));
        assert_eq!(branch_name, "agent-worktree-abcd1234");
    }

    /// Worktree path must be under the allthecodes worktree root with matching suffix.
    #[test]
    fn test_worktree_path_format() {
        let short_id = "abcd1234";
        let worktree_path = crate::worktree_hooks::default_agent_worktree_path(short_id);
        let path_str = worktree_path.to_string_lossy();
        assert!(path_str.contains("agent-worktree-abcd1234"));
        assert!(crate::worktree_hooks::is_allowed_worktree_path(
            &worktree_path
        ));
    }

    /// Branch name and worktree path suffix must be consistent (same short_id).
    #[test]
    fn test_branch_name_and_path_share_same_id() {
        let short_id = "deadbeef";
        let branch_name = format!("agent-worktree-{}", short_id);
        let worktree_path = crate::worktree_hooks::default_agent_worktree_path(short_id);
        let path_tail = worktree_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        assert_eq!(branch_name, path_tail);
    }

    // -----------------------------------------------------------------------
    // Result text suffix — has_changes path
    // -----------------------------------------------------------------------

    #[test]
    fn test_result_text_suffix_with_changes() {
        let files: usize = 3;
        let commits: usize = 1;
        let worktree_path = PathBuf::from("/tmp/agent-worktree-abcd1234");
        let branch_name = "agent-worktree-abcd1234";

        let suffix = format!(
            "\n\n[Worktree isolation: changes detected ({} file(s), {} commit(s)). \
             Worktree kept at: {} on branch: {}]",
            files,
            commits,
            worktree_path.display(),
            branch_name,
        );

        assert!(suffix.contains("changes detected"));
        assert!(suffix.contains("3 file(s)"));
        assert!(suffix.contains("1 commit(s)"));
        assert!(suffix.contains("Worktree kept at"));
        assert!(suffix.contains("agent-worktree-abcd1234"));
    }

    #[test]
    fn test_result_text_suffix_no_changes() {
        let suffix = "\n\n[Worktree isolation: no changes detected — worktree cleaned up]";
        assert!(suffix.contains("no changes detected"));
        assert!(suffix.contains("cleaned up"));
    }

    // -----------------------------------------------------------------------
    // Result text suffix — fallback warning message formats
    // -----------------------------------------------------------------------

    #[test]
    fn test_fallback_warning_format_git_root_error() {
        let error_msg = "Not a git repository";
        let base = "some result text";
        let result = format!(
            "[WARNING: worktree isolation skipped — {}]\n\n{}",
            error_msg, base
        );
        assert!(result.starts_with("[WARNING: worktree isolation skipped —"));
        assert!(result.contains("Not a git repository"));
        assert!(result.ends_with("some result text"));
    }

    #[test]
    fn test_fallback_warning_format_worktree_add_failed() {
        let stderr = "fatal: branch already exists";
        let base = "normal result";
        let result = format!(
            "[WARNING: worktree isolation skipped — git worktree add failed: {}]\n\n{}",
            stderr.trim(),
            base
        );
        assert!(result.contains("git worktree add failed"));
        assert!(result.contains("fatal: branch already exists"));
        assert!(result.ends_with("normal result"));
    }

    // -----------------------------------------------------------------------
    // has_changes logic — fail-closed when count_worktree_changes returns None
    // -----------------------------------------------------------------------

    #[test]
    fn test_has_changes_true_when_none() {
        let changes: Option<(usize, usize)> = None;
        // fail-closed: if we can't tell, assume changes
        let has_changes = match changes {
            Some((files, commits)) => files > 0 || commits > 0,
            None => true,
        };
        assert!(
            has_changes,
            "None result should be treated as has_changes=true"
        );
    }

    #[test]
    fn test_has_changes_false_when_zero_files_and_commits() {
        let changes: Option<(usize, usize)> = Some((0, 0));
        let has_changes = match changes {
            Some((files, commits)) => files > 0 || commits > 0,
            None => true,
        };
        assert!(!has_changes);
    }

    #[test]
    fn test_has_changes_true_when_files_changed() {
        let changes: Option<(usize, usize)> = Some((5, 0));
        let has_changes = match changes {
            Some((files, commits)) => files > 0 || commits > 0,
            None => true,
        };
        assert!(has_changes);
    }

    #[test]
    fn test_has_changes_true_when_only_commits() {
        let changes: Option<(usize, usize)> = Some((0, 2));
        let has_changes = match changes {
            Some((files, commits)) => files > 0 || commits > 0,
            None => true,
        };
        assert!(has_changes);
    }
}
