// BEGIN generated upstream diff modules
// Rust-side diff modules mirrored from upstream React components.
pub mod diff_detail_view;
pub mod diff_dialog;
pub mod diff_file_list;
pub mod file_edit_diff;
pub mod structured_diff;
// END generated upstream diff modules

use std::collections::HashMap;

#[cfg(test)]
use ratatui::buffer::Buffer;
#[cfg(test)]
use ratatui::layout::Rect;
#[cfg(test)]
use ratatui::text::Line;
use ratatui::text::Span;
use similar::{ChangeTag, TextDiff};
use unicode_width::UnicodeWidthChar;

#[cfg(test)]
use self::structured_diff::{parse_structured_hunks, StructuredDiffHunk};
use super::theme::Theme;

pub const MAX_VISIBLE_FILES: usize = 5;

/// Public metadata for one diffed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub is_binary: bool,
    pub is_large_file: bool,
    pub is_truncated: bool,
    pub is_untracked: bool,
}

impl DiffFile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: impl Into<String>,
        lines_added: usize,
        lines_removed: usize,
        is_binary: bool,
        is_large_file: bool,
        is_truncated: bool,
        is_untracked: bool,
    ) -> Self {
        Self {
            path: path.into(),
            lines_added,
            lines_removed,
            is_binary,
            is_large_file,
            is_truncated,
            is_untracked,
        }
    }
}

/// Aggregate diff stats displayed in dialog headings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStats {
    pub files_count: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

impl DiffStats {
    pub fn new(files_count: usize, lines_added: usize, lines_removed: usize) -> Self {
        Self {
            files_count,
            lines_added,
            lines_removed,
        }
    }
}

/// Parsed diff payload for the diff dialog surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffData {
    pub stats: Option<DiffStats>,
    pub files: Vec<DiffFile>,
    pub hunks: HashMap<String, Vec<String>>,
    pub loading: bool,
}

impl DiffData {
    pub fn empty() -> Self {
        Self {
            stats: None,
            files: Vec::new(),
            hunks: HashMap::new(),
            loading: false,
        }
    }

    pub fn hunks_for_path(&self, path: &str) -> &[String] {
        self.hunks.get(path).map_or(&[], |hunks| hunks.as_slice())
    }

    #[cfg(test)]
    pub fn structured_hunks_for_path(&self, path: &str) -> Vec<StructuredDiffHunk> {
        parse_structured_hunks(self.hunks_for_path(path))
    }
}

/// Shared truncation utility used by diff list/detail surfaces.
pub(crate) fn truncate_start_to_width(text: &str, max_width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_width {
        text.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        let start = chars.len() - (max_width - 3);
        format!("...{}", chars[start..].iter().collect::<String>())
    }
}

/// Truncate by terminal display width while preserving grapheme boundaries.
pub(crate) fn truncate_by_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut width = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

#[cfg(test)]
/// A single line from a unified diff.
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// Whether this line was added, removed, or is context.
    pub tag: ChangeTag,
    /// The text content of this line (without the leading +/- prefix).
    pub content: String,
}

#[cfg(test)]
/// Compute a line-level diff between `old` and `new` and return a list of
/// [`DiffLine`] entries.
pub fn format_diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();

    for change in diff.iter_all_changes() {
        lines.push(DiffLine {
            tag: change.tag(),
            content: change.value().trim_end_matches('\n').to_string(),
        });
    }

    lines
}

