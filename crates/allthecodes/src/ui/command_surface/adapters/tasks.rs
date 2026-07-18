use crate::ui::command_surface::surfaces::tasks::{TaskSurfaceItem, TaskSurfaceSource};
use crate::ui::messages::task_list_content::TaskListItem;
use crate::ui::tasks::{
    TaskKind as UiTaskKind, TaskState as UiTaskState, TaskStatus as UiTaskStatus,
};
use serde_json::Value;
pub(crate) fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn elapsed_ms_since_timestamp(timestamp: i64) -> u64 {
    chrono::Utc::now()
        .timestamp()
        .saturating_sub(timestamp)
        .max(0) as u64
        * 1000
}

pub(crate) fn task_surface_items() -> Vec<TaskSurfaceItem> {
    let mut items = allthecodes_tasks::global_store()
        .list()
        .into_iter()
        .map(tool_task_surface_item)
        .collect::<Vec<_>>();
    items.extend(
        allthecodes_teams::in_process::InProcessBackend::task_snapshots()
            .into_iter()
            .map(team_task_surface_item),
    );
    items
}

/// Snapshot the task-store data used by the compact expanded task list.
///
/// Team activity deliberately stays out of this list: the `Teammates` view
/// owns that surface, while this one mirrors the plan/task state maintained by
/// `allthecodes_tasks`.
pub(crate) fn task_list_items() -> Vec<TaskListItem> {
    allthecodes_tasks::global_store()
        .list()
        .into_iter()
        .map(|task| TaskListItem {
            title: if task.subject.trim().is_empty() {
                task.id.clone()
            } else {
                task.subject.clone()
            },
            state: ui_task_state_from_tool_status(task.status),
            blocked_by: task.depends_on,
            updated_at: task.updated_at,
        })
        .collect()
}

pub(crate) fn tool_task_surface_item(task: allthecodes_tasks::TaskEntry) -> TaskSurfaceItem {
    if task.kind == allthecodes_tasks::TASK_KIND_LOCAL_WORKFLOW {
        return workflow_task_surface_item(task);
    }

    let title = if task.subject.trim().is_empty() {
        task.id.clone()
    } else {
        task.subject.clone()
    };
    let summary = first_non_empty([
        task.output_summary.as_str(),
        task.description.as_str(),
        task.status.as_str(),
    ]);
    let elapsed_ms = elapsed_ms_since_timestamp(task.created_at);
    let output_lines = task
        .output
        .lines()
        .take(20)
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    TaskSurfaceItem {
        task: UiTaskStatus {
            id: task.id,
            title,
            kind: ui_task_kind_from_tool_kind(&task.kind),
            state: ui_task_state_from_tool_status(task.status),
            progress: None,
            summary,
            elapsed_ms,
            output_lines,
        },
        source: TaskSurfaceSource::Tool,
    }
}

fn workflow_task_surface_item(task: allthecodes_tasks::TaskEntry) -> TaskSurfaceItem {
    let metadata = task.metadata.as_ref();
    let workflow_name = metadata_string(metadata, "workflow_name");
    let workflow_file = metadata_string(metadata, "workflow_file");
    let workflow_path = metadata_string(metadata, "workflow_path");
    let run_status =
        metadata_string(metadata, "run_status").unwrap_or_else(|| task.status.as_str().to_string());
    let total_steps = metadata_usize(metadata, "total_steps").unwrap_or_default();
    let completed_steps = metadata_usize(metadata, "completed_steps").unwrap_or_default();
    let current_step_number = metadata_usize(metadata, "current_step_number");
    let current_step_name = metadata_string(metadata, "current_step_name");

    let title = workflow_name
        .as_deref()
        .map(|name| format!("Workflow: {name}"))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            if task.subject.trim().is_empty() {
                task.id.clone()
            } else {
                task.subject.clone()
            }
        });
    let summary = if let (Some(step_number), Some(step_name)) =
        (current_step_number, current_step_name.as_deref())
    {
        format!("{run_status} - step {step_number}/{total_steps}: {step_name}")
    } else if total_steps > 0 {
        format!("{run_status} - {completed_steps}/{total_steps} steps")
    } else {
        first_non_empty([
            task.output_summary.as_str(),
            task.description.as_str(),
            task.status.as_str(),
        ])
    };
    let mut output_lines = vec![
        format!("Run id: {}", task.id),
        format!("Status: {run_status}"),
    ];
    if let Some(file) = workflow_file {
        output_lines.push(format!("Workflow file: {file}"));
    }
    if let Some(path) = workflow_path {
        output_lines.push(format!("Workflow path: {path}"));
    }
    if let (Some(step_number), Some(step_name)) = (current_step_number, current_step_name) {
        output_lines.push(format!(
            "Current step: {step_number}/{total_steps} {step_name}"
        ));
    }
    if total_steps > 0 {
        output_lines.push(format!(
            "Progress: {completed_steps}/{total_steps} completed"
        ));
    }
    output_lines.extend(
        task.output
            .lines()
            .take(20)
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    );

    TaskSurfaceItem {
        task: UiTaskStatus {
            id: task.id,
            title,
            kind: UiTaskKind::Workflow,
            state: ui_task_state_from_tool_status(task.status),
            progress: (total_steps > 0).then_some((completed_steps.min(total_steps), total_steps)),
            summary,
            elapsed_ms: elapsed_ms_since_timestamp(task.created_at),
            output_lines,
        },
        source: TaskSurfaceSource::Tool,
    }
}

