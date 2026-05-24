//! Worker badge rendering for delegated permission requests.

pub fn render_worker_badge(worker_name: &str, pending_count: usize) -> String {
    if pending_count == 0 {
        format!("{worker_name}: no pending permissions")
    } else {
        format!("{worker_name}: {pending_count} pending permission(s)")
    }
}
