//! File edit permission request rendering.

use super::file_edit_tool_diff::render_file_edit_tool_diff;
use crate::ui::permissions::file_permission_dialog::permission_options::file_permission_options;
use crate::ui::permissions::utils::{render_permission_request, PermissionRequestView};

pub fn render_file_edit_permission_request(
    path: &str,
    operation: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("File edit permission", "edit", path)
        .with_detail(format!("operation: {operation}"))
        .with_options(file_permission_options(path, false), selected_index);
    render_permission_request(&view)
}

pub fn render_file_edit_permission_request_with_diff(
    path: &str,
    operation: &str,
    hunk_lines: &[String],
    selected_index: usize,
    width: usize,
) -> String {
    let detail = format!(
        "operation: {operation}\n{}",
        render_file_edit_tool_diff(path, hunk_lines, width, 24)
    );
    let view = PermissionRequestView::new("File edit permission", "edit", path)
        .with_detail(detail)
        .with_options(file_permission_options(path, false), selected_index);
    render_permission_request(&view)
}
