//! Notebook edit permission request rendering.

use super::notebook_edit_tool_diff::render_notebook_edit_tool_diff;
use crate::ui::permissions::file_permission_dialog::permission_options::file_permission_options;
use crate::ui::permissions::utils::{render_permission_request, PermissionRequestView};

pub fn render_notebook_edit_permission_request(
    path: &str,
    cell_index: usize,
    language: &str,
    selected_index: usize,
) -> String {
    let view = PermissionRequestView::new("Notebook edit permission", "notebook_edit", path)
        .with_detail(render_notebook_edit_tool_diff(path, cell_index, language))
        .with_options(file_permission_options(path, false), selected_index);
    render_permission_request(&view)
}
