//! Filesystem permission request rendering.

use crate::ui::permissions::utils::{
    default_permission_options, path_action_summary, render_permission_request,
    PermissionRequestView,
};

pub fn render_filesystem_permission_request(
    action: &str,
    path: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new(
        "Filesystem permission",
        "filesystem",
        path_action_summary(action, path),
    )
    .with_risk("filesystem access can modify or disclose local files")
    .with_options(default_permission_options(), selected_index);
    render_permission_request(&view)
}
