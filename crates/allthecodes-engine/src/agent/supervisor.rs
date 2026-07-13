//! Background agent supervisor.
//!
//! Keeps background `Agent` runs under one lifecycle owner instead of letting
//! each tool call detach an untracked task. The supervisor owns registration,
//! cancellation, worktree pre-spawn setup, shutdown cleanup, and task output
//! tracking.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::lifecycle::QueryEngine;
use allthecodes_tasks::{
    AgentRuntimeActivity, AgentRuntimePhase, TaskCreateOptions, TaskEntry, TaskStatus,
    DEFAULT_TASK_LIST_ID,
};
use allthecodes_types::agent_events::AgentCompletionStatus;

use crate::types::config::{QueryEngineConfig, QuerySource};
use crate::types::tool::*;
use crate::utils::bash::validate_working_directory;
use crate::worktree_hooks::{
    default_agent_worktree_path, ensure_worktree_parent, run_worktree_create_hook,
};

use super::{
    agent_run_usage_from_tracking, assistant_tool_use_count, build_child_config,
    count_worktree_changes, find_git_root, get_head_sha,
    mark_agent_worktree_session_cleanup_failed, mark_agent_worktree_session_kept,
    mark_agent_worktree_session_removed, persist_agent_worktree_session_record, sdk_to_agent_event,
    AgentInput, AgentRunUsage, AgentTool,
};

const SHUTDOWN_WAIT_PER_AGENT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) struct BackgroundLaunch {
    pub(super) task_id: String,
    pub(super) worktree_path: Option<String>,
    pub(super) worktree_branch: Option<String>,
}

#[derive(Clone)]
struct WorktreeRuntime {
    session_id: String,
    git_root: PathBuf,
    worktree_path: PathBuf,
    branch_name: String,
    original_head: Option<String>,
    hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    hooks: allthecodes_types::hooks::HooksMap,
}

struct PreparedRuntime {
    child_cwd: String,
    worktree: Option<WorktreeRuntime>,
    startup_warning: Option<String>,
}

struct BackgroundJob {
    agent_id: String,
    task_ref: crate::agent_runtime::AgentTaskRef,
    cancellation_token: CancellationToken,
    handle: Option<tokio::task::JoinHandle<()>>,
    worktree: Option<WorktreeRuntime>,
}

#[derive(Default)]
struct SupervisorState {
    active: HashMap<String, BackgroundJob>,
    known_tasks: HashMap<String, crate::agent_runtime::AgentTaskRef>,
}

#[derive(Default)]
struct BackgroundSupervisor {
    state: Mutex<SupervisorState>,
}

static BACKGROUND_SUPERVISOR: std::sync::LazyLock<BackgroundSupervisor> =
    std::sync::LazyLock::new(BackgroundSupervisor::default);

