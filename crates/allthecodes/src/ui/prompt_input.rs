use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

const USER_INPUT_BACKGROUND: Color = Color::Rgb(31, 35, 42);
const PROMPT_PREFIX: &str = "> ";
const PROMPT_PREFIX_WIDTH: usize = 2;
const INPUT_VERTICAL_PADDING: u16 = 2;
pub const MAX_VISIBLE_INPUT_LINES: usize = 8;

/// One display row of the prompt text. `start..end` is always a UTF-8
/// character-boundary range in the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptInputVisualLine {
    pub start: usize,
    pub end: usize,
    pub display_width: usize,
}

/// The single layout result consumed by both prompt rendering and the real
/// terminal cursor. Keeping the caret and viewport in this value prevents the
/// IME anchor from drifting away from the cell containing the rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptInputLayout {
    pub visual_lines: Vec<PromptInputVisualLine>,
    pub viewport_start: usize,
    pub caret_visual_line: usize,
    pub caret_display_column: usize,
    pub text_width: usize,
    pub cursor_position: Option<Position>,
}

impl PromptInputLayout {
    pub fn visual_height(&self) -> usize {
        self.visual_lines.len()
    }

    pub fn cursor_position(&self) -> Option<Position> {
        self.cursor_position
    }
}

/// A multiline text input widget with UTF-8-safe editing and display-column
/// layout.
///
/// The input keeps a byte cursor because the rest of the completion and vim
/// integrations use byte ranges. Every editing operation preserves the
/// invariant that the cursor is on a character boundary. Rendering uses the
/// same visual-line layout that produces the physical terminal cursor, so
/// CJK, combining marks, wrapping, and vertical scrolling share one source of
/// truth.
pub struct PromptInput {
    /// Current input text.
    pub input: String,
    /// Byte-offset cursor position within `input`.
    pub cursor_position: usize,
    /// Whether this widget is focused / accepting input.
    pub is_active: bool,
    /// Summary of the most recent large paste, shown by the app chrome only.
    large_paste_notice: Option<String>,
    /// Optional ghost suffix text shown dimmed after the cursor.
    ghost_suffix: Option<String>,
    /// Whether to show the ghost suffix.
    show_ghost: bool,
    /// Display column retained while moving vertically through visual rows.
    desired_vertical_column: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptInputRenderContext<'a> {
    pub hint: Option<&'a str>,
    pub placeholder: Option<&'a str>,
    pub mode_indicator: Option<&'a str>,
}

