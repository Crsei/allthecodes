//! Workflow detail dialog.

use super::task_status_utils::state_label;
use super::TaskStatus;

pub fn render_workflow_detail_dialog(tasks: &[TaskStatus], workflow_name: &str) -> String {
    let mut lines = vec![format!("Workflow: {workflow_name}")];
    for task in tasks {
        lines.push(format!(
            "- {:<18} {:<9} {}",
            task.title,
            state_label(task.state),
            task.summary
        ));
    }
    lines.join("\n")
}