#[allow(clippy::too_many_arguments)]
pub(super) async fn spawn_background_agent(
    params: AgentInput,
    ctx: &ToolUseContext,
    agent_id: String,
    description: String,
    subagent_type: String,
    agent_model: String,
    parent_model: String,
    current_depth: usize,
    use_worktree: bool,
    bg_tx: allthecodes_types::agent_channel::AgentSender,
    start_configs: Vec<allthecodes_types::hooks::HookEventConfig>,
    stop_configs: Vec<allthecodes_types::hooks::HookEventConfig>,
) -> Result<BackgroundLaunch> {
    let child_session_id = params
        .delegate_session_id
        .clone()
        .unwrap_or_else(|| agent_id.clone());
    if !start_configs.is_empty() {
        let payload = json!({
            "agent_id": &agent_id,
            "prompt": &params.prompt,
            "description": &description,
            "subagent_type": &subagent_type,
            "model": &agent_model,
            "depth": current_depth + 1,
            "background": true,
        });
        let _ = ctx
            .hook_runner
            .run_event_hooks("SubagentStart", &payload, &start_configs)
            .await;
    }

    let prepared = prepare_runtime(
        use_worktree,
        &agent_id,
        &description,
        &agent_model,
        current_depth,
        ctx.agent_id.as_deref(),
        &child_session_id,
        ctx.hook_runner.clone(),
        (ctx.get_app_state)().hooks,
        params.delegate_cwd.as_deref().unwrap_or(&ctx.cwd),
        params.delegate_worktree_slug.as_deref(),
    )
    .await?;
    validate_working_directory(&prepared.child_cwd)?;

    let mut child_config = build_child_config(
        prepared.child_cwd.clone(),
        ctx,
        &agent_id,
        params.subagent_type.as_deref(),
        &agent_model,
        &parent_model,
        current_depth,
    );
    child_config.verification_policy = params.verification_policy.clone();
    super::dispatch::apply_coordinator_worker_turn_limit(&mut child_config, ctx, &params);
    let delegated = params.delegate_task_id.is_some();
    if delegated {
        child_config.persist_session = true;
        child_config.auto_save_session = true;
    }

    let task_store = crate::agent_runtime::global_task_store();
    let task_list_id = params
        .delegate_task_list_id
        .clone()
        .unwrap_or_else(|| DEFAULT_TASK_LIST_ID.to_string());
    let worktree_path = prepared
        .worktree
        .as_ref()
        .map(|wt| wt.worktree_path.display().to_string());
    let worktree_branch = prepared.worktree.as_ref().map(|wt| wt.branch_name.clone());
    let task_ref = if let Some(task_id) = params.delegate_task_id.clone() {
        let task_ref = crate::agent_runtime::AgentTaskRef {
            task_list_id,
            task_id,
        };
        if let Err(err) = task_store.adopt_pending_agent_task(
            &task_ref,
            &agent_id,
            &child_session_id,
            worktree_path.clone(),
            worktree_branch.clone(),
        ) {
            if let Some(worktree) = prepared.worktree.clone() {
                cleanup_failed_launch_worktree(&agent_id, worktree).await;
            }
            return Err(err);
        }
        task_ref
    } else {
        let task_entry = task_store.try_create_with_options(
            &task_list_id,
            &description,
            &params.prompt,
            TaskCreateOptions {
                kind: Some("local_agent".to_string()),
                agent_id: Some(agent_id.clone()),
                supervisor_id: Some(agent_id.clone()),
                isolation: use_worktree.then(|| "worktree".to_string()),
                worktree_path: worktree_path.clone(),
                worktree_branch: worktree_branch.clone(),
                ..TaskCreateOptions::default()
            },
        )?;
        let task_ref = crate::agent_runtime::AgentTaskRef {
            task_list_id,
            task_id: task_entry.id,
        };
        task_store.try_update_status(&task_ref, TaskStatus::InProgress)?;
        task_ref
    };

    let cancellation_token = CancellationToken::new();
    task_store.register_runtime_handle(&task_ref, cancellation_token.clone());

    register_agent_tree(
        &agent_id,
        ctx.agent_id.clone(),
        &description,
        params.subagent_type.clone(),
        &agent_model,
        current_depth,
        ctx.query_tracking
            .as_ref()
            .map(|t| t.chain_id.clone())
            .unwrap_or_default(),
        &bg_tx,
    );

    BACKGROUND_SUPERVISOR.register(BackgroundJob {
        agent_id: agent_id.clone(),
        task_ref: task_ref.clone(),
        cancellation_token: cancellation_token.clone(),
        handle: None,
        worktree: prepared.worktree.clone(),
    });

    let runtime = AgentRuntime {
        child_config,
        prompt: params.prompt,
        agent_id: agent_id.clone(),
        task_ref: task_ref.clone(),
        child_session_id: params.delegate_session_id.clone(),
        description: description.clone(),
        subagent_type: Some(subagent_type.clone()),
        parent_agent_id: ctx.agent_id.clone(),
        agent_model: agent_model.clone(),
        depth: current_depth + 1,
        bg_tx: bg_tx.clone(),
        task_store: task_store.clone(),
        cancellation_token,
        startup_warning: prepared.startup_warning,
        worktree: prepared.worktree,
        stop_configs,
        hook_runner: ctx.hook_runner.clone(),
        command_dispatcher: ctx.command_dispatcher.clone(),
        permission_callback: ctx.permission_callback.clone(),
        ask_user_callback: ctx.ask_user_callback.clone(),
        permission_pending_count: Arc::new(AtomicUsize::new(0)),
    };

    let handle = tokio::spawn(async move {
        runtime.run().await;
    });
    BACKGROUND_SUPERVISOR.attach_handle(&agent_id, handle);

    Ok(BackgroundLaunch {
        task_id: task_ref.task_id,
        worktree_path,
        worktree_branch,
    })
}

pub fn cancel_agent(agent_id: &str) -> Option<String> {
    let task_ref = BACKGROUND_SUPERVISOR.cancel_agent(agent_id)?;
    if let Err(err) = crate::agent_runtime::global_task_store().try_stop(&task_ref) {
        warn!(task_id = %task_ref.task_id, error = %err, "failed to stop background agent task");
    }
    Some(task_ref.task_id)
}

pub fn output_for_agent(agent_id: &str) -> Option<TaskEntry> {
    let task_ref = BACKGROUND_SUPERVISOR.task_ref(agent_id)?;
    let entry = crate::agent_runtime::global_task_store().get(&task_ref);
    if entry.as_ref().is_some_and(|task| task.status.is_terminal()) {
        BACKGROUND_SUPERVISOR.forget(agent_id);
    }
    entry
}

pub fn output_events_for_agent(
    agent_id: &str,
    after_seq: Option<allthecodes_types::output::EventSeq>,
    limit_bytes: usize,
) -> Result<Option<(String, allthecodes_types::output::OutputReadBatch)>> {
    let Some(task_ref) = BACKGROUND_SUPERVISOR.task_ref(agent_id) else {
        return Ok(None);
    };
    let store = crate::agent_runtime::global_task_store();
    let output = store.read_output_events(&task_ref, after_seq, limit_bytes)?;
    if store
        .get(&task_ref)
        .as_ref()
        .is_some_and(|task| task.status.is_terminal())
    {
        BACKGROUND_SUPERVISOR.forget(agent_id);
    }
    Ok(output.map(|output| (task_ref.task_id, output)))
}

