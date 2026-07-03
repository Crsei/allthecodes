use std::cell::RefCell;
use std::num::NonZeroUsize;

use lru::LruCache;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use super::syntax_highlight::highlight_code_block;
use super::theme::Theme;

// ---------------------------------------------------------------------------
// LRU cache (thread-local, 256 entries)
// ---------------------------------------------------------------------------

thread_local! {
    static MD_CACHE: RefCell<LruCache<u64, Vec<Line<'static>>>> =
        RefCell::new(LruCache::new(NonZeroUsize::new(256).unwrap_or(NonZeroUsize::MIN)));
}

fn cache_key(text: &str, theme: &Theme) -> u64 {
    let mut key = String::with_capacity(text.len() + 512);
    key.push_str(text);
    key.push('\0');
    key.push_str(&theme_fingerprint(theme));
    allthecodes_utils::hash::hash_content(key.as_bytes())
}

fn theme_fingerprint(theme: &Theme) -> String {
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        theme.assistant_name,
        theme.user_name,
        theme.system_name,
        theme.tool_name,
        theme.tool_result,
        theme.error,
        theme.warning,
        theme.info,
        theme.prompt,
        theme.border,
        theme.code,
        theme.code_bg,
        theme.thinking,
        theme.dim,
        theme.heading,
        theme.bold,
        theme.italic,
        theme.link,
        theme.syntax_keyword,
        theme.syntax_string,
        theme.syntax_comment,
        theme.syntax_type,
        theme.syntax_function,
        theme.syntax_number,
        theme.syntax_operator,
        theme.syntax_builtin,
        theme.syntax_punctuation,
        theme.diff_add,
        theme.diff_remove,
        theme.diff_context,
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a markdown string into a vector of styled ratatui [`Line`]s.
///
/// Results are LRU-cached so repeated calls with the same content skip
/// re-parsing (e.g. when scrolling back through history).
pub fn markdown_to_lines(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let key = cache_key(text, theme);
    let cached = MD_CACHE.with(|c| c.borrow_mut().get(&key).cloned());
    if let Some(lines) = cached {
        return lines;
    }
    let lines = markdown_to_lines_inner(text, theme);
    MD_CACHE.with(|c| c.borrow_mut().put(key, lines.clone()));
    lines
}

// ---------------------------------------------------------------------------
// Table rendering state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TableBuffer {
    alignments: Vec<Alignment>,
    head: Option<TableRow>,
    body: Vec<TableRow>,
}

type TableCell = Vec<Span<'static>>;
type TableRow = Vec<TableCell>;

// ---------------------------------------------------------------------------
// Inner implementation
// ---------------------------------------------------------------------------

