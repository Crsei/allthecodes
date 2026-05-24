//! File edit update message rendering.

use crate::ui::diff::file_edit_diff::{
    file_edit_diff_stats, format_file_edit_summary, render_file_edit_diff_preview,
    structured_file_edit_hunks,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEditMessageStyle {
    Regular,
    Condensed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEditToolUpdatedView {
    pub file_path: String,
    pub hunk_lines: Vec<String>,
    pub style: FileEditMessageStyle,
    pub verbose: bool,
    pub preview_hint: Option<String>,
    pub width: usize,
    pub max_lines: usize,
}

pub fn render_file_edit_tool_updated_message(view: &FileEditToolUpdatedView) -> String {
    let hunks = structured_file_edit_hunks(&view.hunk_lines);
    let summary = format_file_edit_summary(file_edit_diff_stats(&hunks));

    if let Some(hint) = &view.preview_hint {
        if view.style != FileEditMessageStyle::Condensed && !view.verbose {
            return hint.clone();
        }
    } else if view.style == FileEditMessageStyle::Condensed && !view.verbose {
        return summary;
    }

    let mut lines = vec![summary];
    lines.extend(render_file_edit_diff_preview(
        &view.file_path,
        &view.hunk_lines,
        view.width,
        view.max_lines,
    ));
    lines.join("\n")
}

#[cfg(test)]
pub fn render_file_edit_tool_rejected_message(path: &str, reason: Option<&str>) -> String {
    match reason {
        Some(reason) if !reason.trim().is_empty() => {
            format!("File edit rejected: {path}\nreason: {reason}")
        }
        _ => format!("File edit rejected: {path}"),
    }
}

#[cfg(test)]
pub fn render_file_edit_tool_canceled_message(path: &str) -> String {
    format!("File edit canceled: {path}")
}
