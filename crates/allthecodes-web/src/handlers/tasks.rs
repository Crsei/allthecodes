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
use allthecodes_services::scheduler::{
    migrate_default_scheduler_data, ScheduleKind, ScheduledTask, SchedulerService, TaskPayload,
};
use allthecodes_tasks::{TaskEntry, TaskStatus};

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
        let scheduler = SchedulerService::open_default();
        migrate_default_scheduler_data(&scheduler).map_err(|error| ProtocolApiError::Internal {
            message: error.to_string(),
        })?;
        let scheduled_tasks =
            scheduler
                .list_definitions()
                .map_err(|error| ProtocolApiError::Internal {
                    message: error.to_string(),
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

        let scheduler = SchedulerService::open_default();
        migrate_default_scheduler_data(&scheduler).map_err(|error| ProtocolApiError::Internal {
            message: error.to_string(),
        })?;
        let tasks = scheduler
            .list_definitions()
            .map_err(|error| ProtocolApiError::Internal {
                message: error.to_string(),
            })?;
        let task = tasks
            .iter()
            .find(|task| task.id.as_str() == params.id)
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

fn scheduled_task_item(task: &ScheduledTask) -> TaskItem {
    TaskItem {
        id: task.id.to_string(),
        title: task.name.clone(),
        kind: "scheduled_agent".to_string(),
        state: if task.paused {
            "canceled".to_string()
        } else {
            "pending".to_string()
        },
        progress_current: None,
        progress_total: None,
        summary: scheduled_task_summary(task),
        elapsed_ms: 0,
        session_id: task.metadata.session_id.clone(),
        cwd: task.metadata.working_directory.clone(),
        schedule: Some(task.schedule.clone()),
        schedule_kind: Some(schedule_kind(task.schedule_kind).to_string()),
        enabled: Some(!task.paused),
        last_run_at: task.last_run_at.map(|value| value.to_rfc3339()),
        next_run_at: Some(task.next_run_at.to_rfc3339()),
    }
}

fn scheduled_task_summary(task: &ScheduledTask) -> String {
    if task.paused {
        return "Disabled".to_string();
    }
    match &task.payload {
        TaskPayload::Prompt(prompt) | TaskPayload::SlashCommand(prompt) => {
            let first_line = prompt.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                format!("Next run at {}", task.next_run_at.to_rfc3339())
            } else {
                first_line.chars().take(160).collect()
            }
        }
    }
}

fn schedule_kind(schedule: ScheduleKind) -> &'static str {
    match schedule {
        ScheduleKind::Interval => "interval",
        ScheduleKind::Cron => "cron",
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
    use allthecodes_services::scheduler::{Interval, SchedulerKind, SchedulerStore};
    use allthecodes_tasks::{TaskCreateOptions, TaskStatus, TaskStore, TASK_KIND_LOCAL_AGENT};
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn scheduled_task_maps_to_task_item() {
        let now = chrono::Utc::now();
        let task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "daily-review",
            "1h",
            Interval::from_seconds(3600),
            TaskPayload::Prompt("summarize today's work".to_string()),
            now,
        );

        let item = scheduled_task_item(&task);

        assert_eq!(item.id, task.id.as_str());
        assert_eq!(item.title, "daily-review");
        assert_eq!(item.kind, "scheduled_agent");
        assert_eq!(item.state, "pending");
        assert_eq!(item.cwd, None);
        assert_eq!(item.schedule_kind.as_deref(), Some("interval"));
        assert_eq!(item.schedule.as_deref(), Some("1h"));
        assert_eq!(item.next_run_at, Some(task.next_run_at.to_rfc3339()));
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

    #[tokio::test]
    #[serial_test::serial]
    async fn task_processors_project_canonical_scheduler_definitions() {
        let (_home, _guard) = temp_home();
        let now = chrono::Utc::now();
        let mut scheduled = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "canonical-nightly-review",
            "1h",
            Interval::from_seconds(3600),
            TaskPayload::Prompt("summarize the canonical scheduler".to_string()),
            now,
        );
        scheduled.metadata.session_id = Some("scheduled-session".to_string());
        scheduled.metadata.working_directory = Some("/repo/scheduled".to_string());
        let scheduled = SchedulerStore::open_default()
            .add(scheduled)
            .expect("persist canonical scheduler definition");

        let state = make_web_state();
        let list = TaskListProcessor::from(state.clone())
            .handle(allthecodes_protocol::NoParams {})
            .await
            .expect("task list response");
        let item = list
            .tasks
            .iter()
            .find(|item| item.id == scheduled.id.as_str())
            .expect("canonical scheduler definition should be listed");
        assert_eq!(item.title, "canonical-nightly-review");
        assert_eq!(item.kind, "scheduled_agent");
        assert_eq!(item.session_id.as_deref(), Some("scheduled-session"));
        assert_eq!(item.cwd.as_deref(), Some("/repo/scheduled"));
        assert_eq!(item.schedule.as_deref(), Some("1h"));
        assert_eq!(item.schedule_kind.as_deref(), Some("interval"));

        let detail = TaskDetailProcessor::from(state)
            .handle(TaskDetailParams {
                id: scheduled.id.to_string(),
            })
            .await
            .expect("task detail response");
        assert_eq!(detail.task.id, scheduled.id.as_str());
        assert_eq!(detail.task.title, "canonical-nightly-review");
        assert_eq!(detail.task.session_id.as_deref(), Some("scheduled-session"));
        assert_eq!(detail.task.cwd.as_deref(), Some("/repo/scheduled"));
    }
}
