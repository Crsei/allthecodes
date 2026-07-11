//! Task-domain state, store option, output, and result types.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// Options accepted by task store creation.
#[derive(Debug, Clone, Default)]
pub struct TaskCreateOptions {
    pub kind: Option<String>,
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub owner: Option<String>,
    pub active_form: Option<String>,
    pub metadata: Option<Value>,
    pub tool_use_id: Option<String>,
    pub agent_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub isolation: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub remote_task_type: Option<String>,
    pub remote_session_id: Option<String>,
    pub remote_task_metadata: Option<Value>,
    pub poll_started_at: Option<i64>,
    pub runtime_activity: Option<AgentRuntimeActivity>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimePhase {
    #[default]
    Queued,
    Running,
    WaitingForPermission,
    Stalled,
    NeedsManualRecovery,
    Completed,
    Failed,
    Cancelled,
}

impl AgentRuntimePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingForPermission => "waiting_for_permission",
            Self::Stalled => "stalled",
            Self::NeedsManualRecovery => "needs_manual_recovery",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntimeActivity {
    pub phase: AgentRuntimePhase,
    pub last_heartbeat_at_ms: i64,
    pub last_progress_at_ms: i64,
    pub task_id: String,
    pub agent_id: String,
    pub child_session_id: String,
    pub partial_output_bytes: usize,
}

/// Fields accepted by task updates.
#[derive(Debug, Clone, Default)]
pub struct TaskUpdateFields {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub active_form: Option<Option<String>>,
    pub owner: Option<Option<String>>,
    pub metadata_patch: Option<Value>,
    pub status: Option<TaskStatus>,
    pub add_blocks: Vec<String>,
    pub add_blocked_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnassignedTaskSummary {
    pub id: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnassignTeammateTasksResult {
    pub unassigned_tasks: Vec<UnassignedTaskSummary>,
    pub notification_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeammateTaskExitReason {
    Terminated,
    Shutdown,
}

/// A process-local handle used to cancel active task execution.
#[derive(Debug, Clone)]
pub struct TaskRuntimeHandle {
    cancellation_token: CancellationToken,
}

impl TaskRuntimeHandle {
    pub fn new(cancellation_token: CancellationToken) -> Self {
        Self { cancellation_token }
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }
}

/// A single task entry.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub output: String,
    pub output_summary: String,
    pub output_bytes: usize,
    pub output_truncated: bool,
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub owner: Option<String>,
    pub active_form: Option<String>,
    pub metadata: Option<Value>,
    pub tool_use_id: Option<String>,
    pub agent_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub isolation: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub remote_task_type: Option<String>,
    pub remote_session_id: Option<String>,
    pub remote_task_metadata: Option<Value>,
    pub poll_started_at: Option<i64>,
    pub cancel_requested_at: Option<i64>,
    pub recovered_at: Option<i64>,
    pub previous_status: Option<TaskStatus>,
    pub runtime_activity: Option<AgentRuntimeActivity>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Recoverable,
    /// Legacy status retained so old serialized values can migrate cleanly.
    Stopped,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Interrupted => "interrupted",
            TaskStatus::Recoverable => "recoverable",
            TaskStatus::Stopped => "stopped",
        }
    }

    pub fn from_status_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" | "running" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "failed" => Some(TaskStatus::Failed),
            "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
            "interrupted" => Some(TaskStatus::Interrupted),
            "recoverable" => Some(TaskStatus::Recoverable),
            "stopped" => Some(TaskStatus::Stopped),
            _ => None,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Self::from_status_str(s)
    }

    pub fn should_interrupt_on_startup(self) -> bool {
        matches!(
            self,
            TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Recoverable
        )
    }