/// Supported elements:
/// - Headings (rendered bold + underlined)
/// - Bold / strong emphasis
/// - Italic / emphasis
/// - Inline code (rendered with `theme.code` style)
/// - Fenced / indented code blocks (each line rendered with `theme.code`)
/// - Unordered lists (prefixed with "  - ")
/// - Ordered lists (prefixed with "  N. ")
/// - Links (rendered underlined)
/// - Paragraphs (separated by blank lines)
/// - Tables (column-aligned grid with borders)
fn markdown_to_lines_inner(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(text, opts);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();

    // Style stack: the most recent style is applied to incoming text events.
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_buf = String::new();
    let mut list_stack: Vec<ListKind> = Vec::new();

    // Table rendering state
    let mut in_table = false;
    let mut table = TableBuffer::default();
    let mut current_row_cells: TableRow = Vec::new();
    let mut current_cell_spans: TableCell = Vec::new();
    let mut in_table_head = false;

    for event in parser {
        match event {
            // ── Block-level starts ──────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                flush_line(&mut current_spans, &mut lines);
                let heading_style = match level {
                    pulldown_cmark::HeadingLevel::H1 => theme
                        .heading
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    pulldown_cmark::HeadingLevel::H2 => theme.heading.add_modifier(Modifier::BOLD),
                    _ => theme.bold,
                };
                style_stack.push(heading_style);
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_line(&mut current_spans, &mut lines);
                style_stack.pop();
                lines.push(Line::from(""));
            }

            Event::Start(Tag::Paragraph) => {
                flush_line(&mut current_spans, &mut lines);
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_table {
                    flush_line(&mut current_spans, &mut lines);
                    lines.push(Line::from(""));
                }
            }

            // ── Code blocks ─────────────────────────────────────────
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_line(&mut current_spans, &mut lines);
                in_code_block = true;
                code_block_lang = extract_code_block_lang(&kind);
                code_block_buf.clear();
                style_stack.push(theme.code);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                style_stack.pop();

                // Apply syntax highlighting to the buffered code.
                if code_block_buf.is_empty() {
                    flush_line(&mut current_spans, &mut lines);
                } else {
                    let highlighted =
                        highlight_code_block(&code_block_buf, &code_block_lang, theme);
                    // Split highlighted spans into lines
                    let mut line_spans: Vec<Span<'static>> = Vec::new();
                    for span in highlighted {
                        if span.content.as_ref() == "\n" {
                            if line_spans.is_empty() {
                                lines.push(Line::default());
                            } else {
                                lines.push(Line::from(std::mem::take(&mut line_spans)));
                            }
                        } else {
                            line_spans.push(Span::styled(span.content.to_string(), span.style));
                        }
                    }
                    if !line_spans.is_empty() {
                        lines.push(Line::from(line_spans));
                    }
                    current_spans.clear();
                }
                code_block_buf.clear();
                code_block_lang.clear();
                lines.push(Line::from(""));
            }

            // ── Inline styles ───────────────────────────────────────
            Event::Start(Tag::Strong) => {
                style_stack.push(theme.bold);
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(theme.italic);
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }

            // ── Lists ───────────────────────────────────────────────
            Event::Start(Tag::List(first_number)) => {
                flush_line(&mut current_spans, &mut lines);
                let kind = match first_number {
                    Some(start) => ListKind::Ordered(start as usize),
                    None => ListKind::Unordered,
                };
                list_stack.push(kind);
            }
            Event::End(TagEnd::List(_)) => {
                flush_line(&mut current_spans, &mut lines);
                list_stack.pop();
                if list_stack.is_empty() {
                    lines.push(Line::from(""));
                }
            }
            Event::Start(Tag::Item) => {
                flush_line(&mut current_spans, &mut lines);
                let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                match list_stack.last_mut() {
                    Some(ListKind::Unordered) => {
                        current_spans.push(Span::raw(format!("{}- ", indent)));
                    }
                    Some(ListKind::Ordered(n)) => {
                        current_spans.push(Span::raw(format!("{}{}. ", indent, n)));
                        *n += 1;
                    }
                    None => {}
                }
            }
            Event::End(TagEnd::Item) => {
                flush_line(&mut current_spans, &mut lines);
            }

            // ── Links ───────────────────────────────────────────────
            Event::Start(Tag::Link { .. }) => {
                style_stack.push(theme.link);
            }
            Event::End(TagEnd::Link) => {
                style_stack.pop();
            }

            // ── Block quote ─────────────────────────────────────────
            Event::Start(Tag::BlockQuote(_)) => {
                flush_line(&mut current_spans, &mut lines);
                current_spans.push(Span::styled("  | ", theme.dim));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush_line(&mut current_spans, &mut lines);
            }

            // ── Inline code ─────────────────────────────────────────
            Event::Code(code) => {
                let span = Span::styled(format!("`{}`", code), theme.code);
                if in_table {
                    current_cell_spans.push(span);
                } else {
                    current_spans.push(span);
                }
            }

            // ── Text ────────────────────────────────────────────────
            Event::Text(text) => {
                let style = current_style(&style_stack);
                if in_code_block {
                    code_block_buf.push_str(&text);
                } else if in_table {
                    current_cell_spans.push(Span::styled(text.to_string(), style));
                } else {
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }

            // ── Soft / Hard break ───────────────────────────────────
            Event::SoftBreak => {
                if in_table {
                    current_cell_spans.push(Span::raw(" "));
                } else {
                    current_spans.push(Span::raw(" "));
                }
            }
            Event::HardBreak => {
                if in_table {
                    current_cell_spans.push(Span::raw(" "));
                } else {
                    flush_line(&mut current_spans, &mut lines);
                }
            }

            // ── Horizontal rule ─────────────────────────────────────
            Event::Rule => {
                flush_line(&mut current_spans, &mut lines);
                lines.push(Line::from(Span::styled(
                    "────────────────────────────────".to_string(),
                    theme.dim,
                )));
                lines.push(Line::from(""));
            }

            // ── Tables ──────────────────────────────────────────────
            Event::Start(Tag::Table(alignments)) => {
                flush_line(&mut current_spans, &mut lines);
                in_table = true;
                table = TableBuffer {
                    alignments: alignments.clone(),
                    head: None,
                    body: Vec::new(),
                };
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                in_table_head = false;
                render_table(&table, theme, &mut lines);
                table = TableBuffer::default();
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
                lines.push(Line::from(""));
            }
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
            }
            Event::End(TagEnd::TableHead) => {
                flush_cell_spans(&mut current_cell_spans, &mut current_row_cells);
                if !current_row_cells.is_empty() {
                    table.head = Some(std::mem::take(&mut current_row_cells));
                }
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
                in_table_head = false;
            }
            Event::Start(Tag::TableRow) => {
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
            }
            Event::End(TagEnd::TableRow) => {
                flush_cell_spans(&mut current_cell_spans, &mut current_row_cells);
                if !current_row_cells.is_empty() {
                    if in_table_head {
                        table.head = Some(std::mem::take(&mut current_row_cells));
                    } else {
                        table.body.push(std::mem::take(&mut current_row_cells));
                    }
                }
                current_row_cells = Vec::new();
                current_cell_spans = Vec::new();
            }
            Event::Start(Tag::TableCell) => {
                current_cell_spans = Vec::new();
            }
            Event::End(TagEnd::TableCell) => {
                current_row_cells.push(std::mem::take(&mut current_cell_spans));
                current_cell_spans = Vec::new();
            }

            _ => {}
        }
    }

    // Flush any remaining spans outside table context.
    if !in_table {
        flush_line(&mut current_spans, &mut lines);
    }

    // Remove trailing blank lines.
    while lines
        .last()
        .is_some_and(|l| l.spans.is_empty() || line_is_empty(l))
    {
        lines.pop();
    }

    lines
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Render a complete Markdown table as styled ratatui lines.
///
/// ┌───────┬───────┐
/// │ Head1 │ Head2 │
/// ├───────┼───────┤
/// │ Cell1 │ Cell2 │
/// └───────┴───────┘
fn render_table(table: &TableBuffer, theme: &Theme, lines: &mut Vec<Line<'static>>) {
    let ncols = compute_column_count(table);
    if ncols == 0 {
        return;
    }

    // Build rows: head + body
    let mut rows: Vec<TableRow> = Vec::new();
    if let Some(head) = &table.head {
        rows.push(pad_row(head, ncols));
    }
    for row in &table.body {
        rows.push(pad_row(row, ncols));
    }

    // Compute column widths
    let col_widths = compute_column_widths(&rows, ncols);

    // Render header separator
    render_table_separator(&col_widths, theme, lines, true);

    // Render body rows
    let header_len = usize::from(table.head.is_some());
    for (idx, row) in rows.iter().enumerate() {
        render_table_row(row, &col_widths, &table.alignments, theme, lines);
        if header_len > 0 && idx + 1 == header_len {
            // After header: render header/body separator
            render_table_separator(&col_widths, theme, lines, false);
        }
    }

    // Render bottom separator
    render_table_bottom(&col_widths, theme, lines);
}