pub async fn shutdown_all(reason: &str) -> usize {
    let jobs = BACKGROUND_SUPERVISOR.take_active_jobs();
    let count = jobs.len();

    for mut job in jobs {
        job.cancellation_token.cancel();
        let _ = crate::agent_runtime::global_task_store().append_output(
            &job.task_ref,
            &format!("[Supervisor: cancelled during shutdown: {}]", reason),
        );
        if let Err(err) = crate::agent_runtime::global_task_store().try_stop(&job.task_ref) {
            warn!(
                task_id = %job.task_ref.task_id,
                error = %err,
                "failed to stop background agent task during shutdown"
            );
        }

        if let Some(handle) = job.handle.take() {
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(SHUTDOWN_WAIT_PER_AGENT, handle).await {
                Ok(join_result) => {
                    if let Err(err) = join_result {
                        warn!(
                            agent_id = %job.agent_id,
                            error = %err,
                            "background agent task failed while shutting down"
                        );
                    }
                }
                Err(_) => {
                    warn!(
                        agent_id = %job.agent_id,
                        task_id = %job.task_ref.task_id,
                        "background agent did not stop before shutdown timeout"
                    );
                    abort_handle.abort();
                    if let Some(worktree) = job.worktree.take() {
                        finalize_or_keep_worktree_after_forced_shutdown(
                            &job.agent_id,
                            &job.task_ref,
                            worktree,
                        )
                        .await;
                    }
                }
            }
        }
    }

    count
}

struct AgentRuntime {
    child_config: QueryEngineConfig,
    prompt: String,
    agent_id: String,
    task_ref: crate::agent_runtime::AgentTaskRef,
    child_session_id: Option<String>,
    description: String,
    subagent_type: Option<String>,
    parent_agent_id: Option<String>,
    agent_model: String,
    depth: usize,
    bg_tx: allthecodes_types::agent_channel::AgentSender,
    task_store: Arc<dyn crate::agent_runtime::AgentTaskStore>,
    cancellation_token: CancellationToken,
    startup_warning: Option<String>,
    worktree: Option<WorktreeRuntime>,
    stop_configs: Vec<allthecodes_types::hooks::HookEventConfig>,
    hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    command_dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
    permission_callback: Option<PermissionCallback>,
    ask_user_callback: Option<AskUserCallback>,
    permission_pending_count: Arc<AtomicUsize>,
}

