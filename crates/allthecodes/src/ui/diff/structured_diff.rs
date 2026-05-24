use ratatui::text::{Line, Span};

use super::truncate_by_width;
use crate::ui::syntax_highlight::highlight_code_block;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredDiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub section: Option<String>,
    pub lines: Vec<StructuredDiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredDiffLineKind {
    Context,
    Add,
    Remove,
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredDiffLine {
    pub kind: StructuredDiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

impl StructuredDiffHunk {
    pub fn header(&self) -> String {
        let old_range = format_range(self.old_start, self.old_lines);
        let new_range = format_range(self.new_start, self.new_lines);
        match self
            .section
            .as_deref()
            .filter(|section| !section.is_empty())
        {
            Some(section) => format!("@@ -{old_range} +{new_range} @@ {section}"),
            None => format!("@@ -{old_range} +{new_range} @@"),
        }
    }
}

fn format_range(start: usize, lines: usize) -> String {
    if lines == 1 {
        start.to_string()
    } else {
        format!("{start},{lines}")
    }
}

/// Parse unified-diff hunk headers and their following body lines.
///
/// The input may include file headers (`diff --git`, `index`, `---`, `+++`) or
/// other patch metadata. Those lines are ignored until an `@@ ... @@` hunk
/// header is encountered.
pub fn parse_structured_hunks(lines: &[String]) -> Vec<StructuredDiffHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<StructuredDiffHunk> = None;
    let mut old_line = 0usize;
    let mut new_line = 0usize;

    for raw_line in lines {
        if let Some((old_start, old_lines, new_start, new_lines, section)) =
            parse_hunk_header(raw_line)
        {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(StructuredDiffHunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
                section,
                lines: Vec::new(),
            });
            old_line = old_start;
            new_line = new_start;
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        if raw_line.starts_with("\\ No newline at end of file") {
            hunk.lines.push(StructuredDiffLine {
                kind: StructuredDiffLineKind::NoNewline,
                old_line: None,
                new_line: None,
                content: raw_line.clone(),
            });
            continue;
        }

        let Some((prefix, content)) = split_diff_line(raw_line) else {
            continue;
        };
        match prefix {
            ' ' => {
                hunk.lines.push(StructuredDiffLine {
                    kind: StructuredDiffLineKind::Context,
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    content: content.to_string(),
                });
                old_line += 1;
                new_line += 1;
            }
            '+' => {
                hunk.lines.push(StructuredDiffLine {
                    kind: StructuredDiffLineKind::Add,
                    old_line: None,
                    new_line: Some(new_line),
                    content: content.to_string(),
                });
                new_line += 1;
            }
            '-' => {
                hunk.lines.push(StructuredDiffLine {
                    kind: StructuredDiffLineKind::Remove,
                    old_line: Some(old_line),
                    new_line: None,
                    content: content.to_string(),
                });
                old_line += 1;
            }
            _ => {}
        }
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }

    hunks
}

pub fn render_structured_diff_hunks(
    hunks: &[StructuredDiffHunk],
    width: usize,
    max_lines: usize,
) -> Vec<String> {
    render_structured_diff_hunks_styled(hunks, width, max_lines, &Theme::default(), None)
        .into_iter()
        .map(line_to_plain)
        .collect()
}

fn render_structured_prefix(line: &StructuredDiffLine, number_width: usize) -> String {
    if line.kind == StructuredDiffLineKind::NoNewline {
        let gutter_width = number_width * 2 + 4;
        return format!("{:gutter_width$}", "");
    }

    let old = format_optional_line(line.old_line, number_width);
    let new = format_optional_line(line.new_line, number_width);
    let sigil = match line.kind {
        StructuredDiffLineKind::Context => ' ',
        StructuredDiffLineKind::Add => '+',
        StructuredDiffLineKind::Remove => '-',
        StructuredDiffLineKind::NoNewline => ' ',
    };
    format!("{old} {new} {sigil} ")
}

fn format_optional_line(line: Option<usize>, width: usize) -> String {
    match line {
        Some(line) => format!("{line:>width$}"),
        None => " ".repeat(width),
    }
}

fn line_number_width(hunks: &[StructuredDiffHunk]) -> usize {
    let max_line = hunks
        .iter()
        .flat_map(|hunk| {
            [
                hunk.old_start
                    .saturating_add(hunk.old_lines.saturating_sub(1)),
                hunk.new_start
                    .saturating_add(hunk.new_lines.saturating_sub(1)),
            ]
        })
        .max()
        .unwrap_or(1);
    max_line.to_string().len().max(4)
}

fn split_diff_line(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let prefix = chars.next()?;
    if matches!(prefix, ' ' | '+' | '-') {
        Some((prefix, chars.as_str()))
    } else {
        None
    }
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize, Option<String>)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old_range, rest) = rest.split_once(" +")?;
    let (new_range, rest) = rest.split_once(" @@")?;
    let (old_start, old_lines) = parse_range(old_range)?;
    let (new_start, new_lines) = parse_range(new_range)?;
    let section = rest.trim_start().trim().to_string();
    let section = if section.is_empty() {
        None
    } else {
        Some(section)
    };
    Some((old_start, old_lines, new_start, new_lines, section))
}

