//! Worker pending permission notification rendering.

use super::worker_badge::render_worker_badge;

pub fn render_worker_pending_permission(
    worker_name: &str,
    tool_name: &str,
    summary: &str,
    queue_position: usize,
) -> String {
    format!(
        "{}\ntool: {}\nrequest: {}\nqueue position: {}",
        render_worker_badge(worker_name, 1),
        tool_name,
        summary,
        queue_position
    )
}