impl AgentRuntime {
    async fn run(self) {
        let started = std::time::Instant::now();
        info!(
            agent_id = %self.agent_id,
            task_id = %self.task_ref.task_id,
            description = %self.description,
            "background agent started"
        );

        let mut child_engine = QueryEngine::new(self.child_config);
        if let Some(session_id) = self.child_session_id.as_deref() {
            child_engine
                .set_current_session_id(crate::bootstrap::SessionId::from_string(session_id));
        }
        child_engine.set_hook_runner(self.hook_runner.clone());
        child_engine.set_command_dispatcher(self.command_dispatcher.clone());
        if let Some(callback) = self.permission_callback.clone() {
            let bg_tx = self.bg_tx.clone();
            let agent_id = self.agent_id.clone();
            let pending_count = self.permission_pending_count.clone();
            let task_store = self.task_store.clone();
            let task_ref = self.task_ref.clone();
            let child_session_id = self
                .child_session_id
                .clone()
                .unwrap_or_else(|| self.agent_id.clone());
            child_engine.set_permission_callback(Arc::new(move |request| {
                let callback = callback.clone();
                let bg_tx = bg_tx.clone();
                let agent_id = agent_id.clone();
                let pending_count = pending_count.clone();
                let task_store = task_store.clone();
                let task_ref = task_ref.clone();
                let child_session_id = child_session_id.clone();
                Box::pin(async move {
                    let queue_position = pending_count.fetch_add(1, Ordering::SeqCst) + 1;
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let _ = task_store.update_runtime_activity(
                        &task_ref,
                        AgentRuntimeActivity {
                            phase: AgentRuntimePhase::WaitingForPermission,
                            last_heartbeat_at_ms: now_ms,
                            last_progress_at_ms: now_ms,
                            task_id: task_ref.task_id.clone(),
                            agent_id: agent_id.clone(),
                            child_session_id: child_session_id.clone(),
                            partial_output_bytes: task_store
                                .get(&task_ref)
                                .map(|task| task.output_bytes)
                                .unwrap_or_default(),
                        },
                    );
                    let _ = bg_tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                        allthecodes_types::agent_events::AgentEvent::PermissionQueued {
                            agent_id: agent_id.clone(),
                            tool_use_id: request.tool_use_id.clone(),
                            tool_name: request.tool_name.clone(),
                            summary: request.legacy_command(),
                            queue_position,
                            pending_count: pending_count.load(Ordering::SeqCst),
                        },
                    ));
                    let decision = callback(request.clone()).await;
                    let remaining = pending_count.fetch_sub(1, Ordering::SeqCst) - 1;
                    if remaining == 0 {
                        let now_ms = chrono::Utc::now().timestamp_millis();
                        let _ = task_store.update_runtime_activity(
                            &task_ref,
                            AgentRuntimeActivity {
                                phase: AgentRuntimePhase::Running,
                                last_heartbeat_at_ms: now_ms,
                                last_progress_at_ms: now_ms,
                                task_id: task_ref.task_id.clone(),
                                agent_id: agent_id.clone(),
                                child_session_id: child_session_id.clone(),
                                partial_output_bytes: task_store
                                    .get(&task_ref)
                                    .map(|task| task.output_bytes)
                                    .unwrap_or_default(),
                            },
                        );
                    }
                    let _ = bg_tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                        allthecodes_types::agent_events::AgentEvent::PermissionResolved {
                            agent_id,
                            tool_use_id: request.tool_use_id,
                            tool_name: request.tool_name,
                            decision: decision.decision.clone(),
                            pending_count: remaining,
                        },
                    ));
                    decision
                })
            }));
        }
        if let Some(callback) = self.ask_user_callback.clone() {
            child_engine.set_ask_user_callback(callback);
        }
        child_engine.set_bg_agent_tx(self.bg_tx.clone());

        let stream =
            child_engine.submit_message(&self.prompt, QuerySource::Agent(self.agent_id.clone()));
        let mut stream = std::pin::pin!(stream);
        let mut result_text = String::new();
        let mut had_error = false;
        let mut was_cancelled = false;
        let mut tool_uses = 0;
        let mut usage = AgentRunUsage::default();
        let mut persisted_output_bytes = 0usize;
        let mut turn_had_delta = false;
        let mut last_progress_at_ms = chrono::Utc::now().timestamp_millis();
        let mut last_progress = tokio::time::Instant::now();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
        heartbeat.tick().await;

        loop {
            let msg = tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    child_engine.abort();
                    was_cancelled = true;
                    had_error = true;
                    break;
                }
                _ = heartbeat.tick() => {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let waiting = self.permission_pending_count.load(Ordering::SeqCst) > 0;
                    let phase = if waiting {
                        AgentRuntimePhase::WaitingForPermission
                    } else if last_progress.elapsed() >= Duration::from_secs(5 * 60) {
                        AgentRuntimePhase::Stalled
                    } else {
                        AgentRuntimePhase::Running
                    };
                    let partial_output_bytes = self
                        .task_store
                        .get(&self.task_ref)
                        .map(|task| task.output_bytes)
                        .unwrap_or_default();
                    let activity = AgentRuntimeActivity {
                            phase,
                            last_heartbeat_at_ms: now_ms,
                            last_progress_at_ms,
                            task_id: self.task_ref.task_id.clone(),
                            agent_id: self.agent_id.clone(),
                            child_session_id: self
                                .child_session_id
                                .clone()
                                .unwrap_or_else(|| self.agent_id.clone()),
                            partial_output_bytes,
                        };
                    let _ = self
                        .task_store
                        .update_runtime_activity(&self.task_ref, activity.clone());
                    emit_runtime_activity(&self.bg_tx, &activity);
                    continue;
                }
                msg = stream.next() => msg,
            };

            let Some(msg) = msg else {
                break;
            };
            last_progress_at_ms = chrono::Utc::now().timestamp_millis();
            last_progress = tokio::time::Instant::now();

            match &msg {
                allthecodes_types::sdk::SdkMessage::StreamEvent(event) => match &event.event {
                    crate::types::message::StreamEvent::MessageStart { .. } => {
                        turn_had_delta = false;
                    }
                    crate::types::message::StreamEvent::ContentBlockDelta { delta, .. } => {
                        if let Some(text) = delta
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            turn_had_delta = true;
                            persisted_output_bytes += text.len();
                            self.task_store.append_output(&self.task_ref, text);
                        }
                    }
                    _ => {}
                },
                allthecodes_types::sdk::SdkMessage::Assistant(assistant_msg) => {
                    tool_uses += assistant_tool_use_count(assistant_msg);
                    for block in &assistant_msg.message.content {
                        if let crate::types::message::ContentBlock::Text { text } = block {
                            if !result_text.is_empty() {
                                result_text.push('\n');
                            }
                            result_text.push_str(text);
                            if !turn_had_delta && !text.is_empty() {
                                persisted_output_bytes += text.len();
                                self.task_store.append_output(&self.task_ref, text);
                            }
                        }
                    }
                }
                allthecodes_types::sdk::SdkMessage::Result(sdk_result) => {
                    usage = agent_run_usage_from_tracking(&sdk_result.usage, tool_uses);
                    if sdk_result.is_error {
                        had_error = true;
                        if !sdk_result.result.is_empty() {
                            result_text = sdk_result.result.clone();
                        }
                    } else if result_text.is_empty() && !sdk_result.result.is_empty() {
                        result_text = sdk_result.result.clone();
                    }
                }
                _ => {}
            }

            if let Some(agent_event) = sdk_to_agent_event(&msg, &self.agent_id) {
                let _ = self
                    .bg_tx
                    .send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                        agent_event,
                    ));
            }
        }

        if result_text.is_empty() {
            result_text = if was_cancelled {
                "(Agent cancelled before producing text output)".to_string()
            } else {
                "(Agent completed with no text output)".to_string()
            };
        }
        if usage.tool_uses.is_none() && tool_uses > 0 {
            usage.tool_uses = Some(tool_uses);
        }

        let mut terminal_diagnostics = String::new();
        if let Some(warning) = &self.startup_warning {
            result_text = format!("[WARNING: {}]\n\n{}", warning, result_text);
            terminal_diagnostics.push_str(&format!("\n[WARNING: {warning}]"));
        }

        if let Some(worktree) = &self.worktree {
            let before_worktree = result_text.len();
            append_worktree_outcome(
                &mut result_text,
                &self.agent_id,
                &self.task_ref.task_id,
                worktree,
                self.parent_agent_id.as_deref(),
                &self.description,
                &self.agent_model,
                self.depth,
            )
            .await;
            terminal_diagnostics.push_str(&result_text[before_worktree..]);
        }

        let duration_ms = started.elapsed().as_millis() as u64;
        let completion_status = if was_cancelled {
            AgentCompletionStatus::Killed
        } else {
            AgentCompletionStatus::from_had_error(had_error)
        };
        let final_status = if was_cancelled {
            TaskStatus::Cancelled
        } else if had_error {
            TaskStatus::Failed
        } else {
            TaskStatus::Completed
        };
        let final_phase = if was_cancelled {
            AgentRuntimePhase::Cancelled
        } else if had_error {
            AgentRuntimePhase::Failed
        } else {
            AgentRuntimePhase::Completed
        };

        if persisted_output_bytes == 0 {
            self.task_store.append_output(&self.task_ref, &result_text);
        } else if !terminal_diagnostics.is_empty() {
            self.task_store
                .append_output(&self.task_ref, &terminal_diagnostics);
        }
        let may_finalize = self
            .task_store
            .get(&self.task_ref)
            .is_some_and(|task| task.status == TaskStatus::InProgress);
        if may_finalize {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let partial_output_bytes = self
                .task_store
                .get(&self.task_ref)
                .map(|task| task.output_bytes)
                .unwrap_or_default();
            let activity = AgentRuntimeActivity {
                phase: final_phase,
                last_heartbeat_at_ms: now_ms,
                last_progress_at_ms: now_ms,
                task_id: self.task_ref.task_id.clone(),
                agent_id: self.agent_id.clone(),
                child_session_id: self
                    .child_session_id
                    .clone()
                    .unwrap_or_else(|| self.agent_id.clone()),
                partial_output_bytes,
            };
            let _ = self
                .task_store
                .update_runtime_activity(&self.task_ref, activity.clone());
            emit_runtime_activity(&self.bg_tx, &activity);
            if let Err(err) = self
                .task_store
                .try_update_status(&self.task_ref, final_status)
            {
                warn!(
                    task_id = %self.task_ref.task_id,
                    error = %err,
                    "failed to persist background agent final status"
                );
            }
        }
        self.task_store.unregister_runtime_handle(&self.task_ref);
        BACKGROUND_SUPERVISOR.complete(&self.agent_id);

        let _ = crate::agent_runtime::emit_subagent_event(
            "background_complete",
            &self.agent_id,
            self.parent_agent_id.as_deref(),
            Some(&self.description),
            Some(&self.agent_model),
            self.depth,
            true,
            Some(json!({
                "task_id": self.task_ref.task_id,
                "duration_ms": duration_ms,
                "result_len": result_text.len(),
                "had_error": had_error,
                "cancelled": was_cancelled,
            })),
        );

        if !self.stop_configs.is_empty() {
            let payload = json!({
                "agent_id": &self.agent_id,
                "description": &self.description,
                "is_error": had_error,
                "background": true,
            });
            let _ = self
                .hook_runner
                .run_event_hooks("SubagentStop", &payload, &self.stop_configs)
                .await;
        }

        let result_preview = preview(&result_text);
        crate::agent_runtime::update_agent_state(
            &self.agent_id,
            if was_cancelled {
                "cancelled"
            } else if had_error {
                "error"
            } else {
                "completed"
            },
            Some(result_preview.clone()),
            Some(duration_ms),
            had_error,
        );

        let _ = self
            .bg_tx
            .send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                allthecodes_types::agent_events::AgentEvent::Completed {
                    agent_id: self.agent_id.clone(),
                    result_preview,
                    had_error,
                    completion_status,
                    duration_ms,
                    total_tokens: usage.total_tokens,
                    output_tokens: usage.output_tokens,
                    tool_uses: usage.tool_uses,
                    agent_type: self.subagent_type.clone(),
                },
            ));

        let roots = crate::agent_runtime::agent_tree_snapshot();
        let _ = self
            .bg_tx
            .send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                allthecodes_types::agent_events::AgentEvent::TreeSnapshot { roots },
            ));
    }
}

