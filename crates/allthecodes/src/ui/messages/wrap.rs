use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) fn wrap_line_to_width(line: &Line<'_>, width: u16) -> Vec<Line<'static>> {
    let max_width = usize::from(width.max(1));
    if max_width == 0 {
        return vec![Line::default()];
    }

    let mut wrapped = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let (hanging_indent, hanging_width, hanging_style) = leading_indent(line);
    let mut needs_hanging_indent = false;

    for span in &line.spans {
        let style = span.style;
        let mut segment = String::new();

        for grapheme in span.content.graphemes(true) {
            let ch_width = UnicodeWidthStr::width(grapheme);
            let mut pending = true;
            while pending {
                if needs_hanging_indent && current_width == 0 {
                    // Keep a continuation indent only when at least one cell
                    // remains for the current grapheme. A wide grapheme must
                    // never be split, and retrying it after installing an
                    // indent would otherwise loop forever on a very narrow
                    // terminal.
                    if hanging_width.saturating_add(ch_width) <= max_width {
                        current_spans.push(Span::styled(hanging_indent.clone(), hanging_style));
                        current_width = hanging_width;
                    }
                    needs_hanging_indent = false;
                }

                if current_width > 0 && current_width + ch_width > max_width {
                    if !segment.is_empty() {
                        current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                    }
                    wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                    current_width = 0;
                    needs_hanging_indent = hanging_width > 0;
                    continue;
                }

                segment.push_str(grapheme);
                current_width += ch_width;
                pending = false;

                if current_width >= max_width {
                    current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                    wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                    current_width = 0;
                    needs_hanging_indent = hanging_width > 0;
                }
            }
        }

        if !segment.is_empty() {
            current_spans.push(Span::styled(segment, style));
        }
    }

    if current_spans.is_empty() {
        if wrapped.is_empty() {
            vec![Line::default()]
        } else {
            wrapped
        }
    } else {
        wrapped.push(Line::from(current_spans));
        wrapped
    }
}

fn leading_indent(line: &Line<'_>) -> (String, usize, ratatui::style::Style) {
    let style = line
        .spans
        .first()
        .map(|span| span.style)
        .unwrap_or_default();
    let mut indent = String::new();
    'spans: for span in &line.spans {
        for ch in span.content.chars() {
            if !matches!(ch, ' ' | '\t') {
                break 'spans;
            }
            indent.push(ch);
        }
    }
    let width = indent
        .chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum();
    (indent, width, style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn wrapped_user_line_keeps_hanging_indent() {
        let lines = wrap_line_to_width(&Line::from(Span::raw(" abcdef")), 4);
        assert_eq!(
            lines.iter().map(plain).collect::<Vec<_>>(),
            [" abc", " def"]
        );
    }

    #[test]
    fn hanging_indent_and_wide_text_stay_valid() {
        let lines = wrap_line_to_width(&Line::from(Span::raw(" 你中文abc")), 6);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .is_char_boundary(0)
        }));
        assert_eq!(plain(&lines[1]).chars().next(), Some(' '));
    }

    #[test]
    fn wrapping_does_not_split_combining_graphemes() {
        let lines = wrap_line_to_width(&Line::from(Span::raw("e\u{301}xyz")), 2);
        let rendered = lines.iter().map(plain).collect::<Vec<_>>();

        assert_eq!(rendered, vec!["e\u{301}x", "yz"]);
        assert!(rendered.iter().all(|line| !line.starts_with('\u{301}')));
    }

    #[test]
    fn narrow_hanging_indent_does_not_loop_on_wide_grapheme() {
        let lines = wrap_line_to_width(&Line::from(Span::raw(" 你")), 2);
        assert!(!lines.is_empty());
        assert_eq!(plain(&lines[1]), "你");
        assert!(lines.iter().all(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .is_char_boundary(0)
        }));
    }
}