#[cfg(test)]
/// Render a unified diff between `old` and `new` into the given buffer area.
///
/// Each line is prefixed with `+`, `-`, or a space and colored accordingly.
/// The output is clipped to the available area height.
pub fn render_diff(old: &str, new: &str, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let diff_lines = format_diff_lines(old, new);
    let max_lines = area.height as usize;
    let max_width = area.width as usize;

    // Render a header line if there is room.
    let mut y_offset: u16 = 0;
    let mut old_line_no = 1usize;
    let mut new_line_no = 1usize;

    if !diff_lines.is_empty() {
        // Count additions and deletions for the header.
        let additions = diff_lines
            .iter()
            .filter(|l| l.tag == ChangeTag::Insert)
            .count();
        let deletions = diff_lines
            .iter()
            .filter(|l| l.tag == ChangeTag::Delete)
            .count();
        let header = format!("--- diff: +{} -{} lines ---", additions, deletions);
        let header_line = Line::from(Span::styled(header, theme.diff_header));
        buf.set_line(area.x, area.y + y_offset, &header_line, area.width);
        y_offset += 1;
    }

    let max_body_lines = max_lines.saturating_sub(1);
    let mut rendered_body_lines = 0usize;
    let mut index = 0usize;
    while index < diff_lines.len() && rendered_body_lines < max_body_lines {
        if y_offset >= area.height {
            break;
        }

        let diff_line = &diff_lines[index];
        let paired_insert = if diff_line.tag == ChangeTag::Delete {
            diff_lines
                .get(index + 1)
                .filter(|next| next.tag == ChangeTag::Insert)
        } else {
            None
        };

        if let Some(insert_line) = paired_insert {
            let content_width = max_width.saturating_sub(12);
            let remove_spans = truncate_spans_by_width(
                render_word_diff_line(&diff_line.content, &insert_line.content, false, theme),
                content_width,
            );
            let remove_line = build_diff_line(
                vec![
                    Span::styled(format!("{:>4}", old_line_no), theme.dim),
                    Span::styled(format!("{:>4}", ""), theme.dim),
                ],
                "-",
                theme.diff_remove,
                remove_spans,
            );
            buf.set_line(area.x, area.y + y_offset, &remove_line, area.width);
            y_offset += 1;
            rendered_body_lines += 1;
            old_line_no += 1;

            if y_offset >= area.height || rendered_body_lines >= max_body_lines {
                break;
            }

            let add_spans = truncate_spans_by_width(
                render_word_diff_line(&diff_line.content, &insert_line.content, true, theme),
                content_width,
            );
            let add_line = build_diff_line(
                vec![
                    Span::styled(format!("{:>4}", ""), theme.dim),
                    Span::styled(format!("{:>4}", new_line_no), theme.dim),
                ],
                "+",
                theme.diff_add,
                add_spans,
            );
            buf.set_line(area.x, area.y + y_offset, &add_line, area.width);
            y_offset += 1;
            rendered_body_lines += 1;
            new_line_no += 1;
            index += 2;
            continue;
        }

        let (prefix, style, line_no_spans) = match diff_line.tag {
            ChangeTag::Insert => {
                let spans = vec![
                    Span::styled(format!("{:>4}", ""), theme.dim),
                    Span::styled(format!("{:>4}", new_line_no), theme.dim),
                ];
                new_line_no += 1;
                ("+", theme.diff_add, spans)
            }
            ChangeTag::Delete => {
                let spans = vec![
                    Span::styled(format!("{:>4}", old_line_no), theme.dim),
                    Span::styled(format!("{:>4}", ""), theme.dim),
                ];
                old_line_no += 1;
                ("-", theme.diff_remove, spans)
            }
            ChangeTag::Equal => {
                let spans = vec![
                    Span::styled(format!("{:>4}", old_line_no), theme.dim),
                    Span::styled(format!("{:>4}", new_line_no), theme.dim),
                ];
                old_line_no += 1;
                new_line_no += 1;
                (" ", theme.diff_context, spans)
            }
        };

        // Truncate content to fit within the available width without
        // splitting multi-byte characters.
        let prefix_width = 12usize;
        let content_width = max_width.saturating_sub(prefix_width);
        let content = truncate_by_width(&diff_line.content, content_width);

        let line = build_diff_line(
            line_no_spans,
            prefix,
            style,
            vec![Span::styled(content, style)],
        );

        buf.set_line(area.x, area.y + y_offset, &line, area.width);
        y_offset += 1;
        rendered_body_lines += 1;
        index += 1;
    }

    // If there are more lines than fit, show a truncation notice.
    let total_content_lines = diff_lines.len();
    let shown = (max_lines.saturating_sub(1)).min(total_content_lines);
    if shown < total_content_lines && y_offset < area.height {
        let remaining = total_content_lines - shown;
        let notice = format!("  ... {} more lines ...", remaining);
        let notice_line = Line::from(Span::styled(notice, theme.dim));
        buf.set_line(area.x, area.y + y_offset, &notice_line, area.width);
    }
}

