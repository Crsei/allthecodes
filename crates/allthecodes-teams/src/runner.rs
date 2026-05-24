//! In-process teammate execution loop.
//!
//! Corresponds to TypeScript: `utils/swarm/inProcessRunner.ts`
//!
//! Runs a teammate's QueryEngine inside a `task_local!` scope,
//! processing messages from the mailbox and handling protocol messages.

use std::time::Duration;

use anyhow::Result;
use tokio::time;
use tracing::{debug, info, warn};

use super::context;
use super::in_process::InProcessBackend;
use super::mailbox;
use super::protocol::{self, ProtocolMessage};
use super::types::*;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::{AgentContext, QueryEngineConfig, QuerySource};
use allthecodes_engine::types::tool::QueryChainTracking;
use allthecodes_tools::registry::ToolPolicy;
use allthecodes_tools::tool::PermissionMode;

// ---------------------------------------------------------------------------
// Spawn entry point
// ---------------------------------------------------------------------------

/// Configuration for starting an in-process teammate runner.
pub struct InProcessRunnerConfig {
    pub identity: TeammateIdentity,
    pub task_id: String,
    pub prompt: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub system_prompt_mode: Option<SystemPromptMode>,
    pub cwd: String,
    pub cancellation: tokio_util::sync::CancellationToken,
}

/// Spawn a teammate runner as a background tokio task.
///
/// Corresponds to TS: `startInProcessTeammate(config)`
///
/// The returned `JoinHandle` can be used for monitoring; the
/// `CancellationToken` controls the runner's lifecycle.
pub fn start_runner(config: InProcessRunnerConfig) -> tokio::task::JoinHandle<()> {
    let agent_id = config.identity.agent_id.clone();
    let task_id = config.task_id.clone();
    let identity = config.identity.clone();

    let handle = tokio::spawn(async move {
        if let Err(e) = run_teammate(config).await {
            warn!(agent_id = %agent_id, error = %e, "teammate runner exited with error");
            release_teammate_tasks(
                &identity,
                allthecodes_tasks::TeammateTaskExitReason::Terminated,
            );
            InProcessBackend::mark_task_failed(&task_id, e.to_string());
        }
    });

    handle
}

// ---------------------------------------------------------------------------
// Main execution loop
// ---------------------------------------------------------------------------

