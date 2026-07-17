//! Direct TUI dialog for AskUserQuestion tool prompts.

use allthecodes_types::callbacks::AskUserRequestPayload;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::ui::permissions::ask_user_question_permission_request::ask_user_question_permission_request::render_ask_user_question_permission_request;
use crate::ui::permissions::ask_user_question_permission_request::preview_box::render_preview_box;
use crate::ui::permissions::ask_user_question_permission_request::preview_question_view::render_preview_question_view;
use crate::ui::permissions::ask_user_question_permission_request::submit_questions_view::render_submit_questions_view;
use crate::ui::permissions::ask_user_question_permission_request::use_multiple_choice_state::MultipleChoiceState;
use crate::ui::panel_layout::PanelSizePreset;
use crate::ui::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDialog {
    pub id: String,
    pub request: AskUserRequestPayload,
    answer: String,
    cursor: usize,
    choices: MultipleChoiceState,
}

impl QuestionDialog {
    pub fn new(id: impl Into<String>, request: AskUserRequestPayload) -> Self {
        Self {
            id: id.into(),
            choices: MultipleChoiceState::new(request.choices.clone()),
            request,
            answer: String::new(),
            cursor: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Enter) => return Some(self.submit_answer()),
            (_, KeyCode::Esc) => return Some(String::new()),
            (_, KeyCode::Up) => {
                self.choices.selected = self.choices.selected.saturating_sub(1);
            }
            (_, KeyCode::Char('k')) if !self.request.allow_free_text => {
                self.choices.selected = self.choices.selected.saturating_sub(1);
            }
            (_, KeyCode::Down) => {
                if !self.choices.options.is_empty() {
                    self.choices.select_next();
                }
            }
            (_, KeyCode::Char('j')) if !self.request.allow_free_text => {
                if !self.choices.options.is_empty() {
                    self.choices.select_next();
                }
            }
            (_, KeyCode::Char(' ')) if !self.request.allow_free_text => {
                self.choices.toggle_selected();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.answer.clear();
                self.cursor = 0;
            }
            (_, KeyCode::Backspace) => self.backspace(),
            (_, KeyCode::Delete) => self.delete(),
            (_, KeyCode::Left) => self.cursor = self.cursor.saturating_sub(1),
            (_, KeyCode::Right) => {
                self.cursor = (self.cursor + 1).min(self.answer.graphemes(true).count());
            }
            (_, KeyCode::Home) => self.cursor = 0,
            (_, KeyCode::End) => self.cursor = self.answer.graphemes(true).count(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(ch))
                if self.request.allow_free_text =>
            {
                self.insert(ch);
            }
            _ => {}
        }
        None
    }

    pub fn allows_free_text(&self) -> bool {
        self.request.allow_free_text
    }

    /// Return the physical cursor cell for the free-text answer field. The
    /// rendered header and this coordinate intentionally share the same
    /// `Answer: ` prefix; no pipe character is drawn into the buffer.
    pub fn cursor_position(&self, area: Rect, prompt_area: Option<Rect>) -> Option<Position> {
        if !self.request.allow_free_text {
            return None;
        }
        let dialog_area = self.dialog_area(area, prompt_area)?;
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(dialog_area);
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);
        let answer_width = usize::from(chunks[0].width)
            .saturating_sub(UnicodeWidthStr::width("Answer: "))
            .saturating_sub(1);
        let (_, cursor_column) =
            answer_preview_with_cursor(&self.answer, self.cursor, answer_width);
        let x = chunks[0]
            .x
            .saturating_add(UnicodeWidthStr::width("Answer: ") as u16)
            .saturating_add(cursor_column.min(u16::MAX as usize) as u16);
        let y = chunks[0].y.saturating_add(1);
        let right = chunks[0].x.saturating_add(chunks[0].width);
        (x < right && y < chunks[0].y.saturating_add(chunks[0].height)).then_some(Position { x, y })
    }

    pub fn render(&self, area: Rect, prompt_area: Option<Rect>, buf: &mut Buffer, theme: &Theme) {
        let Some(dialog_area) = self.dialog_area(area, prompt_area) else {
            return;
        };

        Widget::render(Clear, dialog_area, buf);

        let block = Block::default()
            .title(" Need Input ")
            .borders(Borders::ALL)
            .border_style(theme.info)
            .style(Style::default());
        let inner = block.inner(dialog_area);
        Widget::render(block, dialog_area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);
        let answer_width = usize::from(chunks[0].width)
            .saturating_sub(UnicodeWidthStr::width("Answer: "))
            .saturating_sub(1);
        let (answer_text, _) = answer_preview_with_cursor(&self.answer, self.cursor, answer_width);

        let header = vec![
            Line::from(vec![
                Span::styled("Question: ", theme.dim),
                Span::styled(
                    truncate(&self.request.question, chunks[0].width as usize),
                    theme.info,
                ),
            ]),
            Line::from(vec![
                Span::styled("Answer: ", theme.dim),
                Span::styled(answer_text, theme.warning),
            ]),
        ];
        Widget::render(Paragraph::new(header), chunks[0], buf);

        let body = self.rendered_body_lines(chunks[1].width as usize);
        Widget::render(
            Paragraph::new(body).wrap(Wrap { trim: true }),
            chunks[1],
            buf,
        );

        let footer_width = chunks[2].width.saturating_sub(2) as usize;
        let hint_text = if self.request.allow_free_text && !self.choices.options.is_empty() {
            "Type an answer or use arrows for choices. Enter submits. Esc sends an empty answer."
        } else if self.request.allow_free_text {
            "Type an answer. Enter submits. Esc sends an empty answer."
        } else if !self.choices.options.is_empty() {
            "Use arrows for choices. Space toggles. Enter submits. Esc sends an empty answer."
        } else {
            "Enter submits an empty answer. Esc sends an empty answer."
        };
        let hint = Line::from(Span::styled(truncate(hint_text, footer_width), theme.dim));
        buf.set_line(chunks[2].x + 1, chunks[2].y, &hint, footer_width as u16);
    }

    fn rendered_body_lines(&self, width: usize) -> Vec<Line<'static>> {
        let answer_preview = if self.answer.trim().is_empty() {
            self.selected_choice_text()
                .unwrap_or_else(|| "<empty answer>".to_string())
        } else {
            self.answer.clone()
        };
        let mut rendered = if self.choices.options.is_empty() {
            render_ask_user_question_permission_request(
                &self.request.question,
                &MultipleChoiceState::new(vec![answer_preview.clone()]),
                1,
                1,
            )
        } else {
            render_ask_user_question_permission_request(&self.request.question, &self.choices, 1, 1)
        };
        rendered.push_str("\n\n");
        rendered.push_str(&render_preview_box(
            "Preview",
            &render_preview_question_view(&self.request.question, &[answer_preview.as_str()]),
        ));
        rendered.push('\n');
        rendered.push_str(&render_submit_questions_view(
            usize::from(!self.submit_answer().trim().is_empty()),
            1,
        ));
        rendered
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(10)
            .map(|line| Line::from(truncate(line, width)))
            .collect()
    }

    fn dialog_area(&self, area: Rect, prompt_area: Option<Rect>) -> Option<Rect> {
        let spec = PanelSizePreset::QuestionDialog.spec();
        spec.resolve_prompt_or_centered_rect(area, prompt_area, spec.max_height)
            .or_else(|| (area.width > 0 && area.height > 0).then_some(area))
    }

    fn selected_choice_text(&self) -> Option<String> {
        self.request
            .choices
            .get(
                self.choices
                    .selected
                    .min(self.request.choices.len().saturating_sub(1)),
            )
            .cloned()
    }

    fn submit_answer(&self) -> String {
        if !self.answer.trim().is_empty() {
            self.answer.clone()
        } else if !self.choices.submitted.is_empty() {
            self.choices
                .submitted
                .iter()
                .filter_map(|idx| self.choices.options.get(*idx))
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            self.selected_choice_text().unwrap_or_default()
        }
    }

    fn insert(&mut self, ch: char) {
        let cursor = self.cursor.min(self.answer.graphemes(true).count());
        let byte_offset = self
            .answer
            .grapheme_indices(true)
            .nth(cursor)
            .map(|(offset, _)| offset)
            .unwrap_or(self.answer.len());
        self.answer.insert(byte_offset, ch);
        self.cursor = self.answer[..byte_offset + ch.len_utf8()]
            .graphemes(true)
            .count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let ranges = self.answer.grapheme_indices(true).collect::<Vec<_>>();
        let idx = self
            .cursor
            .saturating_sub(1)
            .min(ranges.len().saturating_sub(1));
        if let Some((start, _)) = ranges.get(idx) {
            let end = ranges
                .get(idx + 1)
                .map(|(offset, _)| *offset)
                .unwrap_or(self.answer.len());
            self.answer.drain(*start..end);
            self.cursor = idx;
        }
    }

    fn delete(&mut self) {
        let ranges = self.answer.grapheme_indices(true).collect::<Vec<_>>();
        if let Some((start, _)) = ranges.get(self.cursor) {
            let end = ranges
                .get(self.cursor + 1)
                .map(|(offset, _)| *offset)
                .unwrap_or(self.answer.len());
            self.answer.drain(*start..end);
        }
    }
}

