//! Direct TUI dialog for AskUserQuestion tool prompts.

use allthecodes_types::callbacks::AskUserRequestPayload;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
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
                self.cursor = (self.cursor + 1).min(self.answer.chars().count());
            }
            (_, KeyCode::Home) => self.cursor = 0,
            (_, KeyCode::End) => self.cursor = self.answer.chars().count(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(ch))
                if self.request.allow_free_text =>
            {
                self.insert(ch);
            }
            _ => {}
        }
        None
    }

    pub fn render(&self, area: Rect, prompt_area: Option<Rect>, buf: &mut Buffer, theme: &Theme) {
        let spec = PanelSizePreset::QuestionDialog.spec();
        let dialog_area = spec
            .resolve_prompt_or_centered_rect(area, prompt_area, spec.max_height)
            .unwrap_or(Rect::new(area.x, area.y, area.width, area.height));

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
                Span::styled(
                    truncate(&self.answer_preview(), chunks[0].width as usize),
                    theme.warning,
                ),
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

    fn answer_with_cursor(&self) -> String {
        let mut chars = self.answer.chars().collect::<Vec<_>>();
        let cursor = self.cursor.min(chars.len());
        chars.insert(cursor, '|');
        chars.into_iter().collect()
    }

    fn answer_preview(&self) -> String {
        if self.request.allow_free_text {
            self.answer_with_cursor()
        } else {
            self.selected_choice_text()
                .unwrap_or_else(|| "<select a choice>".to_string())
        }
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
        let mut chars = self.answer.chars().collect::<Vec<_>>();
        let cursor = self.cursor.min(chars.len());
        chars.insert(cursor, ch);
        self.answer = chars.into_iter().collect();
        self.cursor = cursor + 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars = self.answer.chars().collect::<Vec<_>>();
        let idx = self.cursor.saturating_sub(1);
        if idx < chars.len() {
            chars.remove(idx);
            self.answer = chars.into_iter().collect();
            self.cursor = idx;
        }
    }

    fn delete(&mut self) {
        let mut chars = self.answer.chars().collect::<Vec<_>>();
        if self.cursor < chars.len() {
            chars.remove(self.cursor);
            self.answer = chars.into_iter().collect();
        }
    }
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else if max_chars <= 3 {
        input.chars().take(max_chars).collect()
    } else {
        input
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>()
            + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::QuestionDialog;
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