fn compute_column_count(table: &TableBuffer) -> usize {
    let head_cols = table.head.as_ref().map(|r| r.len()).unwrap_or(0);
    let body_cols = table.body.first().map(|r| r.len()).unwrap_or(0);
    head_cols.max(body_cols)
}

fn pad_row(row: &[TableCell], ncols: usize) -> TableRow {
    let mut r = row.to_vec();
    while r.len() < ncols {
        r.push(vec![Span::raw("")]);
    }
    r
}

fn compute_column_widths(rows: &[TableRow], ncols: usize) -> Vec<usize> {
    let mut widths = vec![0usize; ncols];
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            let cell_width: usize = cell
                .iter()
                .map(|span| {
                    span.content
                        .chars()
                        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                        .sum::<usize>()
                })
                .sum();
            widths[col] = widths[col].max(cell_width);
        }
    }
    // Minimum width: at least 1 character per column
    for w in &mut widths {
        *w = (*w).max(1);
    }
    widths
}

fn render_table_separator(
    col_widths: &[usize],
    theme: &Theme,
    lines: &mut Vec<Line<'static>>,
    is_top: bool,
) {
    let left = if is_top { "┌" } else { "├" };
    const RIGHT: &str = "┤";
    const MID: &str = "┬";
    const SEP_MID: &str = "┼";
    let mid_sep = if is_top { MID } else { SEP_MID };
    let right = if is_top { "┐" } else { RIGHT };

    let mut spans = Vec::with_capacity(col_widths.len() * 2 + 1);
    spans.push(Span::styled(left.to_string(), theme.border));
    for (i, w) in col_widths.iter().enumerate() {
        let line = "─".repeat(*w + 2); // +2 for padding
        spans.push(Span::styled(line, theme.border));
        if i < col_widths.len() - 1 {
            spans.push(Span::styled(mid_sep.to_string(), theme.border));
        }
    }
    spans.push(Span::styled(right.to_string(), theme.border));
    lines.push(Line::from(spans));
}

fn render_table_bottom(col_widths: &[usize], theme: &Theme, lines: &mut Vec<Line<'static>>) {
    let mut spans = Vec::with_capacity(col_widths.len() * 2 + 1);
    spans.push(Span::styled("└".to_string(), theme.border));
    for (i, w) in col_widths.iter().enumerate() {
        let line = "─".repeat(*w + 2);
        spans.push(Span::styled(line, theme.border));
        if i < col_widths.len() - 1 {
            spans.push(Span::styled("┴".to_string(), theme.border));
        }
    }
    spans.push(Span::styled("┘".to_string(), theme.border));
    lines.push(Line::from(spans));
}