#[cfg(test)]
fn build_diff_line(
    line_no_spans: Vec<Span<'static>>,
    prefix: &str,
    style: ratatui::style::Style,
    content_spans: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = vec![
        line_no_spans[0].clone(),
        Span::raw(" "),
        line_no_spans[1].clone(),
        Span::raw(" "),
        Span::styled(prefix.to_string(), style),
        Span::styled(" ".to_string(), style),
    ];
    spans.extend(content_spans);
    Line::from(spans)
}

#[cfg(test)]
fn truncate_spans_by_width(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut width = 0usize;
    let mut out = Vec::new();
    for span in spans {
        let mut text = String::new();
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > max_width {
                break;
            }
            text.push(ch);
            width += ch_width;
        }
        if !text.is_empty() {
            out.push(Span::styled(text, span.style));
        }
        if width >= max_width {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Word-level diff
// ---------------------------------------------------------------------------

/// A single word-diff segment within a changed line.
#[derive(Debug, Clone)]
pub struct WordDiffSegment {
    pub text: String,
    pub kind: ChangeTag,
}

/// Compute a word-level diff between two text strings.
///
/// Splits on whitespace boundaries and compares tokens using `similar::TextDiff`
/// at the word granularity, producing a sequence of segments tagged as equal,
/// inserted, or deleted.
pub fn word_diff(old: &str, new: &str) -> Vec<WordDiffSegment> {
    if old == new {
        return vec![WordDiffSegment {
            text: old.to_string(),
            kind: ChangeTag::Equal,
        }];
    }

    let diff = TextDiff::from_words(old, new);
    let mut segments = Vec::new();

    for change in diff.iter_all_changes() {
        segments.push(WordDiffSegment {
            text: change.value().to_string(),
            kind: change.tag(),
        });
    }

    segments
}

/// Render word-level diff segments as styled spans.
///
/// Equal text uses the base style. Added words use diff_add, removed words
/// use diff_remove. This provides a richer inline view of what changed
/// within a line.
pub fn render_word_diff_spans(
    segments: &[WordDiffSegment],
    base_style: ratatui::style::Style,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(segments.len());
    for seg in segments {
        let style = match seg.kind {
            ChangeTag::Equal => base_style,
            ChangeTag::Insert => theme.diff_add,
            ChangeTag::Delete => theme.diff_remove,
        };
        spans.push(Span::styled(seg.text.clone(), style));
    }
    spans
}

/// Render a word-diff line with inline highlights for added/removed words.
///
/// This is the high-level entry point for word-level diff rendering.
/// Given the old and new line content, it produces styled spans where
/// individual words within the line are colored according to their change type.
pub fn render_word_diff_line(
    old: &str,
    new: &str,
    is_addition: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let segments = word_diff(old, new);
    if is_addition {
        let visible_segments = segments
            .into_iter()
            .filter(|segment| segment.kind != ChangeTag::Delete)
            .collect::<Vec<_>>();
        render_word_diff_spans(&visible_segments, theme.diff_add, theme)
    } else {
        let visible_segments = segments
            .into_iter()
            .filter(|segment| segment.kind != ChangeTag::Insert)
            .collect::<Vec<_>>();
        render_word_diff_spans(&visible_segments, theme.diff_remove, theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_to_plain(spans: Vec<Span<'static>>) -> String {
        spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
    }

    #[test]
    fn word_diff_line_keeps_removed_and_added_sides_separate() {
        let theme = Theme::default();
        let removed = spans_to_plain(render_word_diff_line(
            "let stale = true;",
            "let stale = false;",
            false,
            &theme,
        ));
        let added = spans_to_plain(render_word_diff_line(
            "let stale = true;",
            "let stale = false;",
            true,
            &theme,
        ));

        assert!(removed.contains("true"));
        assert!(!removed.contains("false"));
        assert!(added.contains("false"));
        assert!(!added.contains("true"));
    }

    #[test]
    fn structured_hunks_for_path_parses_stored_hunks() {
        let mut data = DiffData::empty();
        data.hunks.insert(
            "src/main.rs".to_string(),
            vec![
                "@@ -1,1 +1,2 @@ fn main".to_string(),
                " fn main() {".to_string(),
                "+    println!(\"hi\");".to_string(),
                " }".to_string(),
            ],
        );

        let hunks = data.structured_hunks_for_path("src/main.rs");

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_lines, 2);
    }
}
