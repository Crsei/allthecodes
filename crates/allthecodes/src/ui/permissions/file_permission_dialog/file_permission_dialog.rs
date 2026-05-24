//! File permission dialog rendering.

use super::permission_options::file_permission_options;
use super::use_file_permission_dialog::FilePermissionDialogState;
use crate::ui::permissions::utils::{
    path_action_summary, render_permission_request, PermissionRequestView,
};

pub fn render_file_permission_dialog(
    state: &FilePermissionDialogState,
    action: &str,
    diff_summary: &str,
) -> String {
    let mut view = PermissionRequestView::new(
        "File permission",
        "file",
        path_action_summary(action, &state.path),
    )
    .with_options(
        file_permission_options(&state.path, state.persistent_rule),
        state.selected_index,
    );
    if state.show_diff {
        view = view.with_detail(format!("diff: {diff_summary}"));
    }
    render_permission_request(&view)
}