impl PromptInput {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            is_active: true,
            large_paste_notice: None,
            ghost_suffix: None,
            show_ghost: false,
            desired_vertical_column: None,
        }
    }

    pub fn set_ghost_suffix(&mut self, text: Option<String>) {
        self.ghost_suffix = text;
    }

    pub fn set_show_ghost(&mut self, show: bool) {
        self.show_ghost = show;
    }

    #[cfg(test)]
    pub fn ghost_suffix(&self) -> Option<&str> {
        self.ghost_suffix.as_deref()
    }

    #[cfg(test)]
    pub fn show_ghost(&self) -> bool {
        self.show_ghost
    }

    /// Return the number of rows needed by the prompt, including its two-row
    /// top/bottom breathing room. The cap prevents a paste from consuming the
    /// entire conversation pane.
    pub fn preferred_height(&self, width: u16, _context: PromptInputRenderContext<'_>) -> u16 {
        if width < 4 {
            return 0;
        }
        let layout = self.layout(Rect::new(0, 0, width, u16::MAX));
        layout
            .visual_height()
            .min(MAX_VISIBLE_INPUT_LINES)
            .saturating_add(usize::from(INPUT_VERTICAL_PADDING))
            .max(3) as u16
    }

    /// Handle a key event. Returns `Some(submitted_text)` when the user
    /// presses Enter with a non-empty input, clearing the internal buffer.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        if !self.is_active {
            return None;
        }

        match (key.modifiers, key.code) {
            // Enter submits; the terminal's IME consumes composition Enter
            // before it reaches this handler, while Shift+Enter is the
            // explicit multiline insertion binding.
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if self.input.trim().is_empty() {
                    return None;
                }
                let text = self.input.clone();
                self.input.clear();
                self.cursor_position = 0;
                self.large_paste_notice = None;
                self.desired_vertical_column = None;
                return Some(text);
            }
            (KeyModifiers::SHIFT, KeyCode::Enter) => {
                self.insert_text_internal("\n");
            }

            // Ctrl shortcuts
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.input.clear();
                self.cursor_position = 0;
                self.reset_vertical_column();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.cursor_position = 0;
                self.reset_vertical_column();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.cursor_position = self.input.len();
                self.reset_vertical_column();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.delete_word_backwards();
                self.reset_vertical_column();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                let end = self.current_visual_line_end();
                self.input.drain(self.cursor_position..end);
                self.reset_vertical_column();
            }

            // Horizontal navigation. Home/End are line-local in multiline
            // input; Ctrl+A/Ctrl+E retain whole-buffer semantics above.
            (_, KeyCode::Left) => {
                self.move_cursor_left();
                self.reset_vertical_column();
            }
            (_, KeyCode::Right) => {
                self.move_cursor_right();
                self.reset_vertical_column();
            }
            (_, KeyCode::Home) => {
                self.cursor_position = self.current_visual_line_start();
                self.reset_vertical_column();
            }
            (_, KeyCode::End) => {
                self.cursor_position = self.current_visual_line_end();
                self.reset_vertical_column();
            }

            // Deletion
            (_, KeyCode::Backspace) => {
                if self.cursor_position > 0 {
                    let prev = self.prev_grapheme_boundary();
                    self.input.drain(prev..self.cursor_position);
                    self.cursor_position = prev;
                }
                self.reset_vertical_column();
            }
            (_, KeyCode::Delete) => {
                if self.cursor_position < self.input.len() {
                    let next = self.next_grapheme_boundary();
                    self.input.drain(self.cursor_position..next);
                }
                self.reset_vertical_column();
            }

            // Character input, including committed Unicode text from an IME.
            (_, KeyCode::Char(c)) => {
                let mut encoded = [0_u8; 4];
                self.insert_text_internal(c.encode_utf8(&mut encoded));
            }

            _ => {}
        }

        None
    }

    /// Move through the visual rows produced for `width`. Returns false at a
    /// vertical edge so App can fall back to prompt history there.
    pub fn move_cursor_vertical(&mut self, width: u16, direction: i8) -> bool {
        let lines = visual_lines_for_width(&self.input, text_width_for_area(width));
        if lines.is_empty() {
            return false;
        }
        let current = visual_line_index_for_cursor(&self.input, &lines, self.cursor_position);
        let target = if direction < 0 {
            current.checked_sub(1)
        } else if direction > 0 {
            (current + 1 < lines.len()).then_some(current + 1)
        } else {
            Some(current)
        };
        let Some(target) = target else {
            return false;
        };

        let current_column = display_width_between(
            &self.input,
            lines[current].start,
            self.cursor_position.min(lines[current].end),
        );
        let desired = self.desired_vertical_column.unwrap_or(current_column);
        self.cursor_position = byte_offset_at_display_column(&self.input, &lines[target], desired);
        self.desired_vertical_column = Some(desired);
        true
    }

    pub fn reset_vertical_navigation(&mut self) {
        self.reset_vertical_column();
    }

    /// Insert text at the current cursor position. Used by voice dictation
    /// and other non-keyboard input paths.
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.insert_text_internal(text);
    }

    /// Insert pasted text, normalizing all common terminal newline forms.
    pub fn paste_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text_internal(&normalized);
        self.large_paste_notice = large_paste_notice(&normalized);
    }

    #[cfg(test)]
    pub fn take_large_paste_notice(&mut self) -> Option<String> {
        self.large_paste_notice.take()
    }

    pub fn large_paste_notice(&self) -> Option<&str> {
        self.large_paste_notice.as_deref()
    }

    #[cfg(test)]
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) -> PromptInputLayout {
        self.render_with_context(area, buf, theme, PromptInputRenderContext::default())
    }

    #[cfg(test)]
    pub fn render_with_hint(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        hint: Option<&str>,
    ) -> PromptInputLayout {
        self.render_with_context(
            area,
            buf,
            theme,
            PromptInputRenderContext {
                hint,
                placeholder: None,
                mode_indicator: None,
            },
        )
    }

    /// Render the prompt and return the exact layout used for the physical
    /// cursor. The application calls `Frame::set_cursor_position` with the
    /// returned position only when this surface owns the terminal cursor.
    pub fn render_with_context(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        context: PromptInputRenderContext<'_>,
    ) -> PromptInputLayout {
        let layout = self.layout(area);
        if area.height == 0 || area.width < 4 {
            return layout;
        }

        fill_input_background(area, buf);
        let text_y = area.y.saturating_add(1);
        let visible_end = (layout.viewport_start + visible_line_count(&layout, area))
            .min(layout.visual_lines.len());

        for visual_index in layout.viewport_start..visible_end {
            let row = visual_index.saturating_sub(layout.viewport_start) as u16;
            let y = text_y.saturating_add(row);
            if y >= area.y.saturating_add(area.height).saturating_sub(1) {
                break;
            }
            let visual = &layout.visual_lines[visual_index];
            let text = &self.input[visual.start..visual.end];
            let mut spans = Vec::new();

            // The prefix is a separate cell region. Every wrapped and hard
            // continuation starts at the same text origin (`x + 2`).
            let prefix = if visual_index == 0 {
                PROMPT_PREFIX
            } else {
                "  "
            };
            spans.push(Span::styled(prefix, with_input_background(theme.prompt)));

            if self.input.is_empty() && visual_index == 0 {
                if let Some(placeholder) = context.placeholder {
                    spans.push(Span::styled(
                        placeholder.to_string(),
                        with_input_background(theme.dim),
                    ));
                }
            } else if self.is_active {
                spans.push(Span::styled(
                    text.to_string(),
                    with_input_background(Style::default()),
                ));
                if self.show_ghost
                    && visual_index == layout.caret_visual_line
                    && self.cursor_position == self.input.len()
                {
                    if let Some(suffix) = self.ghost_suffix.as_deref().filter(|s| !s.is_empty()) {
                        spans.push(Span::styled(
                            suffix.to_string(),
                            with_input_background(theme.dim),
                        ));
                    }
                    if let Some(hint) = context.hint.filter(|hint| !hint.is_empty()) {
                        spans.push(Span::styled(
                            format!(" {hint}"),
                            with_input_background(theme.dim),
                        ));
                    }
                } else if visual_index == layout.caret_visual_line
                    && self.cursor_position == self.input.len()
                {
                    if let Some(hint) = context.hint.filter(|hint| !hint.is_empty()) {
                        spans.push(Span::styled(
                            format!(" {hint}"),
                            with_input_background(theme.dim),
                        ));
                    }
                }
            } else {
                spans.push(Span::styled(
                    text.to_string(),
                    with_input_background(theme.dim),
                ));
            }

            buf.set_line(area.x, y, &Line::from(spans), area.width);
        }

        if let Some(label) = context.mode_indicator.filter(|label| !label.is_empty()) {
            let label = format!("[{label}]");
            let label_width = UnicodeWidthStr::width(label.as_str()) as u16;
            let x = area
                .x
                .saturating_add(area.width.saturating_sub(label_width));
            buf.set_line(
                x,
                area.y,
                &Line::from(Span::styled(label, with_input_background(theme.dim))),
                label_width.min(area.width),
            );
        }

        layout
    }

    /// Compute the layout without mutating the input. `render_with_context`
    /// and the vertical editor use the same `visual_lines_for_width` helper.
    pub fn layout(&self, area: Rect) -> PromptInputLayout {
        let text_width = text_width_for_area(area.width);
        let visual_lines = visual_lines_for_width(&self.input, text_width);
        let caret_visual_line = visual_line_index_for_cursor(
            &self.input,
            &visual_lines,
            self.cursor_position.min(self.input.len()),
        );
        let caret_line =
            visual_lines
                .get(caret_visual_line)
                .cloned()
                .unwrap_or(PromptInputVisualLine {
                    start: 0,
                    end: 0,
                    display_width: 0,
                });
        let caret_display_column = display_width_between(
            &self.input,
            caret_line.start,
            self.cursor_position
                .min(caret_line.end)
                .max(caret_line.start),
        );
        let max_rows = visible_line_count_for_area(area);
        let viewport_start =
            viewport_start_for_caret(caret_visual_line, visual_lines.len(), max_rows);
        let cursor_position = if area.height == 0 || area.width < 4 {
            None
        } else {
            let row = caret_visual_line.saturating_sub(viewport_start);
            let x = area
                .x
                .saturating_add(PROMPT_PREFIX_WIDTH as u16)
                .saturating_add(caret_display_column.min(u16::MAX as usize) as u16);
            let y = area.y.saturating_add(1).saturating_add(row as u16);
            let right = area.x.saturating_add(area.width);
            let bottom = area.y.saturating_add(area.height.saturating_sub(1));
            (x < right && y < bottom).then_some(Position { x, y })
        };

        PromptInputLayout {
            visual_lines,
            viewport_start,
            caret_visual_line,
            caret_display_column,
            text_width,
            cursor_position,
        }
    }

    fn insert_text_internal(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor_position, text);
        self.cursor_position += text.len();
        self.reset_vertical_column();
    }

    fn reset_vertical_column(&mut self) {
        self.desired_vertical_column = None;
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position = self.prev_grapheme_boundary();
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position = self.next_grapheme_boundary();
        }
    }

    fn prev_grapheme_boundary(&self) -> usize {
        let pos = self.cursor_position.min(self.input.len());
        if pos == 0 {
            return 0;
        }
        self.input[..pos]
            .grapheme_indices(true)
            .next_back()
            .map(|(start, _)| start)
            .unwrap_or(0)
    }

    fn next_grapheme_boundary(&self) -> usize {
        let pos = self.cursor_position.min(self.input.len());
        if pos >= self.input.len() {
            return self.input.len();
        }
        self.input[pos..]
            .grapheme_indices(true)
            .nth(1)
            .map(|(offset, _)| pos + offset)
            .unwrap_or(self.input.len())
    }

    fn current_visual_line_start(&self) -> usize {
        let lines = visual_lines_for_width(&self.input, usize::MAX / 4);
        lines
            .iter()
            .find(|line| self.cursor_position <= line.end)
            .map(|line| line.start)
            .unwrap_or(0)
    }

    fn current_visual_line_end(&self) -> usize {
        let lines = visual_lines_for_width(&self.input, usize::MAX / 4);
        lines
            .iter()
            .find(|line| self.cursor_position <= line.end)
            .map(|line| line.end)
            .unwrap_or(self.input.len())
    }

    fn delete_word_backwards(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let mut end = self.cursor_position;
        while end > 0 {
            let Some((start, ch)) = self.input[..end].char_indices().next_back() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            end = start;
        }
        while end > 0 {
            let Some((start, ch)) = self.input[..end].char_indices().next_back() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            end = start;
        }
        self.input.drain(end..self.cursor_position);
        self.cursor_position = end;
    }
}

