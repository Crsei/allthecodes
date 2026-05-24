//! Task activity rendering.

use super::task_status_utils::kind_label;
use super::{TaskState, TaskStatus};
use crate::ui::tool_activity::{render_grouped_activity, ToolActivity, ToolState};

pub fn render_task_tool_activity(tasks: &[TaskStatus]) -> String {
    if tasks.is_empty() {
        return "No task activity".to_string();
    }

    let activities = tasks.iter().map(task_to_activity).collect::<Vec<_>>();
    render_grouped_activity(&activities)
}

fn task_to_activity(task: &TaskStatus) -> ToolActivity {
    let mut activity = ToolActivity::new(task.title.clone(), task_state_to_tool_state(task.state));
    activity.user_facing_name = Some(task.title.clone());
    activity.arguments_summary = Some(kind_label(task.kind).to_string());
    activity.summary = task.summary.clone();
    activity.elapsed_ms = task.elapsed_ms;
    activity.progress = task.progress;
    activity.output_lines = task.output_lines.len();
    activity.output_preview = task.output_lines.iter().take(2).cloned().collect();
    if task.state == TaskState::Failed && !task.summary.is_empty() {
        activity.error_summary = Some(task.summary.clone());
    }
    activity
}

fn task_state_to_tool_state(state: TaskState) -> ToolState {
    match state {
        TaskState::Pending => ToolState::Queued,
        TaskState::Running => ToolState::Running,
        TaskState::Succeeded => ToolState::Succeeded,
        TaskState::Failed => ToolState::Failed,
        TaskState::Canceled => ToolState::Cancelled,
    }
}