fn emit_runtime_activity(
    bg_tx: &allthecodes_types::agent_channel::AgentSender,
    activity: &AgentRuntimeActivity,
) {
    let _ = bg_tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
        allthecodes_types::agent_events::AgentEvent::RuntimeActivity {
            task_id: activity.task_id.clone(),
            agent_id: activity.agent_id.clone(),
            child_session_id: activity.child_session_id.clone(),
            phase: activity.phase.as_str().to_string(),
            last_heartbeat_at_ms: activity.last_heartbeat_at_ms,
            last_progress_at_ms: activity.last_progress_at_ms,
            partial_output_bytes: activity.partial_output_bytes,
        },
    ));
}

impl BackgroundSupervisor {
    fn register(&self, job: BackgroundJob) {
        let mut state = self.state.lock();
        state
            .known_tasks
            .insert(job.agent_id.clone(), job.task_ref.clone());
        state.active.insert(job.agent_id.clone(), job);
    }

    fn attach_handle(&self, agent_id: &str, handle: tokio::task::JoinHandle<()>) {
        if let Some(job) = self.state.lock().active.get_mut(agent_id) {
            job.handle = Some(handle);
        }
    }

    fn cancel_agent(&self, agent_id: &str) -> Option<crate::agent_runtime::AgentTaskRef> {
        let state = self.state.lock();
        let job = state.active.get(agent_id)?;
        job.cancellation_token.cancel();
        Some(job.task_ref.clone())
    }

    fn complete(&self, agent_id: &str) {
        self.state.lock().active.remove(agent_id);
    }

    fn task_ref(&self, agent_id: &str) -> Option<crate::agent_runtime::AgentTaskRef> {
        self.state.lock().known_tasks.get(agent_id).cloned()
    }

    fn forget(&self, agent_id: &str) {
        self.state.lock().known_tasks.remove(agent_id);
    }

