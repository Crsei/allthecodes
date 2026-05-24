//! Footer status for background tasks.

use super::{TaskState, TaskStatus};

pub fn render_background_task_status(tasks: &[TaskStatus], selected: bool) -> String {
    let running = tasks
        .iter()
        .filter(|task| task.state == TaskState::Running)
        .count();
    let pending = tasks
        .iter()
        .filter(|task| task.state == TaskState::Pending)
        .count();
    let succeeded = tasks
        .iter()
        .filter(|task| task.state == TaskState::Succeeded)
        .count();
    let failed = tasks
        .iter()
        .filter(|task| task.state == TaskState::Failed)
        .count();
    let canceled = tasks
        .iter()
        .filter(|task| task.state == TaskState::Canceled)
        .count();
    if tasks.is_empty() {
        return "no background tasks".to_string();
    }
    let mut label = format!(
        "{} tasks, {} pending, {} running, {} succeeded, {} failed, {} canceled",
        tasks.len(),
        pending,
        running,
        succeeded,
        failed,
        canceled
    );
    if selected {
        label = format!("[{label}] Enter to view");
    }
    label
}