/// Run the teammate execution loop within a task_local context.
///
/// Corresponds to TS: `runInProcessTeammate(config)`
///
/// Loop:
/// 1. Submit prompt to QueryEngine
/// 2. Poll mailbox for new messages
/// 3. Handle protocol messages (shutdown, permissions, plan approval)
/// 4. Mark idle when waiting
/// 5. Exit on cancellation or shutdown approval
async fn run_teammate(config: InProcessRunnerConfig) -> Result<()> {
    let identity = config.identity.clone();
    let task_id = config.task_id.clone();
    let agent_name = identity.agent_name.clone();
    let team_name = identity.team_name.clone();
    let cancellation = config.cancellation.clone();

    // Run everything inside the teammate context scope
    context::run_in_scope(identity.clone(), async move {
        info!(
            agent_id = %identity.agent_id,
            task_id = %task_id,
            "teammate runner started"
        );

        // Build a child QueryEngine
        let child_tools = tools_for_teammate(tool_policy_for_teammate(
            config.agent_type.as_deref(),
        ));
        let (custom_system_prompt, append_system_prompt) =
            teammate_system_prompt_parts(config.system_prompt.clone(), config.system_prompt_mode);
        let engine_config = QueryEngineConfig {
            cwd: config.cwd.clone(),
            tools: child_tools,
            custom_system_prompt,
            append_system_prompt,
            user_specified_model: config.model.clone(),
            fallback_model: None,
            max_turns: Some(100),
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: Some(AgentContext {
                agent_id: identity.agent_id.clone(),
                query_tracking: QueryChainTracking {
                    chain_id: task_id.clone(),
                    depth: 1,
                },
                langfuse_session_id: identity.parent_session_id.clone(),
                agent_type: config.agent_type.clone(),
                team_context: Some(allthecodes_types::teams::TeamContext {
                    team_name: identity.team_name.clone(),
                    lead_agent_id: crate::identity::lead_agent_id(&identity.team_name),
                    self_agent_id: Some(identity.agent_id.clone()),
                    self_agent_name: Some(identity.agent_name.clone()),
                    is_leader: Some(false),
                    self_agent_color: identity.color.clone(),
                    ..Default::default()
                }),
                tool_permission_context: None,
            }),
        };

        let mut engine = QueryEngine::new(engine_config);
        engine.set_hook_runner(std::sync::Arc::new(
            allthecodes_tools::hooks::ShellHookRunner::new(),
        ));
        engine.set_command_dispatcher(std::sync::Arc::new(
            allthecodes_commands::DefaultCommandDispatcher::new(
                allthecodes_commands::runtime::command_metadata_snapshot(),
            ),
        ));

        let mut next_prompt = Some(config.prompt.clone());

        loop {
            if let Some(prompt) = next_prompt.take() {
                let should_stop = drive_engine_turn(
                    &engine,
                    &prompt,
                    &identity,
                    &agent_name,
                    &team_name,
                    &task_id,
                    &cancellation,
                )
                .await?;
                if should_stop {
                    InProcessBackend::update_task_status(&task_id, TaskStatus::Stopped);
                    release_teammate_tasks(
                        &identity,
                        allthecodes_tasks::TeammateTaskExitReason::Shutdown,
                    );
                    break;
                }
            }

            let queued = InProcessBackend::take_pending_user_messages(&task_id);
            if !queued.is_empty() {
                next_prompt = Some(format_pending_messages(&queued));
                continue;
            }

            InProcessBackend::set_task_idle(&task_id, true);
            let mut poll_interval = time::interval(Duration::from_millis(
                super::constants::MAILBOX_POLL_INTERVAL_MS,
            ));

            loop {
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        info!(agent_id = %identity.agent_id, "cancellation received");
                        InProcessBackend::update_task_status(&task_id, TaskStatus::Stopped);
                        release_teammate_tasks(
                            &identity,
                            allthecodes_tasks::TeammateTaskExitReason::Terminated,
                        );
                        return Ok(());
                    }

                    _ = poll_interval.tick() => {
                        match process_mailbox(
                            &agent_name,
                            &team_name,
                            &identity.agent_id,
                            &task_id,
                        ) {
                            Ok(actions) => {
                                for message in actions.plain_messages {
                                    InProcessBackend::push_pending_user_message(&task_id, message);
                                }
                                if actions.shutdown_requested {
                                    InProcessBackend::update_task_status(&task_id, TaskStatus::Stopped);
                                    release_teammate_tasks(
                                        &identity,
                                        allthecodes_tasks::TeammateTaskExitReason::Shutdown,
                                    );
                                    return Ok(());
                                }
                                let queued = InProcessBackend::take_pending_user_messages(&task_id);
                                if !queued.is_empty() {
                                    next_prompt = Some(format_pending_messages(&queued));
                                    break;
                                }
                            }
                            Err(e) => {
                                let error = mark_mailbox_processing_failure(&task_id, e);
                                release_teammate_tasks(
                                    &identity,
                                    allthecodes_tasks::TeammateTaskExitReason::Terminated,
                                );
                                return Err(anyhow::anyhow!(error));
                            }
                        }
                    }
                }
            }
        }

        info!(agent_id = %identity.agent_id, "teammate runner finished");
        release_teammate_tasks(
            &identity,
            allthecodes_tasks::TeammateTaskExitReason::Shutdown,
        );
        Ok(())
    })
    .await
}

fn release_teammate_tasks(
    identity: &TeammateIdentity,
    reason: allthecodes_tasks::TeammateTaskExitReason,
) -> allthecodes_tasks::UnassignTeammateTasksResult {
    let result = allthecodes_engine::agent_runtime::unassign_teammate_tasks(
        &identity.team_name,
        &identity.agent_id,
        &identity.agent_name,
        reason,
    );
    if !result.unassigned_tasks.is_empty() {
        info!(
            agent_id = %identity.agent_id,
            team_name = %identity.team_name,
            unassigned = result.unassigned_tasks.len(),
            message = %result.notification_message,
            "teammate tasks unassigned"
        );
    }
    result
}

