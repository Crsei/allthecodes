//! Rust-side helper for collapsed read/search content.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Default)]
pub struct CollapsedReadSearchView {
    pub read_count: usize,
    pub search_count: usize,
    pub list_count: usize,
    pub active: bool,
    pub latest_hint: Option<String>,
}

pub fn render_collapsed_read_search_lines(
    view: &CollapsedReadSearchView,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut parts = Vec::new();
    if view.search_count > 0 {
        parts.push(format!(
            "{} {} {}",
            if view.active {
                "Searching for"
            } else {
                "Searched for"
            },
            view.search_count,
            if view.search_count == 1 {
                "pattern"
            } else {
                "patterns"
            }
        ));
    }
    if view.read_count > 0 {
        parts.push(format!(
            "{} {} {}",
            if view.active { "Reading" } else { "Read" },
            view.read_count,
            if view.read_count == 1 {
                "file"
            } else {
                "files"
            }
        ));
    }
    if view.list_count > 0 {
        parts.push(format!(
            "{} {} {}",
            if view.active { "Listing" } else { "Listed" },
            view.list_count,
            if view.list_count == 1 {
                "directory"
            } else {
                "directories"
            }
        ));
    }
    if parts.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{}{} Ctrl+O to expand",
            parts.join(", "),
            if view.active { "…" } else { "" }
        ),
        theme.dim,
    ))];
    if view.active {
        if let Some(hint) = view.latest_hint.as_deref().filter(|hint| !hint.is_empty()) {
            lines.extend(
                hint.lines()
                    .map(|line| Line::from(Span::styled(format!("  ⎿  {line}"), theme.dim))),
            );
        }
    }
    lines
}

#[cfg(test)]
pub fn render_collapsed_read_search_content(
    source: &str,
    line_count: usize,
    _theme: &Theme,
) -> String {
    if line_count == 0 {
        format!("Read {source}: no lines")
    } else {
        format!("Read {source}: {line_count} lines (collapsed)")
    }
}
