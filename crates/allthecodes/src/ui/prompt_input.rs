use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

const USER_INPUT_BACKGROUND: Color = Color::Rgb(31, 35, 42);

/// A single-line text input widget with cursor support.
///
/// Handles common editing key bindings (arrows, home/end, ctrl shortcuts)
/// and returns `Some(text)` from [`handle_key`] when the user presses Enter.
///
/// Supports ghost suffix rendering: dimmed text shown after the cursor that
/// represents the active completion candidate.
pub struct PromptInput {
    /// Current input text.
    pub input: String,
    /// Byte-offset cursor position within `input`.
    ///
    /// Always kept on a char boundary.
    pub cursor_position: usize,
    /// Whether this widget is focused / accepting input.
    pub is_active: bool,
    /// Summary of the most recent large paste, shown by the app chrome only.
    large_paste_notice: Option<String>,
    /// Optional ghost suffix text shown dimmed after the cursor.
    ghost_suffix: Option<String>,
    /// Whether to show the ghost suffix.
    show_ghost: bool,
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
        }
    }

    /// Set the ghost suffix text (dimmed text shown after the cursor).
    /// Pass `None` to clear.
    pub fn set_ghost_suffix(&mut self, text: Option<String>) {
        self.ghost_suffix = text;
    }

    /// Set whether to show the ghost suffix.
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

    /// Handle a key event. Returns `Some(submitted_text)` when the user
    /// presses Enter with a non-empty input, clearing the internal buffer.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        if !self.is_active {
            return None;
        }

        match (key.modifiers, key.code) {
            // ── Submit ──────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::SHIFT, KeyCode::Enter) => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                self.input.clear();
                self.cursor_position = 0;
                self.large_paste_notice = None;
                return Some(text);
            }

            // ── Ctrl shortcuts ──────────────────────────────────────
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                // Clear entire line
                self.input.clear();
                self.cursor_position = 0;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                // Move to start of line (select-all semantics are tricky in
                // a terminal; we just move the cursor to the beginning).
                self.cursor_position = 0;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                // Move to end of line
                self.cursor_position = self.input.len();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                // Delete word backwards
                self.delete_word_backwards();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                // Kill to end of line
                self.input.truncate(self.cursor_position);
            }

            // ── Navigation ──────────────────────────────────────────
            (_, KeyCode::Left) => {
                self.move_cursor_left();
            }
            (_, KeyCode::Right) => {
                self.move_cursor_right();
            }
            (_, KeyCode::Home) => {
                self.cursor_position = 0;
            }
            (_, KeyCode::End) => {
                self.cursor_position = self.input.len();
            }

            // ── Deletion ────────────────────────────────────────────
            (_, KeyCode::Backspace) => {
                if self.cursor_position > 0 {
                    // Find the previous char boundary
                    let prev = self.prev_char_boundary();
                    self.input.drain(prev..self.cursor_position);
                    self.cursor_position = prev;
                }
            }
            (_, KeyCode::Delete) => {
                if self.cursor_position < self.input.len() {
                    let next = self.next_char_boundary();
                    self.input.drain(self.cursor_position..next);
                }
            }

            // ── Character input ─────────────────────────────────────
            (_, KeyCode::Char(c)) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += c.len_utf8();
            }

            _ => {}
        }

        None
    }

    /// Insert `text` at the current cursor position and advance the
    /// cursor past it. Used by voice dictation (issue #13) so
    /// transcribed text lands wherever the user was typing instead of
    /// being appended at the end.
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor_position, text);
        self.cursor_position += text.len();
    }

    /// Insert pasted text and remember a compact UI notice for large pastes.
    pub fn paste_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_str(&normalized);
        self.large_paste_notice = large_paste_notice(&normalized);
    }

    #[cfg(test)]
    pub fn take_large_paste_notice(&mut self) -> Option<String> {
        self.large_paste_notice.take()
    }

    pub fn large_paste_notice(&self) -> Option<&str> {
        self.large_paste_notice.as_deref()
    }

    /// Render the prompt input widget.
    ///
    /// Shows a "> " prompt prefix followed by the input text with a visible
    /// cursor indicator. The visible window scrolls horizontally when the
    /// cursor would move off-screen.
    #[cfg(test)]
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        self.render_with_context(area, buf, theme, PromptInputRenderContext::default());
    }

    /// Render the prompt input widget with a dim inline hint after the text.
    #[cfg(test)]
    pub fn render_with_hint(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        hint: Option<&str>,
    ) {
        self.render_with_context(
            area,
            buf,
            theme,
            PromptInputRenderContext {
                hint,
                placeholder: None,
                mode_indicator: None,
            },
        );
    }

    pub fn render_with_context(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        context: PromptInputRenderContext<'_>,
    ) {
        if area.height == 0 || area.width < 4 {
            return;
        }

        fill_input_background(area, buf);
        let text_y = input_text_y(area);

        let prompt_str = "> ";
        let prompt_span = Span::styled(prompt_str, with_input_background(theme.prompt));
        let prompt_width = 2u16; // "> " is always 2 columns

        let mode_width = context
            .mode_indicator
            .map(|label| UnicodeWidthStr::width(label) + 3)
            .unwrap_or(0);
        let available_width =
            (area.width.saturating_sub(prompt_width) as usize).saturating_sub(mode_width);

        if self.input.is_empty() {
            let mut spans = vec![prompt_span];
            if self.is_active {
                spans.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
                if let Some(placeholder) = context.placeholder {
                    spans.push(Span::styled(
                        format!(" {placeholder}"),
                        with_input_background(theme.dim),
                    ));
                }
            }
            push_mode_indicator(&mut spans, context.mode_indicator, theme);
            buf.set_line(area.x, text_y, &Line::from(spans), area.width);
            return;
        }

        let preview = input_preview(&self.input, available_width);
        let render_text = preview.as_deref().unwrap_or(&self.input);
        let render_cursor_position = if preview.is_some() {
            render_text.len()
        } else {
            self.cursor_position
        };

        // Compute the visible window of the input text. We track the cursor
        // as a *character* offset for display purposes.
        let char_cursor = render_text[..render_cursor_position].chars().count();
        let input_chars: Vec<char> = render_text.chars().collect();

        // Determine scroll offset so the cursor is always visible.
        let scroll = if available_width > 0 && char_cursor >= available_width {
            char_cursor - available_width + 1
        } else {
            0
        };

        let visible_end = (scroll + available_width).min(input_chars.len());
        let visible_text: String = input_chars[scroll..visible_end].iter().collect();

        // Build the cursor position within the visible region.
        let cursor_in_visible = char_cursor.saturating_sub(scroll).min(visible_text.len());

        // Split visible text around the cursor to insert styling.
        let before_cursor: String = visible_text.chars().take(cursor_in_visible).collect();
        let cursor_char: String = visible_text
            .chars()
            .nth(cursor_in_visible)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after_cursor: String = visible_text.chars().skip(cursor_in_visible + 1).collect();

        let mut spans = vec![prompt_span];

        if self.is_active {
            spans.push(Span::styled(
                before_cursor,
                Style::default().bg(USER_INPUT_BACKGROUND),
            ));
            spans.push(Span::styled(
                cursor_char,
                Style::default().fg(Color::Black).bg(Color::White),
            ));
            spans.push(Span::styled(
                after_cursor,
                Style::default().bg(USER_INPUT_BACKGROUND),
            ));
            // Ghost suffix: dimmed text after cursor showing completion
            if self.show_ghost {
                if let Some(suffix) = &self.ghost_suffix {
                    if !suffix.is_empty() && cursor_in_visible >= visible_text.len() {
                        spans.push(Span::styled(
                            suffix.clone(),
                            with_input_background(theme.dim),
                        ));
                    }
                }
            }
            if let Some(hint) = context
                .hint
                .filter(|_| cursor_in_visible >= visible_text.len())
            {
                spans.push(Span::styled(
                    format!(" {hint}"),
                    with_input_background(theme.dim),
                ));
            }
        } else {
            spans.push(Span::styled(visible_text, with_input_background(theme.dim)));
        }
        push_mode_indicator(&mut spans, context.mode_indicator, theme);

        let line = Line::from(spans);
        buf.set_line(area.x, text_y, &line, area.width);
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position = self.prev_char_boundary();
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position = self.next_char_boundary();
        }
    }

    /// Find the byte offset of the previous character boundary.
    fn prev_char_boundary(&self) -> usize {
        let mut pos = self.cursor_position;
        if pos == 0 {
            return 0;
        }
        pos -= 1;
        while pos > 0 && !self.input.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    /// Find the byte offset of the next character boundary.
    fn next_char_boundary(&self) -> usize {
        let mut pos = self.cursor_position;
        if pos >= self.input.len() {
            return self.input.len();
        }
        pos += 1;
        while pos < self.input.len() && !self.input.is_char_boundary(pos) {
            pos += 1;
        }
        pos
    }

    /// Delete the word before the cursor (Ctrl+W behaviour).
    fn delete_word_backwards(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let bytes = self.input.as_bytes();
        let mut end = self.cursor_position;
        // Skip trailing whitespace
        while end > 0 && bytes[end - 1] == b' ' {
            end -= 1;
        }
        // Skip non-whitespace (the word)
        let start = {
            let mut s = end;
            while s > 0 && bytes[s - 1] != b' ' {
                s -= 1;
            }
            s
        };
        self.input.drain(start..self.cursor_position);
        self.cursor_position = start;
    }
}

