//! Rust-side helper for assistant thinking messages.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

#[derive(Debug, Clone)]
pub struct AssistantThinkingView {
    pub thinking: String,
    pub verbose: bool,
    pub is_transcript_mode: bool,
}

pub fn render_assistant_thinking_lines(
    view: &AssistantThinkingView,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let body = view.thinking.trim();
    if body.is_empty() || (!view.verbose && !view.is_transcript_mode) {
        return Vec::new();
    }

    let mut lines = vec![Line::from(Span::styled("∴ Thinking…", theme.thinking))];
    lines.extend(
        body.lines()
            .map(|line| Line::from(Span::styled(format!("  {line}"), theme.thinking))),
    );
    lines
}

#[cfg(test)]
pub fn render_assistant_thinking_message(thinking: &str, _theme: &Theme) -> String {
    let body = thinking.trim();
    if body.is_empty() {
        return "Assistant thinking: <empty>".to_string();
    }

    let preview_len = body.chars().take(120).collect::<String>();
    if body.len() > preview_len.len() {
        format!("Assistant thinking: {preview_len}...")
    } else {
        format!("Assistant thinking: {body}")
    }
}