fn tool_policy_for_teammate(agent_type: Option<&str>) -> ToolPolicy {
    match agent_type.map(|value| value.trim()) {
        Some(agent_type) if agent_type.eq_ignore_ascii_case("worker") => {
            ToolPolicy::CoordinatorWorker
        }
        _ => ToolPolicy::InProcessTeammate,
    }
}

fn tools_for_teammate(policy: ToolPolicy) -> allthecodes_tools::tool::Tools {
    allthecodes_engine::agent_runtime::all_tools()
        .into_iter()
        .filter(|tool| allthecodes_tools::registry::tool_allowed(policy, tool.name()))
        .collect()
}

fn teammate_system_prompt_parts(
    system_prompt: Option<String>,
    mode: Option<SystemPromptMode>,
) -> (Option<String>, Option<String>) {
    let Some(prompt) = system_prompt.filter(|value| !value.trim().is_empty()) else {
        return (None, None);
    };

    match mode.unwrap_or(SystemPromptMode::Append) {
        SystemPromptMode::Replace => (Some(prompt), None),
        SystemPromptMode::Append | SystemPromptMode::Default => (None, Some(prompt)),
    }
}

async fn drive_engine_turn(
    engine: &QueryEngine,
    prompt: &str,
    identity: &TeammateIdentity,
    agent_name: &str,
    team_name: &str,
    task_id: &str,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<bool> {
    use allthecodes_types::sdk::SdkMessage;
    use futures::StreamExt;

    InProcessBackend::set_task_idle(task_id, false);

    let stream = engine.submit_message(prompt, QuerySource::Agent(identity.agent_id.clone()));
    let mut stream = std::pin::pin!(stream);
    let mut poll_interval = time::interval(Duration::from_millis(
        super::constants::MAILBOX_POLL_INTERVAL_MS,
    ));

    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                info!(agent_id = %identity.agent_id, "cancellation received");
                return Ok(true);
            }

            msg = stream.next() => {
                match msg {
                    Some(SdkMessage::Result(_)) => {
                        InProcessBackend::set_task_idle(task_id, true);
                        debug!(agent_id = %identity.agent_id, "query completed, marking idle");

                        let _ = send_idle_notification(
                            agent_name,
                            team_name,
                            IdleReason::Available,
                            None,
                        );
                        return Ok(false);
                    }
                    Some(_) => {
                        InProcessBackend::set_task_idle(task_id, false);
                    }
                    None => {
                        InProcessBackend::set_task_idle(task_id, true);
                        return Ok(false);
                    }
                }
            }

            _ = poll_interval.tick() => {
                let actions = match process_mailbox(
                    agent_name,
                    team_name,
                    &identity.agent_id,
                    task_id,
                ) {
                    Ok(actions) => actions,
                    Err(e) => {
                        return Err(anyhow::anyhow!(mark_mailbox_processing_failure(task_id, e)));
                    }
                };
                for message in actions.plain_messages {
                    InProcessBackend::push_pending_user_message(task_id, message);
                }
                if actions.shutdown_requested {
                    return Ok(true);
                }
            }
        }
    }
}

fn format_pending_messages(messages: &[String]) -> String {
    if messages.len() == 1 {
        return messages[0].clone();
    }

    let mut prompt = String::from("Team mailbox messages:\n");
    for message in messages {
        prompt.push_str("- ");
        prompt.push_str(message);
        prompt.push('\n');
    }
    prompt
}

// ---------------------------------------------------------------------------
// Mailbox processing
// ---------------------------------------------------------------------------

/// Process unread mailbox messages for this teammate.
#[derive(Default)]
struct MailboxActions {
    shutdown_requested: bool,
    plain_messages: Vec<String>,
}

