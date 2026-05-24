//! In-process teammate execution backend.
//!
//! Corresponds to TypeScript: `utils/swarm/backends/InProcessBackend.ts`
//!
//! Teammates run as tokio tasks within the same process.
//! Uses `tokio::task_local!` for context isolation and
//! `CancellationToken` for lifecycle management.

use parking_lot::Mutex;
use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::backend::TeammateExecutor;
use super::constants::*;
use super::identity;
use super::mailbox;
use super::protocol;
use super::runner;
use super::types::*;

use allthecodes_tools::tool::PermissionMode;

// ---------------------------------------------------------------------------
// In-process task registry
// ---------------------------------------------------------------------------

/// Global registry of in-process teammate tasks.
///
/// Shared across the application to track and manage spawned teammates.
static TASK_REGISTRY: std::sync::LazyLock<Mutex<HashMap<String, InProcessTeammateTaskState>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// InProcessBackend
// ---------------------------------------------------------------------------

/// In-process execution backend — always available, no external dependencies.
pub struct InProcessBackend;

impl InProcessBackend {
    pub fn new() -> Self {
        Self
    }

    /// Look up a task by agent_id.
    fn find_task(agent_id: &str) -> Option<String> {
        let registry = TASK_REGISTRY.lock();
        registry
            .iter()
            .find(|(_, state)| state.identity.agent_id == agent_id)
            .map(|(id, _)| id.clone())
    }

    /// Get the number of running tasks.
    pub fn running_task_count() -> usize {
        let registry = TASK_REGISTRY.lock();
        registry
            .values()
            .filter(|s| s.status == TaskStatus::Running)
            .count()
    }

    /// Check if any in-process teammates are currently working (not idle).
    pub fn has_working_teammates() -> bool {
        let registry = TASK_REGISTRY.lock();
        registry
            .values()
            .any(|s| s.status == TaskStatus::Running && !s.is_idle)
    }

    /// Register a new task in the registry.
    pub fn register_task(state: InProcessTeammateTaskState) {
        let id = state.id.clone();
        let mut registry = TASK_REGISTRY.lock();
        registry.insert(id, state);
    }