fn truncate(input: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(input) <= max_width {
        input.to_string()
    } else {
        let (content_width, suffix) = if max_width > 3 {
            (max_width - 3, "...")
        } else {
            (max_width, "")
        };
        let mut width = 0usize;
        let mut output = String::new();
        for grapheme in input.graphemes(true) {
            let ch_width = UnicodeWidthStr::width(grapheme);
            if width.saturating_add(ch_width) > content_width {
                break;
            }
            output.push_str(grapheme);
            width = width.saturating_add(ch_width);
        }
        output.push_str(suffix);
        output
    }
}

/// Render a horizontally windowed answer while keeping the scalar caret
/// visible. The returned column is measured in terminal cells from the start
/// of the answer field, so the drawing path and [`QuestionDialog::cursor_position`]
/// share the same truncation decision for long answers.
fn answer_preview_with_cursor(answer: &str, cursor: usize, max_width: usize) -> (String, usize) {
    let graphemes = answer
        .graphemes(true)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let widths = graphemes
        .iter()
        .map(|grapheme| UnicodeWidthStr::width(grapheme.as_str()))
        .collect::<Vec<_>>();
    let cursor = cursor.min(graphemes.len());
    let total_width = widths.iter().sum::<usize>();
    if total_width <= max_width {
        return (answer.to_string(), widths[..cursor].iter().sum::<usize>());
    }
    if max_width == 0 {
        return (String::new(), 0);
    }

    // Reserve room for one marker on a short field and both markers when the
    // caret is in the middle. A marker is only emitted if the corresponding
    // side is still hidden after the window is selected.
    let has_left = cursor > 0;
    let has_right = cursor < graphemes.len();
    let marker_slots = if max_width >= 6 && has_left && has_right {
        2
    } else if max_width >= 6 && (has_left || has_right) {
        1
    } else {
        0
    };
    let content_width = max_width.saturating_sub(marker_slots * 3);
    let (reserve_prefix, reserve_suffix) = match marker_slots {
        2 => (true, true),
        1 if has_left && has_right => {
            let left = widths[..cursor].iter().sum::<usize>();
            let right = widths[cursor..].iter().sum::<usize>();
            (left >= right, left < right)
        }
        1 => (has_left, has_right),
        _ => (false, false),
    };

    let mut start = cursor;
    let mut end = cursor;
    let mut used_width = 0usize;
    while start > 0 || end < graphemes.len() {
        let left_width = start
            .checked_sub(1)
            .and_then(|index| widths.get(index).copied());
        let right_width = widths.get(end).copied();
        let can_take_left =
            left_width.is_some_and(|width| used_width.saturating_add(width) <= content_width);
        let can_take_right =
            right_width.is_some_and(|width| used_width.saturating_add(width) <= content_width);
        if !can_take_left && !can_take_right {
            break;
        }

        let take_left = match (can_take_left, can_take_right) {
            (true, false) => true,
            (false, true) => false,
            (true, true) => {
                let left = left_width.unwrap_or(0);
                let right = right_width.unwrap_or(0);
                left <= right
            }
            (false, false) => false,
        };
        if take_left {
            start -= 1;
            used_width = used_width.saturating_add(left_width.unwrap_or(0));
        } else {
            used_width = used_width.saturating_add(right_width.unwrap_or(0));
            end += 1;
        }
    }

    let show_prefix = reserve_prefix && start > 0;
    let show_suffix = reserve_suffix && end < graphemes.len();
    let mut output = String::new();
    if show_prefix {
        output.push_str("...");
    }
    for grapheme in &graphemes[start..end] {
        output.push_str(grapheme);
    }
    if show_suffix {
        output.push_str("...");
    }
    let cursor_column =
        usize::from(show_prefix) * 3 + widths[start..cursor.min(end)].iter().sum::<usize>();
    (output, cursor_column)
}

