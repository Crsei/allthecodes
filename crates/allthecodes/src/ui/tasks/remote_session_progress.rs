//! Remote session progress rendering.

use super::task_status_utils::state_label;
use super::TaskStatus;

pub fn render_remote_session_progress(task: &TaskStatus) -> String {
    format!(
        "remote: {}\nstate: {}\n{}",
        task.title,
        state_label(task.state),
        task.summary
    )
}
