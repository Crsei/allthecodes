//! Confirmation dialog for bypass permissions mode.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::ui::panel_layout::PanelSizePreset;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassPermissionsModeChoice {
    Decline,
    Accept,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BypassPermissionsModeDialog {
    selected: usize,
    disabled: bool,
}

impl BypassPermissionsModeDialog {
    pub fn new(disabled: bool) -> Self {
        Self {
            selected: usize::from(!disabled),
            disabled,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<BypassPermissionsModeChoice> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.selected = 0,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab if !self.disabled => {
                self.selected = 1;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                return Some(BypassPermissionsModeChoice::Decline);
            }
            KeyCode::Char('y') | KeyCode::Char('Y') if !self.disabled => {
                return Some(BypassPermissionsModeChoice::Accept);
            }
            KeyCode::Enter => {
                return Some(if self.selected == 1 && !self.disabled {
                    BypassPermissionsModeChoice::Accept
                } else {
                    BypassPermissionsModeChoice::Decline
                });
            }
            _ => {}
        }
        None
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let spec = PanelSizePreset::BypassPermissionsMode.spec();
        let dialog_area = spec
            .resolve_rect(area, spec.max_height)
            .unwrap_or(Rect::new(area.x, area.y, area.width, area.height));

        Clear.render(dialog_area, buf);

        let block = Block::default()
            .title(" WARNING: Bypass Permissions mode ")
            .borders(Borders::ALL)
            .border_style(theme.error.add_modifier(Modifier::BOLD))
            .style(Style::default());
        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(inner);

        let body = if self.disabled {
            "Bypass permissions mode is disabled by settings policy. Exit this session or choose another permission mode."
        } else {
            "allthecodes will not ask before running potentially dangerous commands. Use this only in a sandboxed container or VM that can be restored if damaged.\n\nBy proceeding, you accept responsibility for actions taken in this mode."
        };
        Paragraph::new(body)
            .wrap(Wrap { trim: true })
            .render(chunks[0], buf);

        let labels = if self.disabled {
            ["Exit", "Disabled"]
        } else {
            ["No, exit", "Yes, I accept"]
        };
        let line = Line::from(vec![
            button_span(labels[0], self.selected == 0, theme),
            Span::raw("  "),
            button_span(labels[1], self.selected == 1, theme),
        ]);
        buf.set_line(chunks[1].x + 1, chunks[1].y, &line, chunks[1].width);

        let hint = if self.disabled {
            "Enter/Esc exits"
        } else {
            "Left/Right select. Enter confirms. Esc exits."
        };
        buf.set_line(
            chunks[2].x + 1,
            chunks[2].y,
            &Line::from(Span::styled(hint, theme.dim)),
            chunks[2].width,
        );
    }
}

fn button_span(label: &str, selected: bool, theme: &Theme) -> Span<'static> {
    if selected {
        Span::styled(
            format!("[ {label} ]"),
            theme.warning.add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!("  {label}  "), theme.dim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn accepts_when_enabled_and_yes_is_selected() {
        let mut dialog = BypassPermissionsModeDialog::new(false);
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(BypassPermissionsModeChoice::Accept)
        );
    }

    #[test]
    fn disabled_dialog_cannot_accept() {
        let mut dialog = BypassPermissionsModeDialog::new(true);
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(BypassPermissionsModeChoice::Decline)
        );
    }
}
