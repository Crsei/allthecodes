//! Monitor permission request rendering.

use crate::ui::permissions::utils::{
    default_permission_options, render_permission_request, PermissionRequestView,
};

pub fn render_monitor_permission_request(
    process_name: &str,
    duration_seconds: u64,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("Monitor permission", "monitor", process_name)
        .with_detail(format!("duration: {duration_seconds}s"))
        .with_risk("background observation of process output")
        .with_options(default_permission_options(), selected_index);
    render_permission_request(&view)
}
