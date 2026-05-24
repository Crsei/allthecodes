//! File edit diff rendering for permission prompts.

use crate::ui::diff::file_edit_diff::render_file_edit_diff_preview;

pub fn render_file_edit_tool_diff(
    path: &str,
    hunk_lines: &[String],
    width: usize,
    max_lines: usize,
) -> String {
    render_file_edit_diff_preview(path, hunk_lines, width, max_lines).join("\n")
}
