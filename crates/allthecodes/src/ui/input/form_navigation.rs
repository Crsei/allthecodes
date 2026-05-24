//! Reusable keyboard state for tabbed forms and picker-style dialogs.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
}

impl FormOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: String::new(),
            enabled: true,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormTab {
    pub id: String,
    pub label: String,
    pub options: Vec<FormOption>,
}

impl FormTab {
    pub fn new(id: impl Into<String>, label: impl Into<String>, options: Vec<FormOption>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabbedFormEvent {
    None,
    TabChanged { tab_index: usize, tab_id: String },
    SelectionChanged { selected_index: usize },
    Selected { tab_id: String, option_id: String },
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabbedFormState {
    pub title: String,
    pub tabs: Vec<FormTab>,
    pub active_tab: usize,
    pub selected_index: usize,
    pub closed: bool,
}

impl TabbedFormState {
    pub fn new(title: impl Into<String>, tabs: Vec<FormTab>) -> Self {
        Self {
            title: title.into(),
            tabs,
            active_tab: 0,
            selected_index: 0,
            closed: false,
        }
    }

    pub fn active_tab(&self) -> Option<&FormTab> {
        self.tabs.get(self.active_tab)
    }

    pub fn selected_option(&self) -> Option<&FormOption> {
        self.active_tab()
            .and_then(|tab| tab.options.get(self.selected_index))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TabbedFormEvent {
        if key.kind != KeyEventKind::Press {
            return TabbedFormEvent::None;
        }

        match key.code {
            KeyCode::Left | KeyCode::Char('[') => self.previous_tab(),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char(']') => self.next_tab(),
            KeyCode::BackTab => self.previous_tab(),
            KeyCode::Up | KeyCode::Char('k') => self.previous_option(),
            KeyCode::Down | KeyCode::Char('j') => self.next_option(),
            KeyCode::Enter => self.select_current(),
            KeyCode::Esc => {
                self.closed = true;
                TabbedFormEvent::Closed
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let index = ch.to_digit(10).unwrap_or(0).saturating_sub(1) as usize;
                self.jump_tab(index)
            }
            _ => TabbedFormEvent::None,
        }
    }

    pub fn render_lines(&self) -> Vec<String> {
        use crate::ui::better_view_panel::{selected_row, BetterViewPanel};

        let sections = self
            .tabs
            .iter()
            .map(|tab| tab.label.clone())
            .collect::<Vec<_>>();
        let (detail_title, detail_lines) = if let Some(tab) = self.active_tab() {
            let detail_lines = tab
                .options
                .iter()
                .enumerate()
                .map(|(idx, option)| {
                    let mut detail = option.description.clone();
                    if !option.enabled {
                        if detail.is_empty() {
                            detail = "disabled".to_string();
                        } else {
                            detail.push_str(" disabled");
                        }
                    }
                    selected_row(&option.label, detail, idx == self.selected_index)
                })
                .collect::<Vec<_>>();
            (tab.label.clone(), detail_lines)
        } else {
            ("Detail".to_string(), Vec::new())
        };

        BetterViewPanel::new(&self.title)
            .sections(sections, self.active_tab)
            .detail_title(detail_title)
            .detail_lines(detail_lines)
            .footer("Left/Right section | Up/Down navigate | Enter select | Esc close")
            .render_lines()
    }

    fn previous_tab(&mut self) -> TabbedFormEvent {
        if self.tabs.is_empty() {
            return TabbedFormEvent::None;
        }
        self.active_tab = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
        self.clamp_selection();
        self.current_tab_event()
    }

    fn next_tab(&mut self) -> TabbedFormEvent {
        if self.tabs.is_empty() {
            return TabbedFormEvent::None;
        }
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
        self.clamp_selection();
        self.current_tab_event()
    }

    fn jump_tab(&mut self, index: usize) -> TabbedFormEvent {
        if index >= self.tabs.len() {
            return TabbedFormEvent::None;
        }
        self.active_tab = index;
        self.clamp_selection();
        self.current_tab_event()
    }

    fn previous_option(&mut self) -> TabbedFormEvent {
        let Some(count) = self.active_option_count() else {
            return TabbedFormEvent::None;
        };
        self.selected_index = if self.selected_index == 0 {
            count - 1
        } else {
            self.selected_index - 1
        };
        TabbedFormEvent::SelectionChanged {
            selected_index: self.selected_index,
        }
    }

    fn next_option(&mut self) -> TabbedFormEvent {
        let Some(count) = self.active_option_count() else {
            return TabbedFormEvent::None;
        };
        self.selected_index = (self.selected_index + 1) % count;
        TabbedFormEvent::SelectionChanged {
            selected_index: self.selected_index,
        }
    }

    fn select_current(&self) -> TabbedFormEvent {
        let Some(tab) = self.active_tab() else {
            return TabbedFormEvent::None;
        };
        let Some(option) = tab.options.get(self.selected_index) else {
            return TabbedFormEvent::None;
        };
        if !option.enabled {
            return TabbedFormEvent::None;
        }
        TabbedFormEvent::Selected {
            tab_id: tab.id.clone(),
            option_id: option.id.clone(),
        }
    }

    fn active_option_count(&self) -> Option<usize> {
        let count = self.active_tab()?.options.len();
        (count > 0).then_some(count)
    }

    fn clamp_selection(&mut self) {
        match self.active_option_count() {
            Some(count) => self.selected_index = self.selected_index.min(count - 1),
            None => self.selected_index = 0,
        }
    }

    fn current_tab_event(&self) -> TabbedFormEvent {
        let Some(tab) = self.active_tab() else {
            return TabbedFormEvent::None;
        };
        TabbedFormEvent::TabChanged {
            tab_index: self.active_tab,
            tab_id: tab.id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn form() -> TabbedFormState {
        TabbedFormState::new(
            "Settings",
            vec![
                FormTab::new(
                    "status",
                    "Status",
                    vec![
                        FormOption::new("summary", "Summary"),
                        FormOption::new("diagnostics", "Diagnostics"),
                    ],
                ),
                FormTab::new(
                    "config",
                    "Config",
                    vec![
                        FormOption::new("vim", "Vim mode"),
                        FormOption::new("model", "Model").disabled(),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn arrows_switch_tabs_and_navigate_options() {
        let mut state = form();

        assert_eq!(
            state.handle_key(key(KeyCode::Right)),
            TabbedFormEvent::TabChanged {
                tab_index: 1,
                tab_id: "config".to_string(),
            }
        );
        assert_eq!(state.active_tab, 1);

        assert_eq!(
            state.handle_key(key(KeyCode::Down)),
            TabbedFormEvent::SelectionChanged { selected_index: 1 }
        );
        assert_eq!(state.selected_index, 1);

        assert_eq!(
            state.handle_key(key(KeyCode::Up)),
            TabbedFormEvent::SelectionChanged { selected_index: 0 }
        );
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn enter_selects_enabled_option_and_esc_closes() {
        let mut state = form();
        state.handle_key(key(KeyCode::Right));

        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            TabbedFormEvent::Selected {
                tab_id: "config".to_string(),
                option_id: "vim".to_string(),
            }
        );
        assert_eq!(state.handle_key(key(KeyCode::Esc)), TabbedFormEvent::Closed);
        assert!(state.closed);
    }

    #[test]
    fn disabled_option_does_not_submit() {
        let mut state = form();
        state.handle_key(key(KeyCode::Right));
        state.handle_key(key(KeyCode::Down));

        assert_eq!(state.handle_key(key(KeyCode::Enter)), TabbedFormEvent::None);
    }
}
