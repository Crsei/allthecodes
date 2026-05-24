use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

pub(super) fn wrap_line_to_width(line: &Line<'_>, width: u16) -> Vec<Line<'static>> {
    let max_width = usize::from(width.max(1));
    if max_width == 0 {
        return vec![Line::default()];
    }

    let mut wrapped = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in &line.spans {
        let style = span.style;
        let mut segment = String::new();

        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width > 0 && current_width + ch_width > max_width {
                if !segment.is_empty() {
                    current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                }
                wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
            }

            segment.push(ch);
            current_width += ch_width;

            if current_width >= max_width {
                current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
            }
        }

        if !segment.is_empty() {
            current_spans.push(Span::styled(segment, style));
        }
    }

    if current_spans.is_empty() {
        vec![Line::default()]
    } else {
        wrapped.push(Line::from(current_spans));
        wrapped
    }
}