fn text_width_for_area(width: u16) -> usize {
    // The text owns every cell after the `> ` prefix. If the caret lands
    // immediately after a row that exactly fills this width, the visual-line
    // builder adds an empty sentinel row; reserving a column here would make
    // ordinary input wrap one cell too early and would make the rendered text
    // disagree with the terminal's cell coordinates.
    usize::from(width.saturating_sub(PROMPT_PREFIX_WIDTH as u16)).max(1)
}

fn visible_line_count_for_area(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(INPUT_VERTICAL_PADDING)).max(1)
}

fn visible_line_count(layout: &PromptInputLayout, area: Rect) -> usize {
    visible_line_count_for_area(area).min(layout.visual_lines.len().max(1))
}

fn viewport_start_for_caret(caret: usize, total: usize, max_rows: usize) -> usize {
    if total <= max_rows.max(1) {
        return 0;
    }
    caret
        .saturating_add(1)
        .saturating_sub(max_rows.max(1))
        .min(total - 1)
}

fn visual_lines_for_width(input: &str, width: usize) -> Vec<PromptInputVisualLine> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut segment_start = 0;

    loop {
        let segment_end = input[segment_start..]
            .find('\n')
            .map(|offset| segment_start + offset)
            .unwrap_or(input.len());
        append_wrapped_segment(input, segment_start, segment_end, width, &mut lines);

        if segment_end == input.len() {
            // A caret after a line that exactly fills the viewport belongs to
            // the next empty visual row. Without this sentinel row the
            // cursor would be placed at the frame's right edge and Ratatui
            // would hide it as out of bounds.
            if lines
                .last()
                .is_some_and(|line| line.end == input.len() && line.display_width >= width)
            {
                lines.push(PromptInputVisualLine {
                    start: input.len(),
                    end: input.len(),
                    display_width: 0,
                });
            }
            break;
        }
        segment_start = segment_end + 1;
        if segment_start == input.len() {
            lines.push(PromptInputVisualLine {
                start: segment_start,
                end: segment_start,
                display_width: 0,
            });
            break;
        }
    }

    if lines.is_empty() {
        lines.push(PromptInputVisualLine {
            start: 0,
            end: 0,
            display_width: 0,
        });
    }
    lines
}

