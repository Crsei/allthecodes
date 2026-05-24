// test infrastructure — markdown render helpers for future transcript integration
//! Markdown-to-ratatui rendering facade.

use std::path::Path;

use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthChar;

use super::markdown::markdown_to_lines;
use super::theme::Theme;

pub fn render_markdown_text(input: &str) -> Text<'static> {
    render_markdown_text_with_theme(input, &Theme::default())
}

pub fn render_markdown_text_with_theme(input: &str, theme: &Theme) -> Text<'static> {
    Text::from(markdown_to_lines(input, theme))
}

pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    render_markdown_text_with_width_and_cwd(input, width, None)
}

pub(crate) fn render_markdown_text_with_width_and_cwd(
    input: &str,
    width: Option<usize>,
    _cwd: Option<&Path>,
) -> Text<'static> {
    let theme = Theme::default();
    let lines = markdown_to_lines(input, &theme);
    match width {
        Some(width) if width > 0 => Text::from(wrap_lines(lines, width)),
        _ => Text::from(lines),
    }
}

/// A word with an associated style, used during style-preserving word wrap.
struct StyledWord {
    text: String,
    style: ratatui::style::Style,
}

/// Wrap styled lines at a given width, preserving per-span styles.
///
/// Each line is split into words (by whitespace), preserving the style of each
/// word from its original span. Words are then reflowed into new lines that
/// fit within `width` columns, carrying the original styles forward.
fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();

    for line in lines {
        if line.spans.is_empty() {
            out.push(Line::default());
            continue;
        }

        // Extract words from the styled spans.
        let words = extract_words(&line);
        if words.is_empty() {
            out.push(Line::default());
            continue;
        }

        // Reflow words into lines.
        let wrapped = reflow_words(&words, width);
        out.extend(wrapped);
    }

    out
}

/// Split styled spans into words, preserving style per word.
fn extract_words(line: &Line<'static>) -> Vec<StyledWord> {
    let mut words = Vec::new();
    for span in &line.spans {
        let style = span.style;
        let mut current = String::new();
        for ch in span.content.chars() {
            if ch.is_whitespace() && ch != '\u{a0}' {
                if !current.is_empty() {
                    words.push(StyledWord {
                        text: std::mem::take(&mut current),
                        style,
                    });
                }
                // Preserve space characters between words.
                if ch == ' ' {
                    words.push(StyledWord {
                        text: " ".to_string(),
                        style,
                    });
                }
            } else {
                current.push(ch);
            }
        }
        if !current.is_empty() {
            words.push(StyledWord {
                text: current,
                style,
            });
        }
    }
    words
}

/// Reflow words into lines fitting the given width.
fn reflow_words(words: &[StyledWord], width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;
    let mut prev_was_space = false;

    for word in words {
        let word_width: usize = word
            .text
            .chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();

        if word.text == " " {
            if prev_was_space {
                // Collapse multiple spaces like HTML/Markdown
                continue;
            }
            // Check if adding a space would exceed width or we're at start of line
            if current_width == 0 {
                // Skip leading spaces
                continue;
            }
            if current_width + 1 > width {
                // The space is at the break point; start a new line
                flush_spans(&mut current_spans, &mut out);
                current_width = 0;
                prev_was_space = false;
                continue;
            }
            // Add the space
            current_spans.push(Span::styled(" ".to_string(), word.style));
            current_width += 1;
            prev_was_space = true;
            continue;
        }

        prev_was_space = false;

        if current_width + word_width > width && current_width > 0 {
            // Flush current line and start a new one
            flush_spans(&mut current_spans, &mut out);
            current_width = 0;
        }

        // If a single word is wider than the line width, it overflows
        // (same behavior as terminal emulators).
        current_spans.push(Span::styled(word.text.clone(), word.style));
        current_width += word_width;
    }

    flush_spans(&mut current_spans, &mut out);
    out
}

fn flush_spans(spans: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        out.push(Line::from(std::mem::take(spans)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    #[test]
    fn renders_markdown_lines() {
        let text = render_markdown_text("**bold**");
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn preserves_style_through_wrap() {
        let style = Style::default().fg(Color::Rgb(255, 0, 0));
        let line = Line::from(vec![
            Span::styled("hello ".to_string(), style),
            Span::styled("world".to_string(), style),
        ]);
        let wrapped = wrap_lines(vec![line], 8);
        // "hello " fits in 6 chars, "world" on next line
        assert_eq!(wrapped.len(), 2);
        // Both spans should still have the red style
        for wline in &wrapped {
            for span in &wline.spans {
                assert_eq!(span.style.fg, Some(Color::Rgb(255, 0, 0)));
            }
        }
    }

    #[test]
    fn width_wrappers_reflow_markdown_text() {
        let text = render_markdown_text_with_width("hello world", Some(6));
        assert_eq!(text.lines.len(), 2);

        let with_cwd = render_markdown_text_with_width_and_cwd(
            "hello world",
            Some(80),
            Some(Path::new("/repo")),
        );
        assert_eq!(with_cwd.lines.len(), 1);
    }
}