    fn take_active_jobs(&self) -> Vec<BackgroundJob> {
        let mut state = self.state.lock();
        state.active.drain().map(|(_, job)| job).collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn register_agent_tree(
    agent_id: &str,
    parent_agent_id: Option<String>,
    description: &str,
    agent_type: Option<String>,
    agent_model: &str,
    current_depth: usize,
    chain_id: String,
    bg_tx: &allthecodes_types::agent_channel::AgentSender,
) {
    let node = allthecodes_types::agent_types::AgentNode {
        agent_id: agent_id.to_string(),
        parent_agent_id: parent_agent_id.clone(),
        description: description.to_string(),
        agent_type: agent_type.clone(),
        model: Some(agent_model.to_string()),
        state: "running".into(),
        is_background: true,
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

    let _ = bg_tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
        allthecodes_types::agent_events::AgentEvent::Spawned {
            agent_id: agent_id.to_string(),
            parent_agent_id,
            description: description.to_string(),
            agent_type,
            model: Some(agent_model.to_string()),
            is_background: true,
            depth: current_depth + 1,
            chain_id,
        },
    ));

    let roots = crate::agent_runtime::agent_tree_snapshot();
    let _ = bg_tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
        allthecodes_types::agent_events::AgentEvent::TreeSnapshot { roots },
    ));
}

#[expect(
    clippy::too_many_arguments,
    reason = "runtime setup threads agent identity and hook context together"
)]
async fn prepare_runtime(
    use_worktree: bool,
    agent_id: &str,
    description: &str,
    agent_model: &str,
    current_depth: usize,
    parent_agent_id: Option<&str>,
    session_id: &str,
    hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    hooks: allthecodes_types::hooks::HooksMap,
    delegated_cwd: &str,
    delegated_worktree_slug: Option<&str>,
) -> Result<PreparedRuntime> {
    let canonical_cwd = std::fs::canonicalize(delegated_cwd)
        .map_err(|err| anyhow::anyhow!("invalid delegated cwd '{}': {err}", delegated_cwd))?;
    let canonical_cwd = canonical_cwd.to_string_lossy().to_string();
    if !use_worktree {
        return Ok(PreparedRuntime {
            child_cwd: canonical_cwd,
            worktree: None,
            startup_warning: None,
        });
    }

    match prepare_worktree_runtime(
        agent_id,
        description,
        agent_model,
        current_depth,
        parent_agent_id,
        session_id,
        hook_runner,
        hooks,
        &canonical_cwd,
        delegated_worktree_slug,
    )
    .await
    {
        Ok(runtime) => Ok(runtime),
        Err(err) => {
            if delegated_worktree_slug.is_some() || !worktree_fallback_enabled() {
                let message = format!(
                    "background worktree isolation required but setup failed: {err}. Set ALLTHECODES_ALLOW_WORKTREE_FALLBACK=true to run without isolation."
                );
                warn!(
                    agent_id = %agent_id,
                    error = %err,
                    "background worktree isolation failed; fallback disabled"
                );
                let _ = crate::agent_runtime::emit_subagent_event(
                    "error",
                    agent_id,
                    parent_agent_id,
                    Some(description),
                    Some(agent_model),
                    current_depth + 1,
                    true,
                    Some(json!({ "message": message })),
                );
                return Err(anyhow::anyhow!(message));
            }

            warn!(
                agent_id = %agent_id,
                error = %err,
                "background worktree isolation failed; falling back to normal cwd"
            );
            let warning = format!("worktree isolation skipped: {}", err);
            let _ = crate::agent_runtime::emit_subagent_event(
                "warning",
                agent_id,
                parent_agent_id,
                Some(description),
                Some(agent_model),
                current_depth + 1,
                true,
                Some(json!({ "message": warning })),
            );
            Ok(PreparedRuntime {
                child_cwd: canonical_cwd,
                worktree: None,
                startup_warning: Some(warning),
            })
        }
    }
}