    pub fn is_success(self) -> bool {
        matches!(self, TaskStatus::Completed)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Completed
                | TaskStatus::Failed
                | TaskStatus::Cancelled
                | TaskStatus::Interrupted
                | TaskStatus::Stopped
        )
    }

    pub fn is_active_for_output_wait(self) -> bool {
        matches!(
            self,
            TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Recoverable
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClaimFailureReason {
    TaskNotFound,
    AlreadyClaimed,
    AlreadyResolved,
    Blocked,
    AgentBusy,
    OwnerRequired,
    LockUnavailable,
}

impl TaskClaimFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskClaimFailureReason::TaskNotFound => "task_not_found",
            TaskClaimFailureReason::AlreadyClaimed => "already_claimed",
            TaskClaimFailureReason::AlreadyResolved => "already_resolved",
            TaskClaimFailureReason::Blocked => "blocked",
            TaskClaimFailureReason::AgentBusy => "agent_busy",
            TaskClaimFailureReason::OwnerRequired => "owner_required",
            TaskClaimFailureReason::LockUnavailable => "lock_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskClaimFailure {
    pub reason: TaskClaimFailureReason,
    pub owner: Option<String>,
    pub blocked_by: Vec<String>,
    pub busy_task_id: Option<String>,
}

impl TaskClaimFailure {
    pub fn new(reason: TaskClaimFailureReason) -> Self {
        Self {
            reason,
            owner: None,
            blocked_by: Vec::new(),
            busy_task_id: None,
        }
    }

    pub fn with_owner(mut self, owner: String) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn with_blocked_by(mut self, blocked_by: Vec<String>) -> Self {
        self.blocked_by = blocked_by;
        self
    }

    pub fn with_busy_task_id(mut self, busy_task_id: String) -> Self {
        self.busy_task_id = Some(busy_task_id);
        self
    }

    pub fn to_json(&self) -> Value {
        json!({
            "reason": self.reason.as_str(),
            "owner": self.owner.clone(),
            "blocked_by": self.blocked_by.clone(),
            "blockedBy": self.blocked_by.clone(),
            "busy_task_id": self.busy_task_id.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    #[serde(
        rename = "activeForm",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub active_form: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TodoWriteOutcome {
    pub todos: Vec<TodoItem>,
    pub cleared: bool,
    pub verification_nudge_needed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutputRetrievalStatus {
    Success,
    Timeout,
    NotReady,
}

impl TaskOutputRetrievalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskOutputRetrievalStatus::Success => "success",
            TaskOutputRetrievalStatus::Timeout => "timeout",
            TaskOutputRetrievalStatus::NotReady => "not_ready",
        }
    }
}

#[derive(Debug)]
pub enum TaskOutputWaitResult {
    Ready(TaskEntry),
    TimedOut(Option<TaskEntry>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTaskFile {
    pub schema_version: u32,
    pub task: PersistedTaskRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTaskRecord {
    pub id: String,
    #[serde(default = "default_task_kind")]
    pub kind: String,
    pub subject: String,
    pub description: String,
    pub status: String,
    #[serde(default)]
    pub output_file: Option<String>,
    #[serde(default)]
    pub output_summary: String,
    #[serde(default)]
    pub output_bytes: usize,
    #[serde(default)]
    pub output_truncated: bool,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default, alias = "blocked_by", alias = "blockedBy")]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default, rename = "activeForm")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub supervisor_id: Option<String>,
    #[serde(default)]
    pub isolation: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub remote_task_type: Option<String>,
    #[serde(default)]
    pub remote_session_id: Option<String>,
    #[serde(default)]
    pub remote_task_metadata: Option<Value>,
    #[serde(default)]
    pub poll_started_at: Option<i64>,
    #[serde(default)]
    pub cancel_requested_at: Option<i64>,
    #[serde(default)]
    pub recovered_at: Option<i64>,
    #[serde(default)]
    pub previous_status: Option<String>,
    #[serde(default)]
    pub runtime_activity: Option<AgentRuntimeActivity>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing)]
    pub legacy_inline_output: Option<String>,
}

pub fn default_task_kind() -> String {
    crate::domain::TASK_KIND_TOOL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_round_trips_persisted_strings() {
        let cases = [
            (TaskStatus::Pending, "pending"),
            (TaskStatus::InProgress, "in_progress"),
            (TaskStatus::Completed, "completed"),
            (TaskStatus::Failed, "failed"),
            (TaskStatus::Cancelled, "cancelled"),
            (TaskStatus::Interrupted, "interrupted"),
            (TaskStatus::Recoverable, "recoverable"),
            (TaskStatus::Stopped, "stopped"),
        ];

        for (status, value) in cases {
            assert_eq!(status.as_str(), value);
            assert_eq!(TaskStatus::from_str(value), Some(status));
        }
        assert_eq!(
            TaskStatus::from_str("running"),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(
            TaskStatus::from_str("canceled"),
            Some(TaskStatus::Cancelled)
        );
        assert_eq!(TaskStatus::from_str("invalid"), None);
    }

    #[test]
    fn task_status_classifies_runtime_states() {
        assert!(TaskStatus::Pending.should_interrupt_on_startup());
        assert!(TaskStatus::InProgress.is_active_for_output_wait());
        assert!(TaskStatus::Recoverable.is_active_for_output_wait());
        assert!(TaskStatus::Completed.is_success());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
    }

    #[test]
    fn task_claim_failure_json_keeps_legacy_alias() {
        let failure = TaskClaimFailure::new(TaskClaimFailureReason::Blocked)
            .with_owner("agent-a".to_string())
            .with_blocked_by(vec!["dep-1".to_string()]);

        let value = failure.to_json();
        assert_eq!(value["reason"], "blocked");
        assert_eq!(value["owner"], "agent-a");
        assert_eq!(value["blocked_by"], json!(["dep-1"]));
        assert_eq!(value["blockedBy"], json!(["dep-1"]));
    }

    #[test]
    fn task_output_status_strings_match_tool_payload_schema() {
        assert_eq!(TaskOutputRetrievalStatus::Success.as_str(), "success");
        assert_eq!(TaskOutputRetrievalStatus::Timeout.as_str(), "timeout");
        assert_eq!(TaskOutputRetrievalStatus::NotReady.as_str(), "not_ready");
    }
}