fn append_wrapped_segment(
    input: &str,
    start: usize,
    end: usize,
    width: usize,
    lines: &mut Vec<PromptInputVisualLine>,
) {
    if start == end {
        lines.push(PromptInputVisualLine {
            start,
            end,
            display_width: 0,
        });
        return;
    }

    let mut line_start = start;
    let mut current_width = 0usize;
    for (relative, grapheme) in input[start..end].grapheme_indices(true) {
        let absolute = start + relative;
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if current_width > 0 && current_width.saturating_add(grapheme_width) > width {
            lines.push(PromptInputVisualLine {
                start: line_start,
                end: absolute,
                display_width: current_width,
            });
            line_start = absolute;
            current_width = 0;
        }
        // Never split a grapheme cluster or place half of a wide cluster at
        // the right edge. A cluster wider than the available row is kept as
        // one visual line; the cursor visibility check will reject a cell
        // that cannot fit in an extremely narrow frame.
        current_width = current_width.saturating_add(grapheme_width);
        if current_width >= width {
            let end_offset = absolute + grapheme.len();
            lines.push(PromptInputVisualLine {
                start: line_start,
                end: end_offset,
                display_width: current_width,
            });
            line_start = end_offset;
            current_width = 0;
        }
    }

    if line_start < end || current_width == 0 && lines.last().is_none_or(|line| line.end < end) {
        lines.push(PromptInputVisualLine {
            start: line_start,
            end,
            display_width: display_width_between(input, line_start, end),
        });
    }
}

