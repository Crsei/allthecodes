//! Workflow detail dialog.

use super::task_status_utils::state_label;
use super::{TaskKind, TaskStatus};

pub fn render_workflow_detail_dialog(tasks: &[TaskStatus], workflow_name: &str) -> String {
    if let Some(task) = tasks.iter().find(|task| {
        task.kind == TaskKind::Workflow && (task.title == workflow_name || task.id == workflow_name)
    }) {
        return render_selected_workflow(task);
    }

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

fn render_selected_workflow(task: &TaskStatus) -> String {
    let mut lines = vec![
        format!("Workflow: {}", task.title),
        format!("Run id: {}", task.id),
        format!("State: {}", state_label(task.state)),
    ];
    if !task.summary.trim().is_empty() {
        lines.push(format!("Summary: {}", task.summary));
    }
    if let Some((done, total)) = task.progress {
        lines.push(format!("Progress: {done}/{total} steps"));
    }
    if !task.output_lines.is_empty() {
        lines.push(String::new());
        lines.push("Recent output:".to_string());
        for line in task.output_lines.iter().take(14) {
            lines.push(format!("- {line}"));
        }
    }
    lines.join("\n")
}