    /// Update a task's status.
    pub fn update_task_status(task_id: &str, status: TaskStatus) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.status = status;
        }
    }

    /// Mark a task as idle or working.
    pub fn set_task_idle(task_id: &str, idle: bool) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.is_idle = idle;
        }
    }

    /// Mark whether a teammate is waiting for leader plan approval.
    pub fn set_plan_approval_pending(task_id: &str, pending: bool) -> bool {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.awaiting_plan_approval = pending;
            return true;
        }
        false
    }

    /// Mark whether a teammate is waiting for leader plan approval by name.
    pub fn set_plan_approval_pending_by_agent(
        agent_name: &str,
        team_name: &str,
        pending: bool,
    ) -> bool {
        let agent_id = identity::format_agent_id(agent_name, team_name);
        let Some(task_id) = Self::find_task(&agent_id) else {
            return false;
        };
        Self::set_plan_approval_pending(&task_id, pending)
    }

    /// Update the teammate permission mode tracked by the in-process registry.
    pub fn set_permission_mode(task_id: &str, mode: PermissionMode) -> bool {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.permission_mode = mode;
            return true;
        }
        false
    }

    /// Update the teammate permission mode by agent name.
    pub fn set_permission_mode_by_agent(
        agent_name: &str,
        team_name: &str,
        mode: PermissionMode,
    ) -> bool {
        let agent_id = identity::format_agent_id(agent_name, team_name);
        let Some(task_id) = Self::find_task(&agent_id) else {
            return false;
        };
        Self::set_permission_mode(&task_id, mode)
    }

    /// Attach the tokio abort handle after spawning the runner task.
    pub fn set_abort_handle(task_id: &str, abort_handle: tokio::task::AbortHandle) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.abort_handle = Some(abort_handle);
        }
    }

    /// Queue a plain-text user message that arrived while the teammate was
    /// already working. The runner drains this queue between turns.
    pub fn push_pending_user_message(task_id: &str, message: String) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.pending_user_messages.push(message);
        }
    }

    /// Take all queued user messages for the next teammate turn.
    pub fn take_pending_user_messages(task_id: &str) -> Vec<String> {
        let mut registry = TASK_REGISTRY.lock();
        registry
            .get_mut(task_id)
            .map(|state| std::mem::take(&mut state.pending_user_messages))
            .unwrap_or_default()
    }

    /// Mark a task as shutdown-requested.
    pub fn request_shutdown(task_id: &str) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            state.shutdown_requested = true;
        }
    }

    /// Mark a runner failure and stop the task unless it was already stopped.
    pub fn mark_task_failed(task_id: &str, error: String) {
        let mut registry = TASK_REGISTRY.lock();
        if let Some(state) = registry.get_mut(task_id) {
            if state.status == TaskStatus::Running {
                state.status = TaskStatus::Stopped;
            }
            state.error = Some(error);
            state.is_idle = true;
        }
    }

    /// Remove a task from the registry.
    pub fn remove_task(task_id: &str) {
        let mut registry = TASK_REGISTRY.lock();
        registry.remove(task_id);
    }

    /// Get all task IDs.
    pub fn all_task_ids() -> Vec<String> {
        let registry = TASK_REGISTRY.lock();
        registry.keys().cloned().collect()
    }

    /// Read-only snapshot of every task in the registry.
    ///
    /// `InProcessTeammateTaskState` holds non-`Clone` data (an abort handle
    /// among other things), so we project the registry into a simple
    /// struct that callers can use for display or inspection without having
    /// to hold the registry lock.
    pub fn task_snapshots() -> Vec<TeammateTaskSnapshot> {
        let registry = TASK_REGISTRY.lock();
        registry
            .values()
            .map(|state| TeammateTaskSnapshot {
                id: state.id.clone(),
                agent_id: state.identity.agent_id.clone(),
                agent_name: state.identity.agent_name.clone(),
                team_name: state.identity.team_name.clone(),
                status: state.status,
                is_idle: state.is_idle,
                has_error: state.error.is_some(),
                error_message: state.error.clone(),
                prompt: state.prompt.clone(),
                model: state.model.clone(),
                awaiting_plan_approval: state.awaiting_plan_approval,
                permission_mode: state.permission_mode.clone(),
            })
            .collect()
    }

    /// Clear all tasks (for testing).
    #[cfg(test)]
    pub fn clear_registry() {
        let mut registry = TASK_REGISTRY.lock();
        registry.clear();
    }
}

impl Default for InProcessBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable projection of an in-process teammate task, safe to move around.
#[derive(Debug, Clone)]
pub struct TeammateTaskSnapshot {
    pub id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub team_name: String,
    pub status: TaskStatus,
    pub is_idle: bool,
    pub has_error: bool,
    pub error_message: Option<String>,
    pub prompt: String,
    pub model: Option<String>,
    pub awaiting_plan_approval: bool,
    pub permission_mode: PermissionMode,
}

