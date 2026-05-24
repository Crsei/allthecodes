//! Shell task detail dialog.

use super::task_status_utils::task_header;
use super::TaskStatus;

pub fn render_shell_detail_dialog(task: &TaskStatus) -> String {
    let mut lines = vec![task_header(task), "Output:".to_string()];
    if task.output_lines.is_empty() {
        lines.push("  <empty>".to_string());
    } else {
        for line in &task.output_lines {
            lines.push(format!("  {line}"));
        }
    }
    lines.join("\n")
}
