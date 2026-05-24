//! Shell task progress rendering.

use super::task_status_utils::{format_elapsed, progress_bar, progress_detail, state_label};
use super::TaskStatus;

pub fn render_shell_progress(task: &TaskStatus) -> String {
    let mut lines = vec![format!(
        "$ {}\nstate: {}\nelapsed: {}\nprogress: [{}] {}",
        task.title,
        state_label(task.state),
        format_elapsed(task.elapsed_ms),
        progress_bar(task, 20),
        progress_detail(task)
    )];
    if !task.summary.trim().is_empty() {
        lines.push(format!("summary: {}", task.summary));
    }
    if !task.output_lines.is_empty() {
        lines.push(format!(
            "recent output ({} retained):",
            task.output_lines.len()
        ));
        lines.extend(
            task.output_lines
                .iter()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|line| format!("  {line}")),
        );
    }
    lines.join("\n")
}