fn visual_line_index_for_cursor(
    input: &str,
    lines: &[PromptInputVisualLine],
    cursor: usize,
) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let cursor = cursor.min(input.len());
    for (index, line) in lines.iter().enumerate() {
        if cursor < line.end {
            return index;
        }
        if cursor == line.end {
            let next_starts_at_cursor = lines
                .get(index + 1)
                .is_some_and(|next| next.start == cursor);
            if !next_starts_at_cursor {
                return index;
            }
        }
    }
    lines.len() - 1
}

fn display_width_between(input: &str, start: usize, end: usize) -> usize {
    input
        .get(start.min(input.len())..end.min(input.len()))
        .map(UnicodeWidthStr::width)
        .unwrap_or(0)
}

fn byte_offset_at_display_column(
    input: &str,
    line: &PromptInputVisualLine,
    desired_column: usize,
) -> usize {
    let mut column = 0usize;
    for (relative, grapheme) in input[line.start..line.end].grapheme_indices(true) {
        let width = UnicodeWidthStr::width(grapheme);
        if column.saturating_add(width) > desired_column {
            return line.start + relative;
        }
        column = column.saturating_add(width);
    }
    line.end
}

fn large_paste_notice(text: &str) -> Option<String> {
    let char_count = text.chars().count();
    let line_count = text.lines().count().max(1);
    if char_count < 512 && line_count < 4 {
        return None;
    }
    Some(format!(
        "Pasted {char_count} chars across {line_count} lines; the full text remains editable."
    ))
}

