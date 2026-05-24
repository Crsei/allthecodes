//! Rust-side helper for redacted assistant thinking messages.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn render_assistant_redacted_thinking_lines(theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled("✻ Thinking…", theme.thinking))]
}

#[cfg(test)]
pub fn render_assistant_redacted_thinking_message(_theme: &Theme) -> String {
    "Assistant thinking was redacted by policy".to_string()
}
