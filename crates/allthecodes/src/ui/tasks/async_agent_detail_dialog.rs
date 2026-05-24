//! Async agent task detail dialog.

use super::task_status_utils::task_header;
use super::TaskStatus;

pub fn render_async_agent_detail_dialog(task: &TaskStatus) -> String {
    let mut lines = vec![task_header(task), format!("agent result: {}", task.summary)];
    for line in &task.output_lines {
        lines.push(format!("  {line}"));
    }
    lines.join("\n")
}