fn render_table_row(
    row: &[TableCell],
    col_widths: &[usize],
    alignments: &[Alignment],
    theme: &Theme,
    lines: &mut Vec<Line<'static>>,
) {
    let mut spans = Vec::with_capacity(col_widths.len() * 2 + 1);
    spans.push(Span::styled("│".to_string(), theme.border));

    for (col, w) in col_widths.iter().enumerate() {
        let content = row.get(col).cloned().unwrap_or_default();
        let content_width: usize = content
            .iter()
            .flat_map(|s| s.content.chars())
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();

        let padding = w.saturating_sub(content_width);
        let align = alignments.get(col).copied().unwrap_or(Alignment::None);

        let (left_pad, right_pad) = match align {
            Alignment::Left | Alignment::None => (1, padding + 1),
            Alignment::Center => {
                let left = padding / 2;
                (left + 1, padding - left + 1)
            }
            Alignment::Right => (padding + 1, 1),
        };

        spans.push(Span::raw(" ".repeat(left_pad)));
        spans.extend(content);
        spans.push(Span::raw(" ".repeat(right_pad)));
        spans.push(Span::styled("│".to_string(), theme.border));
    }

    lines.push(Line::from(spans));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

fn flush_line(current_spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !current_spans.is_empty() {
        let spans = std::mem::take(current_spans);
        lines.push(Line::from(spans));
    }
}

fn flush_cell_spans(current_cell_spans: &mut TableCell, current_row_cells: &mut TableRow) {
    if !current_cell_spans.is_empty() {
        current_row_cells.push(std::mem::take(current_cell_spans));
    }
}

fn line_is_empty(line: &Line) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn extract_code_block_lang(kind: &CodeBlockKind<'_>) -> String {
    match kind {
        CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
        CodeBlockKind::Indented => String::new(),
    }
}

enum ListKind {
    Unordered,
    Ordered(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_lines(lines: Vec<Line<'static>>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn renders_table_header_and_body_cells() {
        let lines = plain_lines(markdown_to_lines(
            "| Name | Lang |\n| --- | --- |\n| Ada | Rust |\n| Grace | TS |",
            &Theme::default(),
        ));
        let rendered = lines.join("\n");

        assert!(rendered.contains("Name"));
        assert!(rendered.contains("Lang"));
        assert!(rendered.contains("Ada"));
        assert!(rendered.contains("Rust"));
        assert!(rendered.contains("Grace"));
        assert!(rendered.contains("TS"));
    }

    #[test]
    fn table_body_rows_are_not_overwritten_as_header() {
        let lines = plain_lines(markdown_to_lines(
            "| H |\n| - |\n| one |\n| two |",
            &Theme::default(),
        ));
        let rendered = lines.join("\n");

        assert!(rendered.contains("H"));
        assert!(rendered.contains("one"));
        assert!(rendered.contains("two"));
    }

    #[test]
    fn extracts_fenced_code_block_language() {
        use pulldown_cmark::CowStr;

        assert_eq!(
            extract_code_block_lang(&CodeBlockKind::Fenced(CowStr::Borrowed("rust ignore"))),
            "rust"
        );
        assert_eq!(
            extract_code_block_lang(&CodeBlockKind::Fenced(CowStr::Borrowed(""))),
            ""
        );
        assert_eq!(extract_code_block_lang(&CodeBlockKind::Indented), "");
    }

    #[test]
    fn preserves_blank_lines_inside_code_block() {
        let lines = plain_lines(markdown_to_lines(
            "```rust\nfn main() {\n\n    println!(\"hi\");\n}\n```",
            &Theme::default(),
        ));

        assert!(lines.windows(4).any(|window| {
            window[0].contains("fn main()")
                && window[1].is_empty()
                && window[2].contains("println!")
                && window[3].contains("}")
        }));
    }

    #[test]
    fn cache_key_includes_theme_styles() {
        let red_theme = Theme {
            code: ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(255, 0, 0)),
            ..Default::default()
        };
        let blue_theme = Theme {
            code: ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(0, 0, 255)),
            ..Default::default()
        };

        let red_lines = markdown_to_lines("`x`", &red_theme);
        let blue_lines = markdown_to_lines("`x`", &blue_theme);

        let red_style = red_lines[0].spans[0].style;
        let blue_style = blue_lines[0].spans[0].style;
        assert_eq!(red_style, red_theme.code);
        assert_eq!(blue_style, blue_theme.code);
        assert_ne!(red_style, blue_style);
    }
}