fn process_mailbox(
    agent_name: &str,
    team_name: &str,
    agent_id: &str,
    task_id: &str,
) -> Result<MailboxActions> {
    let messages = mailbox::read_unread_messages(agent_name, team_name)?;
    let mut actions = MailboxActions::default();

    for msg in &messages {
        // Try to parse as protocol message
        if let Some(proto) = protocol::try_parse_protocol_message(&msg.text) {
            if handle_protocol_message(proto, agent_name, team_name, agent_id, task_id)? {
                actions.shutdown_requested = true;
            }
        } else {
            debug!(
                from = %msg.from,
                "received plain text message from teammate"
            );
            actions
                .plain_messages
                .push(format!("Message from {}: {}", msg.from, msg.text));
        }
    }

    // Mark all as read
    if !messages.is_empty() {
        mailbox::mark_all_as_read(agent_name, team_name)?;
    }

    Ok(actions)
}

fn mark_mailbox_processing_failure(task_id: &str, error: impl std::fmt::Display) -> String {
    let error = format!("mailbox coordination failure: {error}");
    InProcessBackend::mark_task_failed(task_id, error.clone());
    error
}

/// Handle a structured protocol message.
fn handle_protocol_message(
    msg: ProtocolMessage,
    agent_name: &str,
    team_name: &str,
    agent_id: &str,
    task_id: &str,
) -> Result<bool> {
    match msg {
        ProtocolMessage::ShutdownRequest {
            request_id, reason, ..
        } => {
            info!(
                agent_id,
                request_id = %request_id,
                reason = ?reason,
                "received shutdown request"
            );

            if !shutdown_auto_approval_enabled() {
                let error =
                    format!("shutdown request {request_id} requires explicit auto-approval policy");
                warn!(
                    agent_id,
                    request_id = %request_id,
                    "shutdown request not auto-approved"
                );
                InProcessBackend::mark_task_failed(task_id, error.clone());
                send_shutdown_rejected(agent_name, team_name, &request_id, &error)?;
                return Ok(false);
            }

            let now = chrono::Utc::now();
            let approval = serde_json::json!({
                "type": "shutdown_approved",
                "requestId": request_id,
                "from": agent_name,
                "timestamp": now.to_rfc3339(),
                "backendType": "in-process",
            });

            let response = TeammateMessage {
                from: agent_name.into(),
                text: approval.to_string(),
                timestamp: now.to_rfc3339(),
                read: false,
                color: None,
                summary: Some("Shutdown approved".into()),
            };

            // Write approval to leader's mailbox
            mailbox::write_to_mailbox(super::constants::TEAM_LEAD_NAME, response, team_name)?;

            // Mark task as stopped
            InProcessBackend::update_task_status(task_id, TaskStatus::Stopped);
            InProcessBackend::request_shutdown(task_id);
            return Ok(true);
        }

        ProtocolMessage::PlanApprovalResponse {
            approved,
            feedback,
            permission_mode,
            ..
        } => {
            debug!(approved, feedback = ?feedback, "plan approval response received");
            InProcessBackend::set_plan_approval_pending(task_id, false);
            if let Some(mode) = permission_mode.as_deref() {
                InProcessBackend::set_permission_mode(task_id, PermissionMode::parse(mode));
            }
            let message = if approved {
                "Plan approved by team leader. Exit plan mode and proceed according to the approved plan."
                    .to_string()
            } else {
                format!(
                    "Plan rejected by team leader. Revise the plan before implementation.{}",
                    feedback
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| format!("\nFeedback: {text}"))
                        .unwrap_or_default()
                )
            };
            InProcessBackend::push_pending_user_message(task_id, message);
        }

        ProtocolMessage::PermissionResponse {
            request_id,
            subtype,
            ..
        } => {
            debug!(request_id = %request_id, subtype = %subtype, "permission response received");
            // Would unblock the permission request
        }

        ProtocolMessage::ModeSetRequest { mode, .. } => {
            debug!(mode = %mode, "mode set request received");
            // Would update the permission mode
        }

        other => {
            debug!(?other, "unhandled protocol message");
        }
    }

    Ok(false)
}

