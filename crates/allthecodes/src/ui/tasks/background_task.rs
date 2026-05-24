//! One-line background task row.

use super::task_status_utils::{format_elapsed, kind_label, progress_bar, state_label};
use super::TaskStatus;

pub fn render_background_task(task: &TaskStatus, selected: bool) -> String {
    let marker = if selected { ">" } else { " " };
    format!(
        "{marker} ● {:<18} {:<9} {:<9} worked for {:>6} [{}] {}",
        task.title,
        kind_label(task.kind),
        state_label(task.state),
        format_elapsed(task.elapsed_ms),
        progress_bar(task, 10),
        task.summary
    )
}
