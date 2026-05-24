//! Background tasks dialog.

use super::background_task::render_background_task;
use super::TaskStatus;

pub fn render_background_tasks_dialog(tasks: &[TaskStatus], selected_index: usize) -> String {
    let mut lines = vec![format!("Background tasks ({})", tasks.len())];
    if tasks.is_empty() {
        lines.push("No tasks".to_string());
    } else {
        for (idx, task) in tasks.iter().enumerate() {
            lines.push(render_background_task(task, idx == selected_index));
        }
    }
    lines.push("Enter details | k/s stop | d delete tool | r refresh | Esc close".to_string());
    lines.join("\n")
}
