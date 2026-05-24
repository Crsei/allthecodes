//! File write diff summary rendering.

use crate::ui::diff::file_edit_diff::render_file_edit_diff_preview;

pub fn render_file_write_tool_diff(path: &str, new_lines: usize, replaced_lines: usize) -> String {
    format!("write diff: {path}\nnew lines: {new_lines}\nreplaced lines: {replaced_lines}")
}
pub fn render_file_write_tool_diff_from_hunks(
    path: &str,
    hunk_lines: &[String],
    width: usize,
    max_lines: usize,
) -> String {
    render_file_edit_diff_preview(path, hunk_lines, width, max_lines).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_from_hunks_renders_path_and_changes() {
        let rendered = render_file_write_tool_diff_from_hunks(
            "src/main.rs",
            &[
                "@@ -1 +1 @@".to_string(),
                "-old".to_string(),
                "+new".to_string(),
            ],
            80,
            8,
        );

        assert!(rendered.contains("src/main.rs"));
        assert!(rendered.contains("summary: Added 1 line, removed 1 line"));
        assert!(rendered.contains("@@ -1 +1 @@"));
    }
}
