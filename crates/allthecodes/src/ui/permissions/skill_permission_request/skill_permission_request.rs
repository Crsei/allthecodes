//! Skill permission request rendering.

use crate::ui::permissions::utils::{
    default_permission_options, render_permission_request, PermissionRequestView,
};

pub fn render_skill_permission_request(
    skill_name: &str,
    action: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("Skill permission", "skill", skill_name)
        .with_detail(format!("action: {action}"))
        .with_options(default_permission_options(), selected_index);
    render_permission_request(&view)
}
