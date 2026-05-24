//! Rust-side helper for compact-boundary system messages.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn render_compact_boundary_lines(theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        "✻ Conversation compacted (ctrl+o for history)",
        theme.dim,
    ))]
}

#[cfg(test)]
pub fn render_compact_boundary_message(
    before_tokens: usize,
    after_tokens: usize,
    _theme: &Theme,
) -> String {
    format!("Context compacted: before={before_tokens} after={after_tokens}")
}