fn fill_input_background(area: Rect, buf: &mut Buffer) {
    let style = Style::default().bg(USER_INPUT_BACKGROUND);
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_style(style);
            }
        }
    }
}

fn with_input_background(style: Style) -> Style {
    style.bg(USER_INPUT_BACKGROUND)
}

impl Default for PromptInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn render_to_lines(input: &PromptInput, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        input.render_with_context(
            area,
            &mut buf,
            &Theme::default(),
            PromptInputRenderContext {
                hint: Some("hint"),
                placeholder: Some("Message allthecodes"),
                mode_indicator: Some("INS"),
            },
        );
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn prompt_input_resolves_placeholder_and_mode_indicator() {
        let input = PromptInput::new();
        let rendered = render_to_lines(&input, 60, 3).join("\n");
        assert!(rendered.contains("[INS]"));
        assert!(rendered.contains("> Message allthecodes"));
    }

    #[test]
    fn ghost_suffix_and_hint_use_the_same_rendered_input_row() {
        let mut input = PromptInput::new();
        input.insert_str("hello");
        input.set_ghost_suffix(Some(" world".to_string()));
        input.set_show_ghost(true);
        assert_eq!(input.ghost_suffix(), Some(" world"));
        assert!(input.show_ghost());

        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        let layout = input.render_with_hint(area, &mut buf, &Theme::default(), Some("hint"));
        let rendered: String = (0..area.width).map(|x| buf[(x, 1)].symbol()).collect();
        assert!(rendered.contains("hello world hint"));
        assert_eq!(layout.visual_height(), 1);
    }

    #[test]
    fn shift_enter_inserts_newline_and_enter_submits_full_text() {
        let mut input = PromptInput::new();
        input.insert_str("first");
        assert_eq!(
            input.handle_key(key(KeyCode::Enter, KeyModifiers::SHIFT)),
            None
        );
        input.insert_str("second");
        assert_eq!(input.input, "first\nsecond");
        assert_eq!(
            input.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some("first\nsecond".to_string())
        );
    }

    #[test]
    fn multiline_rows_share_the_same_text_origin() {
        let mut input = PromptInput::new();
        input.insert_str("first\nsecond");
        let lines = render_to_lines(&input, 40, 4);
        let first = lines
            .iter()
            .position(|line| line.contains("first"))
            .unwrap();
        let second = lines
            .iter()
            .position(|line| line.contains("second"))
            .unwrap();
        assert_eq!(lines[first].find("first"), lines[second].find("second"));
    }

    #[test]
    fn exact_width_input_gets_an_empty_caret_row() {
        let mut input = PromptInput::new();
        input.insert_str("abcd");
        let layout = input.layout(Rect::new(0, 0, 6, 4));
        assert_eq!(layout.visual_lines.len(), 2);
        assert_eq!(layout.caret_visual_line, 1);
        assert_eq!(layout.cursor_position(), Some(Position { x: 2, y: 2 }));
    }

    #[test]
    fn wrapped_and_hard_continuations_keep_prompt_prefix_width() {
        let mut input = PromptInput::new();
        input.insert_str("abcd\nefghij");
        let lines = render_to_lines(&input, 8, 5);
        let first_columns = lines
            .iter()
            .filter_map(|line| line.find('a').or_else(|| line.find('e')))
            .collect::<Vec<_>>();
        assert!(first_columns.iter().all(|column| *column == 2));
    }

    #[test]
    fn cursor_moves_over_combining_cluster_as_one_editing_unit() {
        let mut input = PromptInput::new();
        input.insert_str("e\u{301}x");
        input.cursor_position = input.input.len();
        assert_eq!(
            input.handle_key(key(KeyCode::Left, KeyModifiers::NONE)),
            None
        );
        assert_eq!(&input.input[..input.cursor_position], "e\u{301}");
        assert_eq!(
            input.handle_key(key(KeyCode::Left, KeyModifiers::NONE)),
            None
        );
        assert_eq!(input.cursor_position, 0);
    }

    #[test]
    fn layout_uses_display_columns_for_cjk_and_combining_marks() {
        let mut input = PromptInput::new();
        input.insert_str("你a\u{301}");
        input.cursor_position = "你".len();
        let layout = input.layout(Rect::new(10, 4, 30, 4));
        assert_eq!(layout.caret_display_column, 2);
        input.cursor_position = input.input.len();
        let combining_layout = input.layout(Rect::new(10, 4, 30, 4));
        assert_eq!(combining_layout.caret_display_column, 3);
        assert!(combining_layout.cursor_position().is_some());
    }

    #[test]
    fn viewport_keeps_caret_visible_after_soft_wrap() {
        let mut input = PromptInput::new();
        input.insert_str("abcdefghijklmno");
        input.cursor_position = input.input.len();
        let layout = input.layout(Rect::new(0, 0, 8, 4));
        assert!(layout.caret_visual_line >= layout.viewport_start);
        assert!(layout.cursor_position().is_some());
    }

    #[test]
    fn vertical_navigation_preserves_display_column_and_falls_back_at_edges() {
        let mut input = PromptInput::new();
        input.insert_str("12345\n12\n12345");
        input.cursor_position = input.input.find('\n').unwrap();
        assert!(input.move_cursor_vertical(20, 1));
        assert_eq!(&input.input[..input.cursor_position], "12345\n12");
        assert!(input.move_cursor_vertical(20, 1));
        assert_eq!(&input.input[..input.cursor_position], "12345\n12\n12345");
        assert!(!input.move_cursor_vertical(20, 1));
    }

    #[test]
    fn paste_normalizes_newlines_without_collapsing_small_multiline_text() {
        let mut input = PromptInput::new();
        input.paste_text("a\r\nb\rc");
        assert_eq!(input.input, "a\nb\nc");
        assert!(input.large_paste_notice().is_none());
    }

    #[test]
    fn large_paste_notice_does_not_replace_editable_text() {
        let mut input = PromptInput::new();
        let pasted = ["中文内容"; 200].join("\n");
        input.paste_text(&pasted);
        assert_eq!(input.input, pasted);
        assert!(input.large_paste_notice().is_some());
        assert!(input.take_large_paste_notice().is_some());
    }

    #[test]
    fn tiny_width_is_safe_and_does_not_claim_a_cursor_outside_the_frame() {
        let mut input = PromptInput::new();
        input.insert_str("hello");
        let area = Rect::new(0, 0, 4, 3);
        let mut buf = Buffer::empty(area);
        let layout = input.render(area, &mut buf, &Theme::default());
        assert!(layout.cursor_position().is_none() || layout.cursor_position().unwrap().x < 4);
    }
}