#[cfg(test)]
mod tests {
    use super::{answer_preview_with_cursor, QuestionDialog};
    use allthecodes_types::callbacks::AskUserRequestPayload;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use crate::ui::theme::Theme;

    #[test]
    fn question_dialog_collects_answer() {
        let mut dialog = QuestionDialog::new(
            "q-1",
            AskUserRequestPayload {
                question: "Continue?".to_string(),
                choices: vec![],
                allow_free_text: true,
            },
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some("yes".to_string())
        );
    }

    #[test]
    fn question_dialog_returns_selected_choice_when_free_text_is_disabled() {
        let mut dialog = QuestionDialog::new(
            "q-1",
            AskUserRequestPayload {
                question: "Choose a path".to_string(),
                choices: vec!["safe".to_string(), "fast".to_string()],
                allow_free_text: false,
            },
        );

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some("fast".to_string())
        );
    }

    #[test]
    fn question_dialog_keeps_j_and_k_as_free_text_when_allowed() {
        let mut dialog = QuestionDialog::new(
            "q-1",
            AskUserRequestPayload {
                question: "Any notes?".to_string(),
                choices: vec!["No".to_string(), "Yes".to_string()],
                allow_free_text: true,
            },
        );

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some("jk".to_string())
        );
    }

    #[test]
    fn question_dialog_can_render_above_prompt() {
        let dialog = QuestionDialog::new(
            "q-1",
            AskUserRequestPayload {
                question: "Continue?".to_string(),
                choices: vec!["yes".to_string(), "no".to_string()],
                allow_free_text: false,
            },
        );
        let area = Rect::new(0, 0, 100, 30);
        let prompt_area = Rect::new(0, 24, 100, 3);
        let mut buffer = Buffer::empty(area);

        dialog.render(area, Some(prompt_area), &mut buffer, &Theme::default());

        let title_row = row_containing(&buffer, area, "Need Input").expect("title row");
        assert!(title_row < prompt_area.y);
    }

    #[test]
    fn long_answer_preview_keeps_a_display_column_cursor() {
        let answer = "ab你中文defgh";
        let (preview, cursor_column) = answer_preview_with_cursor(answer, 4, 8);

        assert!(unicode_width::UnicodeWidthStr::width(preview.as_str()) <= 8);
        assert!(cursor_column <= unicode_width::UnicodeWidthStr::width(preview.as_str()));
        assert!(preview.contains("你"));
    }

    #[test]
    fn answer_preview_cursor_is_stable_at_each_caret_edge() {
        let answer = "你abc中文";
        for cursor in 0..=answer.chars().count() {
            let (preview, cursor_column) = answer_preview_with_cursor(answer, cursor, 7);
            assert!(unicode_width::UnicodeWidthStr::width(preview.as_str()) <= 7);
            assert!(cursor_column <= unicode_width::UnicodeWidthStr::width(preview.as_str()));
        }
    }

    fn row_containing(buffer: &Buffer, area: Rect, needle: &str) -> Option<u16> {
        (area.y..area.y + area.height).find(|&y| {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.contains(needle)
        })
    }
}
