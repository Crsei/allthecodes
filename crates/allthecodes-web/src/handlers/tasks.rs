//! Task status handlers.
//!
//! Task status handlers.

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::response::Response;
use axum::routing::get;

use allthecodes_protocol::v1::tasks::{
    TaskDetailParams, TaskDetailResponse, TaskItem, TaskListResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_tasks::{ScheduleSpec, ScheduledAgentTask, TaskEntry, TaskStatus};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct TaskListProcessor {
    state: WebState,
}

impl From<WebState> for TaskListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for TaskListProcessor {
    type Request = allthecodes_protocol::NoParams;
    type Response = TaskListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "tasks.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = &self.state;
        let scheduled_tasks = allthecodes_tasks::load_scheduled_tasks().map_err(|error| {
            ProtocolApiError::Internal {
                message: error.to_string(),
            }
        })?;
        let task_store = allthecodes_tasks::global_store();
        let tracked_tasks = task_store.list();
        let mut tasks: Vec<TaskItem> = scheduled_tasks.iter().map(scheduled_task_item).collect();
        tasks.extend(tracked_tasks.iter().map(task_entry_item));
        Ok(TaskListResponse { tasks })
    }
}

#[derive(Clone)]
pub struct TaskDetailProcessor {
    state: WebState,
}

impl From<WebState> for TaskDetailProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for TaskDetailProcessor {
    type Request = TaskDetailParams;
    type Response = TaskDetailResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "tasks.detail"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = &self.state;
        let task_store = allthecodes_tasks::global_store();
        if let Some(entry) = task_store.get(&params.id) {
            return Ok(TaskDetailResponse {
                task: task_entry_item(&entry),
            });
        }

        let tasks = allthecodes_tasks::load_scheduled_tasks().map_err(|error| {
            ProtocolApiError::Internal {
                message: error.to_string(),
            }
        })?;
        let task = tasks
            .iter()
            .find(|task| task.id == params.id)
            .map(scheduled_task_item)
            .ok_or(ProtocolApiError::NotFound {
                entity: "task",
                id: params.id,
            })?;
        Ok(TaskDetailResponse { task })
    }
}

fn task_entry_item(entry: &TaskEntry) -> TaskItem {
    TaskItem {
        id: entry.id.clone(),
        title: entry.subject.clone(),
        kind: entry.kind.clone(),
        state: task_state(entry.status).to_string(),
        progress_current: None,
        progress_total: None,
        summary: task_entry_summary(entry),
        elapsed_ms: task_elapsed_ms(entry),
        session_id: entry
            .remote_session_id
            .clone()
            .or_else(|| entry.agent_id.clone()),
        cwd: task_entry_cwd(entry),
        schedule: None,
        schedule_kind: None,
        enabled: None,
        last_run_at: None,
        next_run_at: None,
    }
}

fn task_state(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress | TaskStatus::Recoverable => "running",
        TaskStatus::Completed => "succeeded",
        TaskStatus::Failed | TaskStatus::Interrupted => "failed",
        TaskStatus::Cancelled | TaskStatus::Stopped => "canceled",
    }
}

fn task_entry_summary(entry: &TaskEntry) -> String {
    if !entry.output_summary.trim().is_empty() {
        entry.output_summary.clone()
    } else if let Some(worktree_path) = entry.worktree_path.as_deref() {
        format!("{} ({worktree_path})", entry.description)
    } else {
        entry.description.clone()
    }
}

fn task_elapsed_ms(entry: &TaskEntry) -> u64 {
    let end = if entry.status.is_terminal() {
        entry.updated_at
    } else {
        chrono::Utc::now().timestamp()
    };
    end.saturating_sub(entry.created_at) as u64 * 1000
}

fn task_entry_cwd(entry: &TaskEntry) -> Option<String> {
    entry
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("cwd"))
        .and_then(|cwd| cwd.as_str())
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(ToString::to_string)
        .or_else(|| entry.worktree_path.clone())
}

fn scheduled_task_item(task: &ScheduledAgentTask) -> TaskItem {
    TaskItem {
        id: task.id.clone(),
        title: scheduled_task_title(&task.prompt),
        kind: "scheduled_agent".to_string(),
        state: if task.enabled {
            "pending".to_string()
        } else {
            "canceled".to_string()
        },
        progress_current: None,
        progress_total: None,
        summary: scheduled_task_summary(task),
        elapsed_ms: 0,
        session_id: None,
        cwd: Some(task.cwd.clone()),
        schedule: Some(schedule_label(&task.schedule)),
        schedule_kind: Some(schedule_kind(&task.schedule).to_string()),
        enabled: Some(task.enabled),
        last_run_at: task.last_run_at.clone(),
        next_run_at: task.next_run_at.clone(),
    }
}

fn scheduled_task_title(prompt: &str) -> String {
    let first_line = prompt.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        "Scheduled task".to_string()
    } else {
        first_line.chars().take(120).collect()
    }
}

fn scheduled_task_summary(task: &ScheduledAgentTask) -> String {
    match (&task.enabled, task.next_run_at.as_deref()) {
        (true, Some(next_run_at)) => format!("Next run at {next_run_at}"),
        (true, None) => "Enabled".to_string(),
        (false, _) => "Disabled".to_string(),
    }
}

