//! Sandbox permission request rendering.

use super::utils::{render_permission_request, PermissionRequestView};

pub fn render_sandbox_permission_request(
    profile: &str,
    operation: &str,
    violations: &[impl AsRef<str>],
) -> String {
    let view = PermissionRequestView::new("Sandbox permission required", "sandbox", operation)
        .with_detail(format!("profile: {profile}"))
        .with_details(
            violations
                .iter()
                .map(|violation| violation.as_ref().to_string()),
        )
        .with_risk("operation is outside current sandbox policy");
    render_permission_request(&view)
}
