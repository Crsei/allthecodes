//! Sed-edit permission request rendering.

use crate::ui::permissions::file_permission_dialog::permission_options::file_permission_options;
use crate::ui::permissions::utils::{render_permission_request, PermissionRequestView};

pub fn render_sed_edit_permission_request(
    path: &str,
    pattern: &str,
    replacement: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("Sed edit permission", "sed", path)
        .with_detail(format!("pattern: {pattern}"))
        .with_detail(format!("replacement: {replacement}"))
        .with_options(file_permission_options(path, false), selected_index);
    render_permission_request(&view)
}