fn schedule_label(schedule: &ScheduleSpec) -> String {
    match schedule {
        ScheduleSpec::Interval { every_seconds } => format!("every {every_seconds}s"),
        ScheduleSpec::Once { run_at } => format!("once at {run_at}"),
    }
}

fn schedule_kind(schedule: &ScheduleSpec) -> &'static str {
    match schedule {
        ScheduleSpec::Interval { .. } => "interval",
        ScheduleSpec::Once { .. } => "once",
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

async fn list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<TaskListProcessor>(
        state,
        ApiMethod::TaskList,
        allthecodes_protocol::NoParams {},
    )
    .await
}

async fn detail_handler(State(state): State<WebState>, AxumPath(id): AxumPath<String>) -> Response {
    rest_processor_response::<TaskDetailProcessor>(
        state,
        ApiMethod::TaskDetail,
        TaskDetailParams { id },
    )
    .await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::TaskList, get(list_handler))
        .handle(ApiMethod::TaskDetail, get(detail_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::{make_web_state, temp_home};
    use allthecodes_tasks::{
        ScheduleSpec, ScheduledAgentTask, TaskCreateOptions, TaskStatus, TaskStore,
        TASK_KIND_LOCAL_AGENT,
    };
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn scheduled_task_maps_to_task_item() {
        let task = ScheduledAgentTask {
            id: "daily-review".to_string(),
            prompt: "summarize today's work".to_string(),
            cwd: "/repo".to_string(),
            schedule: ScheduleSpec::Interval {
                every_seconds: 3600,
            },
            enabled: true,
            last_run_at: None,
            next_run_at: Some("2026-07-04T12:00:00+00:00".to_string()),
        };

        let item = scheduled_task_item(&task);

        assert_eq!(item.id, "daily-review");
        assert_eq!(item.title, "summarize today's work");
        assert_eq!(item.kind, "scheduled_agent");
        assert_eq!(item.state, "pending");
        assert_eq!(item.cwd.as_deref(), Some("/repo"));
        assert_eq!(item.schedule_kind.as_deref(), Some("interval"));
        assert_eq!(item.schedule.as_deref(), Some("every 3600s"));
        assert_eq!(
            item.next_run_at.as_deref(),
            Some("2026-07-04T12:00:00+00:00")
        );
    }

    #[test]
    fn local_agent_task_maps_to_task_item() {
        let dir = TempDir::new().expect("task store temp dir");
        let store = TaskStore::with_dir(dir.path());
        let entry = store
            .try_create_with_options(
                "Delegate explorer: inspect runtime lineage",
                "Inspect runtime lineage in an isolated child session",
                TaskCreateOptions {
                    kind: Some(TASK_KIND_LOCAL_AGENT.to_string()),
                    metadata: Some(json!({
                        "cwd": "/repo",
                        "delegate": true,
                        "parent_session_id": "parent-session",
                        "child_session_id": "child-session",
                    })),
                    agent_id: Some("child-session".to_string()),
                    remote_session_id: Some("child-session".to_string()),
                    isolation: Some("worktree".to_string()),
                    worktree_path: Some("/repo/.worktrees/delegate-lineage".to_string()),
                    ..TaskCreateOptions::default()
                },
            )
            .expect("create local task");
        store
            .try_update_status(&entry.id, TaskStatus::InProgress)
            .expect("update local task");
        store.append_output(&entry.id, "lineage scan started");
        let entry = store.get(&entry.id).expect("load local task");

        let item = task_entry_item(&entry);

        assert_eq!(item.id, entry.id);
        assert_eq!(item.title, "Delegate explorer: inspect runtime lineage");
        assert_eq!(item.kind, TASK_KIND_LOCAL_AGENT);
        assert_eq!(item.state, "running");
        assert_eq!(item.session_id.as_deref(), Some("child-session"));
        assert_eq!(item.cwd.as_deref(), Some("/repo"));
        assert_eq!(item.summary, "lineage scan started");
        assert_eq!(item.schedule, None);
        assert_eq!(item.enabled, None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn task_processors_include_default_store_tasks() {
        let (_home, _guard) = temp_home();
        let store = allthecodes_tasks::global_store();
        let entry = store
            .try_create_with_options(
                "Delegate builder: patch task view",
                "Patch task view",
                TaskCreateOptions {
                    kind: Some(TASK_KIND_LOCAL_AGENT.to_string()),
                    metadata: Some(json!({ "cwd": "/repo" })),
                    remote_session_id: Some("child-session".to_string()),
                    ..TaskCreateOptions::default()
                },
            )
            .expect("create default task");

        let state = make_web_state();
        let list = TaskListProcessor::from(state.clone())
            .handle(allthecodes_protocol::NoParams {})
            .await
            .expect("task list response");
        assert!(list.tasks.iter().any(|item| {
            item.id == entry.id
                && item.kind == TASK_KIND_LOCAL_AGENT
                && item.session_id.as_deref() == Some("child-session")
        }));

        let detail = TaskDetailProcessor::from(state)
            .handle(TaskDetailParams {
                id: entry.id.clone(),
            })
            .await
            .expect("task detail response");
        assert_eq!(detail.task.id, entry.id);
        assert_eq!(detail.task.cwd.as_deref(), Some("/repo"));
    }
}