fn parse_range(raw: &str) -> Option<(usize, usize)> {
    if let Some((start, lines)) = raw.split_once(',') {
        Some((start.parse().ok()?, lines.parse().ok()?))
    } else {
        Some((raw.parse().ok()?, 1))
    }
}

// ---------------------------------------------------------------------------
// Styled rendering (word-level diff + syntax highlighting)
// ---------------------------------------------------------------------------

/// Render structured diff hunks as styled ratatui lines with word-level
/// diff and syntax highlighting.
///
/// Each line gets appropriate diff coloring (add/remove/context) with
/// inline word-level highlights for changed lines. When a `lang` hint
/// is provided (e.g., from the file extension), syntax highlighting is
/// applied to context lines.
pub fn render_structured_diff_hunks_styled(
    hunks: &[StructuredDiffHunk],
    width: usize,
    max_lines: usize,
    theme: &Theme,
    lang: Option<&str>,
) -> Vec<Line<'static>> {
    if hunks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let number_width = line_number_width(hunks);
    let mut rendered = Vec::new();
    for (hunk_index, hunk) in hunks.iter().enumerate() {
        if hunk_index > 0 {
            if rendered.len() >= max_lines {
                break;
            }
            push_styled_truncated(
                &mut rendered,
                vec![Span::styled("...", theme.dim)],
                width,
                max_lines,
            );
        }

        if rendered.len() < max_lines {
            push_styled_truncated(
                &mut rendered,
                vec![Span::styled(hunk.header(), theme.diff_header)],
                width,
                max_lines,
            );
        }

        let mut i = 0;
        while i < hunk.lines.len() && rendered.len() < max_lines {
            let line = &hunk.lines[i];

            match line.kind {
                StructuredDiffLineKind::Context => {
                    push_styled_truncated(
                        &mut rendered,
                        render_context_line_spans(line, number_width, theme, lang),
                        width,
                        max_lines,
                    );
                    i += 1;
                }
                StructuredDiffLineKind::Add => {
                    if i + 1 < hunk.lines.len()
                        && hunk.lines[i + 1].kind == StructuredDiffLineKind::Remove
                    {
                        let remove_line = &hunk.lines[i + 1];
                        let add_line = &hunk.lines[i];
                        push_paired_change_lines(
                            &mut rendered,
                            remove_line,
                            add_line,
                            number_width,
                            width,
                            max_lines,
                            theme,
                        );
                        i += 2;
                    } else {
                        push_styled_truncated(
                            &mut rendered,
                            render_structured_line_spans(line, number_width, theme),
                            width,
                            max_lines,
                        );
                        i += 1;
                    }
                }
                StructuredDiffLineKind::Remove => {
                    if i + 1 < hunk.lines.len()
                        && hunk.lines[i + 1].kind == StructuredDiffLineKind::Add
                    {
                        let remove_line = &hunk.lines[i];
                        let add_line = &hunk.lines[i + 1];
                        push_paired_change_lines(
                            &mut rendered,
                            remove_line,
                            add_line,
                            number_width,
                            width,
                            max_lines,
                            theme,
                        );
                        i += 2;
                    } else {
                        push_styled_truncated(
                            &mut rendered,
                            render_structured_line_spans(line, number_width, theme),
                            width,
                            max_lines,
                        );
                        i += 1;
                    }
                }
                StructuredDiffLineKind::NoNewline => {
                    push_styled_truncated(
                        &mut rendered,
                        render_structured_line_spans(line, number_width, theme),
                        width,
                        max_lines,
                    );
                    i += 1;
                }
            }
        }

        if rendered.len() >= max_lines {
            break;
        }
    }

    rendered
}

fn push_paired_change_lines(
    rendered: &mut Vec<Line<'static>>,
    remove_line: &StructuredDiffLine,
    add_line: &StructuredDiffLine,
    number_width: usize,
    width: usize,
    max_lines: usize,
    theme: &Theme,
) {
    if rendered.len() < max_lines {
        let mut remove_spans = vec![Span::styled(
            render_structured_prefix(remove_line, number_width),
            theme.diff_remove,
        )];
        remove_spans.extend(super::render_word_diff_line(
            &remove_line.content,
            &add_line.content,
            false,
            theme,
        ));
        push_styled_truncated(rendered, remove_spans, width, max_lines);
    }

    if rendered.len() < max_lines {
        let mut add_spans = vec![Span::styled(
            render_structured_prefix(add_line, number_width),
            theme.diff_add,
        )];
        add_spans.extend(super::render_word_diff_line(
            &remove_line.content,
            &add_line.content,
            true,
            theme,
        ));
        push_styled_truncated(rendered, add_spans, width, max_lines);
    }
}