#[async_trait]
impl TeammateExecutor for InProcessBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::InProcess
    }

    async fn is_available(&self) -> bool {
        true // Always available
    }

    async fn spawn(&self, config: TeammateSpawnConfig) -> Result<TeammateSpawnResult> {
        let agent_id = identity::format_agent_id(&config.name, &config.team_name);
        let task_id = format!("in_process_teammate_{}", uuid::Uuid::new_v4());

        let identity = TeammateIdentity {
            agent_id: agent_id.clone(),
            agent_name: config.name.clone(),
            team_name: config.team_name.clone(),
            color: config.color.clone(),
            plan_mode_required: config.plan_mode_required,
            parent_session_id: config.parent_session_id.clone(),
        };

        // Create task state
        let task_state = InProcessTeammateTaskState {
            id: task_id.clone(),
            status: TaskStatus::Running,
            identity: identity.clone(),
            prompt: config.prompt.clone(),
            model: config.model.clone(),
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
        };

        // Register task
        let cancellation = CancellationToken::new();
        let runner_config = runner::InProcessRunnerConfig {
            identity: identity.clone(),
            task_id: task_id.clone(),
            prompt: config.prompt.clone(),
            agent_type: config.agent_type.clone(),
            model: config.model.clone(),
            system_prompt: config.system_prompt.clone(),
            system_prompt_mode: config.system_prompt_mode,
            cwd: config.cwd.clone(),
            cancellation: cancellation.clone(),
        };

        let mut task_state = task_state;
        task_state.cancellation_token = Some(cancellation);
        Self::register_task(task_state);
        let handle = runner::start_runner(runner_config);
        let abort_handle = handle.abort_handle();
        Self::set_abort_handle(&task_id, abort_handle.clone());
        std::mem::drop(handle);

        info!(
            agent_id = %agent_id,
            task_id = %task_id,
            "in-process teammate spawned"
        );

        Ok(TeammateSpawnResult {
            success: true,
            agent_id,
            error: None,
            abort_handle: Some(abort_handle),
            task_id: Some(task_id),
            pane_id: None,
        })
    }

    async fn send_message(
        &self,
        agent_id: &str,
        team_name: &str,
        message: TeammateMessage,
    ) -> Result<()> {
        let (agent_name, _) = identity::parse_agent_id(agent_id)
            .ok_or_else(|| anyhow::anyhow!("invalid agent_id: {}", agent_id))?;
        mailbox::write_to_mailbox(&agent_name, message, team_name)
    }

    async fn terminate(&self, agent_id: &str, team_name: &str, reason: Option<&str>) -> bool {
        let task_id = match Self::find_task(agent_id) {
            Some(id) => id,
            None => {
                warn!(agent_id, "no task found for agent");
                return false;
            }
        };

        // Check if already requested
        {
            let registry = TASK_REGISTRY.lock();
            if let Some(state) = registry.get(&task_id) {
                if state.shutdown_requested {
                    debug!(agent_id, "shutdown already requested");
                    return true;
                }
            }
        }

        // Send shutdown request via mailbox
        let (agent_name, _) = match identity::parse_agent_id(agent_id) {
            Some(parts) => parts,
            None => return false,
        };

        let now = chrono::Utc::now();
        let request_id = protocol::shutdown_request_id(agent_id, now.timestamp());
        let shutdown_msg = serde_json::json!({
            "type": "shutdown_request",
            "requestId": request_id,
            "from": TEAM_LEAD_NAME,
            "reason": reason.unwrap_or("Team lead requested shutdown"),
            "timestamp": now.to_rfc3339(),
        });

        let message = TeammateMessage {
            from: TEAM_LEAD_NAME.into(),
            text: shutdown_msg.to_string(),
            timestamp: now.to_rfc3339(),
            read: false,
            color: None,
            summary: Some("Shutdown request".into()),
        };

        if let Err(e) = mailbox::write_to_mailbox(&agent_name, message, team_name) {
            warn!(error = %e, "failed to write shutdown request");
            return false;
        }

        Self::request_shutdown(&task_id);

        info!(agent_id, "shutdown request sent");
        true
    }

    async fn kill(&self, agent_id: &str) -> bool {
        let task_id = match Self::find_task(agent_id) {
            Some(id) => id,
            None => return false,
        };

        let (abort_handle, cancellation_token) = {
            let mut registry = TASK_REGISTRY.lock();
            let Some(state) = registry.get_mut(&task_id) else {
                return false;
            };
            state.status = TaskStatus::Stopped;
            state.shutdown_requested = true;
            state.is_idle = true;
            (state.abort_handle.clone(), state.cancellation_token.clone())
        };

        if let Some(token) = cancellation_token {
            token.cancel();
        }
        if let Some(handle) = abort_handle {
            handle.abort();
        }
        info!(agent_id, "teammate force-killed");
        true
    }

    async fn is_active(&self, agent_id: &str) -> bool {
        let task_id = match Self::find_task(agent_id) {
            Some(id) => id,
            None => return false,
        };
        let registry = TASK_REGISTRY.lock();
        registry
            .get(&task_id)
            .map(|s| s.status == TaskStatus::Running)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        InProcessBackend::clear_registry();
    }

    #[test]
    #[serial_test::serial]
    fn test_backend_type() {
        let backend = InProcessBackend::new();
        assert_eq!(backend.backend_type(), BackendType::InProcess);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_is_available() {
        let backend = InProcessBackend::new();
        assert!(backend.is_available().await);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_spawn_and_active() {
        setup();
        let backend = InProcessBackend::new();
        let config = TeammateSpawnConfig {
            name: "test-worker".into(),
            team_name: "test-team".into(),
            color: Some("blue".into()),
            plan_mode_required: false,
            prompt: "Do something".into(),
            agent_type: None,
            cwd: ".".into(),
            model: None,
            system_prompt: None,
            system_prompt_mode: None,
            worktree_path: None,
            parent_session_id: "sess-1".into(),
            permissions: vec![],
            allow_permission_prompts: false,
        };

        let result = backend.spawn(config).await.unwrap();
        assert!(result.success);
        assert_eq!(result.agent_id, "test-worker@test-team");
        assert!(result.task_id.is_some());
        assert!(result.abort_handle.is_some());

        assert!(backend.is_active("test-worker@test-team").await);
        assert_eq!(InProcessBackend::running_task_count(), 1);

        setup(); // cleanup
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_kill() {
        setup();
        let backend = InProcessBackend::new();
        let config = TeammateSpawnConfig {
            name: "kill-test".into(),
            team_name: "t".into(),
            color: None,
            plan_mode_required: false,
            prompt: "p".into(),
            agent_type: None,
            cwd: ".".into(),
            model: None,
            system_prompt: None,
            system_prompt_mode: None,
            worktree_path: None,
            parent_session_id: "s".into(),
            permissions: vec![],
            allow_permission_prompts: false,
        };

        backend.spawn(config).await.unwrap();
        assert!(backend.is_active("kill-test@t").await);

        let killed = backend.kill("kill-test@t").await;
        assert!(killed);
        assert!(!backend.is_active("kill-test@t").await);

        setup();
    }

    #[test]
    #[serial_test::serial]
    fn test_has_working_teammates_empty() {
        setup();
        assert!(!InProcessBackend::has_working_teammates());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_kill_nonexistent() {
        setup();
        let backend = InProcessBackend::new();
        assert!(!backend.kill("nonexistent@team").await);
    }

    #[test]
    #[serial_test::serial]
    fn pending_messages_are_drained_once() {
        setup();
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: "task-1".into(),
            status: TaskStatus::Running,
            identity: TeammateIdentity {
                agent_id: "worker@team".into(),
                agent_name: "worker".into(),
                team_name: "team".into(),
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

        InProcessBackend::push_pending_user_message("task-1", "Message from lead: go".into());
        assert_eq!(
            InProcessBackend::take_pending_user_messages("task-1"),
            vec!["Message from lead: go".to_string()]
        );
        assert!(InProcessBackend::take_pending_user_messages("task-1").is_empty());
        setup();
    }

    #[test]
    #[serial_test::serial]
    fn plan_approval_and_permission_mode_state_can_be_updated_by_agent() {
        setup();
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
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Plan,
            error: None,
            pending_user_messages: vec![],
            is_idle: false,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });

        assert!(InProcessBackend::set_plan_approval_pending_by_agent(
            "worker", "team", true
        ));
        assert!(InProcessBackend::set_permission_mode_by_agent(
            "worker",
            "team",
            PermissionMode::AcceptEdits
        ));

        let snapshot = InProcessBackend::task_snapshots()
            .into_iter()
            .find(|task| task.agent_name == "worker")
            .unwrap();
        assert!(snapshot.awaiting_plan_approval);
        assert_eq!(snapshot.permission_mode, PermissionMode::AcceptEdits);
        setup();
    }
}
