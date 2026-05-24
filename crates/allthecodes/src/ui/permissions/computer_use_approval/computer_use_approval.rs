//! Computer-use approval rendering.

use crate::ui::permissions::utils::{render_permission_request, PermissionRequestView};

pub fn render_computer_use_approval(
    action: &str,
    target: &str,
    screenshot_available: bool,
) -> String {
    let view = PermissionRequestView::new("Computer use approval", "computer", action)
        .with_detail(format!("target: {target}"))
        .with_detail(format!("screenshot: {screenshot_available}"))
        .with_risk("interactive desktop action");
    render_permission_request(&view)
}