fn shutdown_auto_approval_enabled() -> bool {
    std::env::var("ALLTHECODES_AUTO_APPROVE_SHUTDOWN_REQUESTS")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

fn send_shutdown_rejected(
    agent_name: &str,
    team_name: &str,
    request_id: &str,
    reason: &str,
) -> Result<()> {
    let now = chrono::Utc::now();
    let rejection = serde_json::json!({
        "type": "shutdown_rejected",
        "requestId": request_id,
        "from": agent_name,
        "timestamp": now.to_rfc3339(),
        "backendType": "in-process",
        "reason": reason,
    });

    mailbox::write_to_mailbox(
        super::constants::TEAM_LEAD_NAME,
        TeammateMessage {
            from: agent_name.into(),
            text: rejection.to_string(),
            timestamp: now.to_rfc3339(),
            read: false,
            color: None,
            summary: Some("Shutdown auto-approval disabled".into()),
        },
        team_name,
    )
}

// ---------------------------------------------------------------------------
// Notification helpers
// ---------------------------------------------------------------------------

/// Send an idle notification to the team leader.
fn send_idle_notification(
    agent_name: &str,
    team_name: &str,
    reason: IdleReason,
    summary: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now();
    let notification = serde_json::json!({
        "type": "idle_notification",
        "from": agent_name,
        "timestamp": now.to_rfc3339(),
        "idleReason": reason,
        "summary": summary,
    });

    let message = TeammateMessage {
        from: agent_name.into(),
        text: notification.to_string(),
        timestamp: now.to_rfc3339(),
        read: false,
        color: None,
        summary: summary.map(|s| s.to_string()),
    };

    mailbox::write_to_mailbox(super::constants::TEAM_LEAD_NAME, message, team_name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn test_runner_config_creation() {
        let config = InProcessRunnerConfig {
            identity: TeammateIdentity {
                agent_id: "worker@team".into(),
                agent_name: "worker".into(),
                team_name: "team".into(),
                color: Some("red".into()),
                plan_mode_required: false,
                parent_session_id: "sess-1".into(),
            },
            task_id: "task-1".into(),
            prompt: "Do work".into(),
            agent_type: Some("worker".into()),
            model: None,
            system_prompt: None,
            system_prompt_mode: None,
            cwd: "/tmp".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        assert_eq!(config.identity.agent_id, "worker@team");
        assert_eq!(config.prompt, "Do work");
    }

    #[test]
    fn test_idle_reason_serialize() {
        let json = serde_json::to_string(&IdleReason::Available).unwrap();
        assert_eq!(json, "\"available\"");
    }

    #[test]
    fn pending_messages_are_formatted_for_next_turn() {
        assert_eq!(
            format_pending_messages(&["Message from lead: continue".into()]),
            "Message from lead: continue"
        );
        let formatted = format_pending_messages(&[
            "Message from lead: first".into(),
            "Message from reviewer: second".into(),
        ]);
        assert!(formatted.starts_with("Team mailbox messages:"));
        assert!(formatted.contains("- Message from lead: first"));
        assert!(formatted.contains("- Message from reviewer: second"));
    }

    #[test]
    fn teammate_tool_policy_uses_worker_boundary_for_worker_agent_type() {
        assert_eq!(
            tool_policy_for_teammate(Some("worker")),
            ToolPolicy::CoordinatorWorker
        );
        assert_eq!(
            tool_policy_for_teammate(Some("teammate")),
            ToolPolicy::InProcessTeammate
        );
        assert_eq!(
            tool_policy_for_teammate(None),
            ToolPolicy::InProcessTeammate
        );
    }

    #[test]
    fn teammate_system_prompt_parts_respects_prompt_mode() {
        assert_eq!(
            teammate_system_prompt_parts(
                Some("worker prompt".into()),
                Some(SystemPromptMode::Append)
            ),
            (None, Some("worker prompt".into()))
        );
        assert_eq!(
            teammate_system_prompt_parts(
                Some("worker prompt".into()),
                Some(SystemPromptMode::Replace)
            ),
            (Some("worker prompt".into()), None)
        );
        assert_eq!(teammate_system_prompt_parts(None, None), (None, None));
    }

    #[test]
    #[serial_test::serial]
    fn process_mailbox_collects_plain_messages_and_marks_them_read() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        mailbox::write_to_mailbox(
            "worker",
            TeammateMessage {
                from: crate::constants::TEAM_LEAD_NAME.into(),
                text: "continue with tests".into(),
                timestamp: "2026-05-06T00:00:00Z".into(),
                read: false,
                color: None,
                summary: None,
            },
            "phase0",
        )
        .unwrap();

        let actions = process_mailbox("worker", "phase0", "worker@phase0", "task-1").unwrap();

        assert!(!actions.shutdown_requested);
        assert_eq!(
            actions.plain_messages,
            vec![format!(
                "Message from {}: continue with tests",
                crate::constants::TEAM_LEAD_NAME
            )]
        );
        let inbox = mailbox::read_mailbox("worker", "phase0").unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(inbox[0].read);
    }

    #[test]
    #[serial_test::serial]
    fn shutdown_request_requires_explicit_auto_approval_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let _approval = EnvGuard::remove("ALLTHECODES_AUTO_APPROVE_SHUTDOWN_REQUESTS");
        InProcessBackend::clear_registry();
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: "task-1".into(),
            status: TaskStatus::Running,
            identity: TeammateIdentity {
                agent_id: "worker@phase0".into(),
                agent_name: "worker".into(),
                team_name: "phase0".into(),
                color: None,
                plan_mode_required: false,
                parent_session_id: "session".into(),
            },
            prompt: "initial".into(),
            model: None,
            abort_handle: None,
            cancellation_token: None,
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Default,
            error: None,
            pending_user_messages: vec![],
            is_idle: false,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });
        let raw = serde_json::json!({
            "type": "shutdown_request",
            "requestId": "shutdown-worker-1",
            "from": crate::constants::TEAM_LEAD_NAME,
            "reason": "phase0 test",
            "timestamp": "2026-05-06T00:00:00Z",
        })
        .to_string();
        mailbox::write_to_mailbox(
            "worker",
            TeammateMessage {
                from: crate::constants::TEAM_LEAD_NAME.into(),
                text: raw,
                timestamp: "2026-05-06T00:00:00Z".into(),
                read: false,
                color: None,
                summary: Some("shutdown".into()),
            },
            "phase0",
        )
        .unwrap();

        let actions = process_mailbox("worker", "phase0", "worker@phase0", "task-1").unwrap();

        assert!(!actions.shutdown_requested);
        let snapshot = InProcessBackend::task_snapshots().remove(0);
        assert_eq!(snapshot.status, TaskStatus::Stopped);
        assert!(snapshot
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("requires explicit auto-approval policy"));
        let leader_inbox =
            mailbox::read_mailbox(crate::constants::TEAM_LEAD_NAME, "phase0").unwrap();
        assert_eq!(leader_inbox.len(), 1);
        assert_eq!(
            leader_inbox[0].summary.as_deref(),
            Some("Shutdown auto-approval disabled")
        );
        assert!(leader_inbox[0].text.contains("shutdown_rejected"));
        InProcessBackend::clear_registry();
    }

    #[test]
    #[serial_test::serial]
    fn shutdown_request_auto_approves_when_policy_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let _approval = EnvGuard::set("ALLTHECODES_AUTO_APPROVE_SHUTDOWN_REQUESTS", "true");
        InProcessBackend::clear_registry();
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: "task-1".into(),
            status: TaskStatus::Running,
            identity: TeammateIdentity {
                agent_id: "worker@phase0".into(),
                agent_name: "worker".into(),
                team_name: "phase0".into(),
                color: None,
                plan_mode_required: false,
                parent_session_id: "session".into(),
            },
            prompt: "initial".into(),
            model: None,
            abort_handle: None,
            cancellation_token: None,
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Default,
            error: None,
            pending_user_messages: vec![],
            is_idle: false,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });
        let raw = serde_json::json!({
            "type": "shutdown_request",
            "requestId": "shutdown-worker-1",
            "from": crate::constants::TEAM_LEAD_NAME,
            "reason": "phase0 test",
            "timestamp": "2026-05-06T00:00:00Z",
        })
        .to_string();
        mailbox::write_to_mailbox(
            "worker",
            TeammateMessage {
                from: crate::constants::TEAM_LEAD_NAME.into(),
                text: raw,
                timestamp: "2026-05-06T00:00:00Z".into(),
                read: false,
                color: None,
                summary: Some("shutdown".into()),
            },
            "phase0",
        )
        .unwrap();

        let actions = process_mailbox("worker", "phase0", "worker@phase0", "task-1").unwrap();

        assert!(actions.shutdown_requested);
        let snapshot = InProcessBackend::task_snapshots().remove(0);
        assert_eq!(snapshot.status, TaskStatus::Stopped);
        let leader_inbox =
            mailbox::read_mailbox(crate::constants::TEAM_LEAD_NAME, "phase0").unwrap();
        assert_eq!(leader_inbox.len(), 1);
        assert_eq!(
            leader_inbox[0].summary.as_deref(),
            Some("Shutdown approved")
        );
        assert!(leader_inbox[0].text.contains("shutdown_approved"));
        InProcessBackend::clear_registry();
    }

    #[test]
    #[serial_test::serial]
    fn mailbox_processing_error_marks_task_failed() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        InProcessBackend::clear_registry();
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: "task-1".into(),
            status: TaskStatus::Running,
            identity: TeammateIdentity {
                agent_id: "worker@phase0".into(),
                agent_name: "worker".into(),
                team_name: "phase0".into(),
                color: None,
                plan_mode_required: false,
                parent_session_id: "session".into(),
            },
            prompt: "initial".into(),
            model: None,
            abort_handle: None,
            cancellation_token: None,
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Default,
            error: None,
            pending_user_messages: vec![],
            is_idle: false,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });

        let inbox = mailbox::inbox_path("worker", "phase0");
        std::fs::create_dir_all(inbox.parent().unwrap()).unwrap();
        std::fs::write(&inbox, "{not valid json").unwrap();

        let err = match process_mailbox("worker", "phase0", "worker@phase0", "task-1") {
            Ok(_) => panic!("corrupt mailbox should fail"),
            Err(err) => err,
        };
        mark_mailbox_processing_failure("task-1", err);

        let snapshot = InProcessBackend::task_snapshots().remove(0);
        assert_eq!(snapshot.status, TaskStatus::Stopped);
        assert!(snapshot
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("mailbox coordination failure"));
        InProcessBackend::clear_registry();
    }

    #[test]
    #[serial_test::serial]
    fn plan_approval_response_updates_runner_state_and_queues_feedback() {
        InProcessBackend::clear_registry();
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: "task-1".into(),
            status: TaskStatus::Running,
            identity: TeammateIdentity {
                agent_id: "worker@team".into(),
                agent_name: "worker".into(),
                team_name: "team".into(),
                color: None,
                plan_mode_required: true,
                parent_session_id: "session".into(),
            },
            prompt: "initial".into(),
            model: None,
            abort_handle: None,
            cancellation_token: None,
            awaiting_plan_approval: true,
            permission_mode: PermissionMode::Plan,
            error: None,
            pending_user_messages: vec![],
            is_idle: true,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });

        let approved = ProtocolMessage::PlanApprovalResponse {
            request_id: "plan_approval-worker-1".into(),
            approved: true,
            feedback: None,
            timestamp: "2026-05-05T00:01:00Z".into(),
            permission_mode: Some("acceptEdits".into()),
        };

        let should_shutdown =
            handle_protocol_message(approved, "worker", "team", "worker@team", "task-1").unwrap();
        assert!(!should_shutdown);
        let snapshot = InProcessBackend::task_snapshots().remove(0);
        assert!(!snapshot.awaiting_plan_approval);
        assert_eq!(snapshot.permission_mode, PermissionMode::AcceptEdits);
        assert_eq!(
            InProcessBackend::take_pending_user_messages("task-1"),
            vec![
                "Plan approved by team leader. Exit plan mode and proceed according to the approved plan."
                    .to_string()
            ]
        );
        InProcessBackend::clear_registry();
    }
}