fn input_preview(input: &str, available_width: usize) -> Option<String> {
    let char_count = input.chars().count();
    let line_count = input.lines().count().max(1);
    let is_large = line_count > 1 || char_count > available_width.saturating_mul(2).max(80);
    if !is_large {
        return None;
    }

    let max_preview = available_width.saturating_sub(24).clamp(16, 96);
    let first_line = input.lines().next().unwrap_or(input).trim();
    let mut preview: String = first_line.chars().take(max_preview).collect();
    if first_line.chars().count() > max_preview || line_count > 1 {
        preview.push_str("...");
    }
    Some(format!(
        "[{} chars, {} lines pasted] {}",
        char_count, line_count, preview
    ))
}

fn large_paste_notice(text: &str) -> Option<String> {
    let char_count = text.chars().count();
    let line_count = text.lines().count().max(1);
    if char_count < 512 && line_count < 4 {
        return None;
    }
    Some(format!(
        "Pasted {} chars across {} lines; preview is truncated in the UI only.",
        char_count, line_count
    ))
}

fn push_mode_indicator(
    spans: &mut Vec<Span<'static>>,
    mode_indicator: Option<&str>,
    theme: &Theme,
) {
    if let Some(label) = mode_indicator.filter(|label| !label.is_empty()) {
        spans.push(Span::styled(
            format!("  [{label}]"),
            with_input_background(theme.dim),
        ));
    }
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

fn input_text_y(area: Rect) -> u16 {
    if area.height >= 3 {
        area.y.saturating_add(1)
    } else {
        area.y
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

    fn render_to_string(
        input: &PromptInput,
        width: u16,
        context: PromptInputRenderContext<'_>,
    ) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        input.render_with_context(area, &mut buf, &Theme::default(), context);
        (0..width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn prompt_input_resolves_placeholder_and_mode_indicator() {
        let input = PromptInput::new();
        let rendered = render_to_string(
            &input,
            60,
            PromptInputRenderContext {
                hint: None,
                placeholder: Some("Message allthecodes"),
                mode_indicator: Some("INS"),
            },
        );

        assert!(rendered.starts_with(">"));
        assert!(rendered.contains("Message allthecodes"));
        assert!(rendered.contains("[INS]"));
    }

    #[test]
    fn prompt_input_large_paste_is_ui_preview_only() {
        let mut input = PromptInput::new();
        let pasted = ["alpha beta gamma"; 60].join("\n");
        input.paste_text(&pasted);

        assert_eq!(input.input, pasted);
        assert!(input.large_paste_notice().is_some());

        let rendered = render_to_string(
            &input,
            80,
            PromptInputRenderContext {
                hint: None,
                placeholder: None,
                mode_indicator: Some("INS"),
            },
        );
        assert!(rendered.contains("chars"));
        assert!(rendered.contains("lines pasted"));
    }

    #[test]
    fn prompt_input_tiny_width_with_mode_indicator_does_not_panic() {
        let mut input = PromptInput::new();
        input.insert_str("hello");

        let rendered = render_to_string(
            &input,
            4,
            PromptInputRenderContext {
                hint: None,
                placeholder: None,
                mode_indicator: Some("INSERT"),
            },
        );

        assert!(rendered.starts_with(">"));
    }

    #[test]
    fn ghost_accessors_and_legacy_render_helpers_are_exercised() {
        let mut input = PromptInput::new();
        input.insert_str("/he");
        input.set_ghost_suffix(Some("lp".to_string()));
        input.set_show_ghost(true);

        assert_eq!(input.ghost_suffix(), Some("lp"));
        assert!(input.show_ghost());

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf, &Theme::default());
        let rendered: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(rendered.contains("/he"));

        let mut hint_buf = Buffer::empty(area);
        input.render_with_hint(area, &mut hint_buf, &Theme::default(), Some("hint"));
        let hinted: String = (0..area.width).map(|x| hint_buf[(x, 0)].symbol()).collect();
        assert!(hinted.contains("/he"));
    }

    #[test]
    fn large_paste_notice_can_be_taken_for_chrome() {
        let mut input = PromptInput::new();
        input.paste_text(&["line"; 12].join("\n"));

        assert!(input.large_paste_notice().is_some());
        assert!(input.take_large_paste_notice().is_some());
        assert!(input.large_paste_notice().is_none());
    }

    #[test]
    fn prompt_input_paints_user_message_background_across_row() {
        let mut input = PromptInput::new();
        input.insert_str("hello");
        let area = Rect::new(0, 0, 24, 3);
        let mut buf = Buffer::empty(area);

        input.render_with_context(
            area,
            &mut buf,
            &Theme::default(),
            PromptInputRenderContext {
                hint: None,
                placeholder: None,
                mode_indicator: Some("INS"),
            },
        );

        for y in 0..3 {
            assert_eq!(buf[(0, y)].style().bg, Some(USER_INPUT_BACKGROUND));
            assert_eq!(buf[(23, y)].style().bg, Some(USER_INPUT_BACKGROUND));
        }

        let top: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        let middle: String = (0..area.width).map(|x| buf[(x, 1)].symbol()).collect();
        let bottom: String = (0..area.width).map(|x| buf[(x, 2)].symbol()).collect();
        assert!(top.trim().is_empty());
        assert!(middle.contains("hello"));
        assert!(bottom.trim().is_empty());
    }
}
