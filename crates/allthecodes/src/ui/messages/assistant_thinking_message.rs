//! Rust-side helper for assistant thinking messages.

use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const THINKING_LABEL: &str = "∴ Thinking…";

#[derive(Debug, Clone)]
pub struct AssistantThinkingView {
    pub thinking: String,
    pub verbose: bool,
    pub is_transcript_mode: bool,
    pub animation_frame: Option<usize>,
}

pub fn render_assistant_thinking_lines(
    view: &AssistantThinkingView,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let body = view.thinking.trim();
    if body.is_empty() || (!view.verbose && !view.is_transcript_mode) {
        return Vec::new();
    }

    let animation_frame = if view.is_transcript_mode {
        None
    } else {
        view.animation_frame
    };
    let mut lines = vec![render_thinking_label(animation_frame, theme)];
    lines.extend(
        body.lines()
            .map(|line| Line::from(Span::styled(format!("  {line}"), theme.thinking))),
    );
    lines
}

pub fn render_thinking_label(frame: Option<usize>, theme: &Theme) -> Line<'static> {
    let Some(frame) = frame else {
        return Line::from(Span::styled(THINKING_LABEL, theme.thinking));
    };

    let chars = THINKING_LABEL.chars().collect::<Vec<_>>();
    let len = chars.len();
    let head = frame % (len + 3);
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;

    for (pos, ch) in chars.into_iter().enumerate() {
        let style = shimmer_style_for(pos, head, theme);
        if current_style == Some(style) {
            current_text.push(ch);
            continue;
        }

        if let Some(style) = current_style.replace(style) {
            spans.push(Span::styled(std::mem::take(&mut current_text), style));
        }
        current_text.push(ch);
    }

    if let Some(style) = current_style {
        spans.push(Span::styled(current_text, style));
    }

    Line::from(spans)
}

fn shimmer_style_for(pos: usize, head: usize, theme: &Theme) -> Style {
    match pos.abs_diff(head) {
        0 => theme.info.add_modifier(Modifier::BOLD),
        1 => theme.thinking.add_modifier(Modifier::BOLD),
        _ => theme.thinking,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn span_styles(line: &Line<'_>) -> Vec<Style> {
        line.spans.iter().map(|span| span.style).collect()
    }

    #[test]
    fn thinking_label_without_frame_is_static() {
        let theme = Theme::default();
        let line = render_thinking_label(None, &theme);

        assert_eq!(line_text(&line), THINKING_LABEL);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style, theme.thinking);
    }

    #[test]
    fn thinking_label_animation_changes_span_styles() {
        let theme = Theme::default();
        let frame_zero = render_thinking_label(Some(0), &theme);
        let frame_one = render_thinking_label(Some(1), &theme);

        assert_eq!(line_text(&frame_zero), THINKING_LABEL);
        assert_eq!(line_text(&frame_one), THINKING_LABEL);
        assert_ne!(span_styles(&frame_zero), span_styles(&frame_one));
    }

    #[test]
    fn transcript_mode_forces_static_thinking_label() {
        let theme = Theme::default();
        let lines = render_assistant_thinking_lines(
            &AssistantThinkingView {
                thinking: "inspect files".to_string(),
                verbose: false,
                is_transcript_mode: true,
                animation_frame: Some(3),
            },
            &theme,
        );

        assert_eq!(line_text(&lines[0]), THINKING_LABEL);
        assert_eq!(lines[0].spans.len(), 1);
        assert_eq!(lines[0].spans[0].style, theme.thinking);
    }
}