fn worktree_fallback_enabled() -> bool {
    std::env::var("ALLTHECODES_ALLOW_WORKTREE_FALLBACK")
        .or_else(|_| std::env::var("CC_RUST_ALLOW_WORKTREE_FALLBACK"))
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
async fn prepare_worktree_runtime(
    agent_id: &str,
    description: &str,
    agent_model: &str,
    current_depth: usize,
    parent_agent_id: Option<&str>,
    session_id: &str,
    hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    hooks: allthecodes_types::hooks::HooksMap,
    delegated_cwd: &str,
    delegated_worktree_slug: Option<&str>,
) -> Result<PreparedRuntime> {
    let cwd = PathBuf::from(delegated_cwd);
    let git_root = find_git_root(&cwd).await?;
    let original_head = get_head_sha(&git_root).await;
    let generated_slug = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let slug = delegated_worktree_slug.unwrap_or(&generated_slug);
    validate_worktree_slug(slug)?;
    let branch_name = format!("agent-worktree-{slug}");
    let worktree_path = default_agent_worktree_path(slug);
    if worktree_path.exists() {
        anyhow::bail!(
            "worktree path collision for slug '{slug}': {}",
            worktree_path.display()
        );
    }
    let branch_exists = tokio::process::Command::new("git")
        .args([
            "-C",
            &git_root.to_string_lossy(),
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch_name}"),
        ])
        .status()
        .await?
        .success();
    if branch_exists {
        anyhow::bail!("worktree branch collision for slug '{slug}': {branch_name}");
    }

    info!(
        agent_id = %agent_id,
        worktree_path = %worktree_path.display(),
        branch = %branch_name,
        "creating background agent worktree"
    );

    let hook_created = match run_worktree_create_hook(
        &hook_runner,
        &hooks,
        "BackgroundAgent",
        &git_root,
        &worktree_path,
        &branch_name,
        slug,
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
        Err(err) => {
            warn!(
                agent_id = %agent_id,
                error = %err,
                "WorktreeCreate hook failed; falling back to git"
            );
            None
        }
    };

    let (worktree_path, branch_name, created_by_hook) = if let Some(created) = hook_created {
        if delegated_worktree_slug.is_some()
            && (created.worktree_path != worktree_path || created.branch_name != branch_name)
        {
            anyhow::bail!(
                "WorktreeCreate hook did not honor delegated slug '{slug}' (path {}, branch {})",
                created.worktree_path.display(),
                created.branch_name
            );
        }
        (created.worktree_path, created.branch_name, true)
    } else {
        ensure_worktree_parent(&worktree_path)?;

        let output = tokio::process::Command::new("git")
            .args([
                "-C",
                &git_root.to_string_lossy(),
                "worktree",
                "add",
                "-b",
                &branch_name,
                &worktree_path.to_string_lossy(),
            ])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        (worktree_path, branch_name, false)
    };

    persist_agent_worktree_session_record(
        session_id,
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

    let _ = crate::agent_runtime::emit_subagent_event(
        "worktree_created",
        agent_id,
        parent_agent_id,
        Some(description),
        Some(agent_model),
        current_depth + 1,
        true,
        Some(json!({
            "worktree_path": worktree_path.display().to_string(),
            "branch": branch_name,
        })),
    );

    Ok(PreparedRuntime {
        child_cwd: worktree_path.to_string_lossy().to_string(),
        worktree: Some(WorktreeRuntime {
            session_id: session_id.to_string(),
            git_root,
            worktree_path,
            branch_name,
            original_head,
            hook_runner,
            hooks,
        }),
        startup_warning: None,
    })
}

fn validate_worktree_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || slug.len() > 64
        || slug.contains("..")
        || !slug
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        anyhow::bail!("invalid delegated worktree slug '{slug}'");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_worktree_outcome(
    result_text: &mut String,
    agent_id: &str,
    task_id: &str,
    worktree: &WorktreeRuntime,
    parent_agent_id: Option<&str>,
    description: &str,
    agent_model: &str,
    depth: usize,
) {
    let changes =
        count_worktree_changes(&worktree.worktree_path, worktree.original_head.as_deref()).await;
    let has_changes = match changes {
        Some((files, commits)) => files > 0 || commits > 0,
        None => true,
    };

    if has_changes {
        let (files, commits) = changes.unwrap_or((0, 0));
        mark_agent_worktree_session_kept(&worktree.session_id, agent_id, &worktree.worktree_path);
        let _ = crate::agent_runtime::emit_subagent_event(
            "worktree_kept",
            agent_id,
            parent_agent_id,
            Some(description),
            Some(agent_model),
            depth,
            true,
            Some(json!({
                "task_id": task_id,
                "files": files,
                "commits": commits,
                "worktree_path": worktree.worktree_path.display().to_string(),
                "branch": worktree.branch_name,
            })),
        );
        result_text.push_str(&format!(
            "\n\n[Worktree isolation: changes detected ({} file(s), {} commit(s)). Worktree kept at: {} on branch: {}]",
            files,
            commits,
            worktree.worktree_path.display(),
            worktree.branch_name
        ));
    } else {
        let _ = crate::agent_runtime::emit_subagent_event(
            "worktree_cleaned",
            agent_id,
            parent_agent_id,
            Some(description),
            Some(agent_model),
            depth,
            true,
            Some(json!({
                "task_id": task_id,
                "worktree_path": worktree.worktree_path.display().to_string(),
                "branch": worktree.branch_name,
            })),
        );
        let cleaned = AgentTool::cleanup_worktree_with_hooks(
            &worktree.git_root,
            &worktree.worktree_path,
            &worktree.branch_name,
            agent_id,
            Some(&worktree.hook_runner),
            Some(&worktree.hooks),
        )
        .await;
        if cleaned {
            mark_agent_worktree_session_removed(
                &worktree.session_id,
                agent_id,
                &worktree.worktree_path,
            );
            result_text
                .push_str("\n\n[Worktree isolation: no changes detected; worktree cleaned up]");
        } else {
            mark_agent_worktree_session_cleanup_failed(
                &worktree.session_id,
                agent_id,
                &worktree.worktree_path,
            );
            result_text.push_str(&format!(
                "\n\n[Worktree isolation: no changes detected, but cleanup could not be verified. Worktree kept at: {} on branch: {}]",
                worktree.worktree_path.display(),
                worktree.branch_name
            ));
        }
    }
}

async fn finalize_or_keep_worktree_after_forced_shutdown(
    agent_id: &str,
    task_ref: &crate::agent_runtime::AgentTaskRef,
    worktree: WorktreeRuntime,
) {
    let changes =
        count_worktree_changes(&worktree.worktree_path, worktree.original_head.as_deref()).await;
    let has_changes = match changes {
        Some((files, commits)) => files > 0 || commits > 0,
        None => true,
    };

    if has_changes {
        mark_agent_worktree_session_kept(&worktree.session_id, agent_id, &worktree.worktree_path);
        let suffix = format!(
            "[Supervisor: shutdown kept worktree at {} on branch {} because changes may exist]",
            worktree.worktree_path.display(),
            worktree.branch_name
        );
        let _ = crate::agent_runtime::global_task_store().append_output(task_ref, &suffix);
    } else {
        let cleaned = AgentTool::cleanup_worktree_with_hooks(
            &worktree.git_root,
            &worktree.worktree_path,
            &worktree.branch_name,
            agent_id,
            Some(&worktree.hook_runner),
            Some(&worktree.hooks),
        )
        .await;
        let suffix = if cleaned {
            mark_agent_worktree_session_removed(
                &worktree.session_id,
                agent_id,
                &worktree.worktree_path,
            );
            "[Supervisor: shutdown cleaned an unchanged worktree]".to_string()
        } else {
            mark_agent_worktree_session_cleanup_failed(
                &worktree.session_id,
                agent_id,
                &worktree.worktree_path,
            );
            format!(
                "[Supervisor: shutdown kept unchanged worktree at {} on branch {} because cleanup could not be verified]",
                worktree.worktree_path.display(),
                worktree.branch_name
            )
        };
        let _ = crate::agent_runtime::global_task_store().append_output(task_ref, &suffix);
    }
}

async fn cleanup_failed_launch_worktree(agent_id: &str, worktree: WorktreeRuntime) {
    let cleaned = AgentTool::cleanup_worktree_with_hooks(
        &worktree.git_root,
        &worktree.worktree_path,
        &worktree.branch_name,
        agent_id,
        Some(&worktree.hook_runner),
        Some(&worktree.hooks),
    )
    .await;
    if cleaned {
        mark_agent_worktree_session_removed(
            &worktree.session_id,
            agent_id,
            &worktree.worktree_path,
        );
    } else {
        mark_agent_worktree_session_cleanup_failed(
            &worktree.session_id,
            agent_id,
            &worktree.worktree_path,
        );
        warn!(
            agent_id,
            worktree_path = %worktree.worktree_path.display(),
            "failed delegated launch left worktree in place for manual recovery"
        );
    }
}

fn preview(result_text: &str) -> String {
    if result_text.len() > 200 {
        let end = result_text.floor_char_boundary(200);
        format!("{}...", &result_text[..end])
    } else {
        result_text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct CurrentDirGuard {
        previous: std::path::PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).unwrap();
        }
    }

    #[test]
    fn preview_respects_char_boundary() {
        let text = format!("{}{}", "a".repeat(199), "é".repeat(10));
        let preview = preview(&text);
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 203);
    }

    #[test]
    fn cancel_missing_agent_returns_none() {
        assert!(BACKGROUND_SUPERVISOR
            .cancel_agent("missing-agent")
            .is_none());
    }

    #[test]
    fn register_and_cancel_agent_returns_task_id() {
        let token = CancellationToken::new();
        let agent_id = format!("agent-{}", uuid::Uuid::new_v4());
        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        BACKGROUND_SUPERVISOR.register(BackgroundJob {
            agent_id: agent_id.clone(),
            task_ref: crate::agent_runtime::AgentTaskRef {
                task_list_id: DEFAULT_TASK_LIST_ID.to_string(),
                task_id: task_id.clone(),
            },
            cancellation_token: token.clone(),
            handle: None,
            worktree: None,
        });

        let cancelled_task_id = BACKGROUND_SUPERVISOR.cancel_agent(&agent_id);
        BACKGROUND_SUPERVISOR.complete(&agent_id);

        assert_eq!(
            cancelled_task_id.map(|task_ref| task_ref.task_id),
            Some(task_id)
        );
        assert!(token.is_cancelled());
    }

    #[test]
    #[serial_test::serial]
    fn worktree_fallback_requires_explicit_policy() {
        let _fallback = EnvGuard::remove("ALLTHECODES_ALLOW_WORKTREE_FALLBACK");
        assert!(!worktree_fallback_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn worktree_fallback_policy_accepts_true() {
        let _fallback = EnvGuard::set("ALLTHECODES_ALLOW_WORKTREE_FALLBACK", "true");
        assert!(worktree_fallback_enabled());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn worktree_setup_failure_is_visible_when_fallback_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd = CurrentDirGuard::set(tmp.path());
        let _fallback = EnvGuard::remove("ALLTHECODES_ALLOW_WORKTREE_FALLBACK");

        let err = match prepare_runtime(
            true,
            "agent-1",
            "test worktree",
            "test-model",
            0,
            None,
            "test-session",
            Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            allthecodes_types::hooks::HooksMap::default(),
            tmp.path().to_str().unwrap(),
            None,
        )
        .await
        {
            Ok(_) => panic!("worktree setup failure should be visible without fallback"),
            Err(err) => err,
        };
        let message = err.to_string();

        assert!(message.contains("background worktree isolation required but setup failed"));
        assert!(message.contains("ALLTHECODES_ALLOW_WORKTREE_FALLBACK=true"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn worktree_setup_failure_fallback_returns_visible_warning_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let _cwd = CurrentDirGuard::set(tmp.path());
        let _fallback = EnvGuard::set("ALLTHECODES_ALLOW_WORKTREE_FALLBACK", "true");

        let runtime = prepare_runtime(
            true,
            "agent-1",
            "test worktree",
            "test-model",
            0,
            None,
            "test-session",
            Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            allthecodes_types::hooks::HooksMap::default(),
            tmp.path().to_str().unwrap(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(runtime.child_cwd, tmp.path().display().to_string());
        assert!(runtime.worktree.is_none());
        assert!(runtime
            .startup_warning
            .as_deref()
            .unwrap_or_default()
            .contains("worktree isolation skipped"));
    }
}
