//! Generic permission request surface.

use super::utils::{render_permission_request, PermissionRequestView};

pub fn render_permission_request_surface(view: &PermissionRequestView) -> String {
    render_permission_request(view)
}

pub fn basic_permission_request(tool_name: &str, summary: &str) -> PermissionRequestView {
    PermissionRequestView::new("Permission required", tool_name, summary)
}
