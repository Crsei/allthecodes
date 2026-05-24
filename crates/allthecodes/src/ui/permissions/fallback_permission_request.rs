//! Fallback permission request rendering.

use super::utils::{render_permission_request, PermissionRequestView};

pub fn render_fallback_permission_request(
    tool_name: &str,
    summary: &str,
    details: &[impl AsRef<str>],
) -> String {
    let view = PermissionRequestView::new("Permission required", tool_name, summary)
        .with_details(details.iter().map(|detail| detail.as_ref().to_string()))
        .with_risk("unknown tool behavior");
    render_permission_request(&view)
}