fn render_context_line_spans(
    line: &StructuredDiffLine,
    number_width: usize,
    theme: &Theme,
    lang: Option<&str>,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        render_structured_prefix(line, number_width),
        theme.diff_context,
    )];
    let content_spans = if let Some(lang) = lang {
        let highlighted = highlight_code_block(&line.content, lang, theme)
            .into_iter()
            .filter(|span| span.content.as_ref() != "\n")
            .collect::<Vec<_>>();
        if highlighted.is_empty() {
            vec![Span::styled(line.content.clone(), theme.diff_context)]
        } else {
            highlighted
        }
    } else {
        vec![Span::styled(line.content.clone(), theme.diff_context)]
    };
    spans.extend(content_spans);
    spans
}

fn render_structured_line_spans(
    line: &StructuredDiffLine,
    number_width: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let style = match line.kind {
        StructuredDiffLineKind::Context => theme.diff_context,
        StructuredDiffLineKind::Add => theme.diff_add,
        StructuredDiffLineKind::Remove => theme.diff_remove,
        StructuredDiffLineKind::NoNewline => theme.dim,
    };
    vec![
        Span::styled(render_structured_prefix(line, number_width), style),
        Span::styled(line.content.clone(), style),
    ]
}

fn push_styled_truncated(
    lines: &mut Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    width: usize,
    max_lines: usize,
) {
    if lines.len() >= max_lines {
        return;
    }
    lines.push(Line::from(truncate_spans_by_width(spans, width)));
}

fn truncate_spans_by_width(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let mut remaining_width = width;
    let mut out = Vec::new();
    for span in spans {
        if remaining_width == 0 {
            break;
        }
        let text = truncate_by_width(span.content.as_ref(), remaining_width);
        if !text.is_empty() {
            let text_width = text
                .chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                .sum::<usize>();
            remaining_width = remaining_width.saturating_sub(text_width);
            out.push(Span::styled(text, span.style));
        }
    }
    out
}

fn line_to_plain(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_structured_hunks, render_structured_diff_hunks, StructuredDiffLineKind};
    use insta::assert_snapshot;

    #[test]
    fn parses_hunk_header_ranges_and_lines() {
        let input = vec![
            "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
            "@@ -10,2 +10,3 @@ fn demo()".to_string(),
            " context".to_string(),
            "-old".to_string(),
            "+new".to_string(),
            "+extra".to_string(),
            "\\ No newline at end of file".to_string(),
        ];

        let hunks = parse_structured_hunks(&input);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 10);
        assert_eq!(hunks[0].old_lines, 2);
        assert_eq!(hunks[0].new_start, 10);
        assert_eq!(hunks[0].new_lines, 3);
        assert_eq!(hunks[0].section.as_deref(), Some("fn demo()"));
        assert_eq!(hunks[0].lines[0].old_line, Some(10));
        assert_eq!(hunks[0].lines[0].new_line, Some(10));
        assert_eq!(hunks[0].lines[1].kind, StructuredDiffLineKind::Remove);
        assert_eq!(hunks[0].lines[1].old_line, Some(11));
        assert_eq!(hunks[0].lines[2].kind, StructuredDiffLineKind::Add);
        assert_eq!(hunks[0].lines[2].new_line, Some(11));
        assert_eq!(hunks[0].lines[3].new_line, Some(12));
        assert_eq!(hunks[0].lines[4].kind, StructuredDiffLineKind::NoNewline);
    }

    #[test]
    fn parses_single_line_ranges_and_multiple_hunks() {
        let input = vec![
            "@@ -1 +1 @@".to_string(),
            "-old".to_string(),
            "+new".to_string(),
            "@@ -20,0 +21,2 @@ inserted".to_string(),
            "+alpha".to_string(),
            "+beta".to_string(),
        ];

        let hunks = parse_structured_hunks(&input);

        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_lines, 1);
        assert_eq!(hunks[0].new_lines, 1);
        assert_eq!(hunks[1].old_start, 20);
        assert_eq!(hunks[1].old_lines, 0);
        assert_eq!(hunks[1].new_start, 21);
        assert_eq!(hunks[1].new_lines, 2);
    }

    #[test]
    fn ignores_raw_lines_without_hunk_headers() {
        let input = vec!["+legacy line".to_string(), "-legacy old".to_string()];

        assert!(parse_structured_hunks(&input).is_empty());
    }

    #[test]
    fn snapshot_renders_structured_hunks_with_old_and_new_gutters() {
        let input = vec![
            "@@ -1,3 +1,4 @@ fn main".to_string(),
            " fn main() {".to_string(),
            "-    old_call();".to_string(),
            "+    new_call();".to_string(),
            "+    extra_call();".to_string(),
            " }".to_string(),
            "@@ -20,1 +21,1 @@".to_string(),
            "-let stale = true;".to_string(),
            "+let stale = false;".to_string(),
            "\\ No newline at end of file".to_string(),
        ];
        let hunks = parse_structured_hunks(&input);

        assert_snapshot!(render_structured_diff_hunks(&hunks, 80, 100).join("\n"));
    }
}
