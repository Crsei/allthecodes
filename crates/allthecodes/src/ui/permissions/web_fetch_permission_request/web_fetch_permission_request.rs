//! Web-fetch permission request rendering.

use crate::ui::permissions::utils::{
    default_permission_options, render_permission_request, PermissionRequestView,
};

pub fn render_web_fetch_permission_request(
    url: &str,
    method: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("Web fetch permission", "web_fetch", url)
        .with_detail(format!("method: {method}"))
        .with_risk("network access")
        .with_options(default_permission_options(), selected_index);
    render_permission_request(&view)
}