fn metadata_string(metadata: Option<&Value>, key: &str) -> Option<String> {
    metadata?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn metadata_usize(metadata: Option<&Value>, key: &str) -> Option<usize> {
    metadata?
        .get(key)?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
}

pub(crate) fn team_task_surface_item(
    task: allthecodes_teams::in_process::TeammateTaskSnapshot,
) -> TaskSurfaceItem {
    let summary = if task.has_error {
        first_non_empty([
            task.error_message.as_deref().unwrap_or_default(),
            "team task failed",
        ])
    } else if task.awaiting_plan_approval {
        format!("awaiting plan approval ({})", task.permission_mode.as_str())
    } else if task.is_idle {
        "idle".to_string()
    } else {
        first_non_empty([task.prompt.as_str(), "working"])
    };

    TaskSurfaceItem {
        task: UiTaskStatus {
            id: task.id,
            title: format!("{} ({})", task.agent_name, task.team_name),
            kind: UiTaskKind::InProcessTeammate,
            state: ui_task_state_from_team_status(task.status, task.has_error),
            progress: None,
            summary,
            elapsed_ms: 0,
            output_lines: vec![task.prompt.clone()],
        },
        source: TaskSurfaceSource::Team {
            teammate_name: task.agent_name,
        },
    }
}

pub(crate) fn ui_task_kind_from_tool_kind(kind: &str) -> UiTaskKind {
    match kind.to_ascii_lowercase().as_str() {
        value if value.contains("agent") => UiTaskKind::AsyncAgent,
        value if value.contains("remote") => UiTaskKind::RemoteSession,
        value if value.contains("monitor") => UiTaskKind::MonitorMcp,
        value if value.contains("dream") => UiTaskKind::Dream,
        value if value.contains("workflow") => UiTaskKind::Workflow,
        _ => UiTaskKind::Shell,
    }
}

pub(crate) fn ui_task_state_from_tool_status(status: allthecodes_tasks::TaskStatus) -> UiTaskState {
    match status {
        allthecodes_tasks::TaskStatus::Pending => UiTaskState::Pending,
        allthecodes_tasks::TaskStatus::InProgress
        | allthecodes_tasks::TaskStatus::Interrupted
        | allthecodes_tasks::TaskStatus::Recoverable => UiTaskState::Running,
        allthecodes_tasks::TaskStatus::Completed => UiTaskState::Succeeded,
        allthecodes_tasks::TaskStatus::Failed => UiTaskState::Failed,
        allthecodes_tasks::TaskStatus::Cancelled | allthecodes_tasks::TaskStatus::Stopped => {
            UiTaskState::Canceled
        }
    }
}

pub(crate) fn ui_task_state_from_team_status(
    status: allthecodes_teams::types::TaskStatus,
    has_error: bool,
) -> UiTaskState {
    if has_error {
        return UiTaskState::Failed;
    }
    match status {
        allthecodes_teams::types::TaskStatus::Running => UiTaskState::Running,
        allthecodes_teams::types::TaskStatus::Stopped => UiTaskState::Canceled,
        allthecodes_teams::types::TaskStatus::Completed => UiTaskState::Succeeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workflow_task_surfaces_adapter_uses_local_workflow_metadata() {
        let task = allthecodes_tasks::TaskEntry {
            id: "workflow-run-1".to_string(),
            kind: allthecodes_tasks::TASK_KIND_LOCAL_WORKFLOW.to_string(),
            subject: "Workflow: release".to_string(),
            description: "/repo/.allthecodes/workflows/release.md".to_string(),
            status: allthecodes_tasks::TaskStatus::InProgress,
            output: "Current step: 2/3 Publish release".to_string(),
            output_summary: String::new(),
            output_bytes: 0,
            output_truncated: false,
            parent_id: None,
            depends_on: Vec::new(),
            owner: None,
            active_form: None,
            metadata: Some(json!({
                "workflow_name": "release",
                "workflow_file": "release.md",
                "workflow_path": "/repo/.allthecodes/workflows/release.md",
                "current_step_number": 2,
                "current_step_name": "Publish release",
                "total_steps": 3,
                "completed_steps": 1,
                "run_status": "running",
            })),
            tool_use_id: None,
            agent_id: None,
            supervisor_id: None,
            isolation: None,
            worktree_path: None,
            worktree_branch: None,
            remote_task_type: None,
            remote_session_id: None,
            remote_task_metadata: None,
            poll_started_at: None,
            cancel_requested_at: None,
            recovered_at: None,
            previous_status: None,
            runtime_activity: None,
            created_at: 0,
            updated_at: 0,
        };

        let item = tool_task_surface_item(task);
        assert_eq!(item.task.kind, UiTaskKind::Workflow);
        assert_eq!(item.task.state, UiTaskState::Running);
        assert_eq!(item.task.title, "Workflow: release");
        assert_eq!(item.task.progress, Some((1, 3)));
        assert_eq!(item.task.summary, "running - step 2/3: Publish release");
        assert!(item
            .task
            .output_lines
            .iter()
            .any(|line| line == "Workflow file: release.md"));
    }
}
