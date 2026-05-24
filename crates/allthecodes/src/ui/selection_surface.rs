//! Reusable selection, command, and picker surface.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::fuzzy_match::best_fuzzy_match;
use crate::ui::search_box::SearchBox;

#[path = "selection_surface_details.rs"]
mod selection_surface_details;
use selection_surface_details::{item_search_terms, push_item_details};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub preview_lines: Vec<String>,
    pub actions: Vec<SelectionAction>,
    pub search_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionAction {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

impl SelectionItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: String::new(),
            enabled: true,
            disabled_reason: None,
            preview_lines: Vec::new(),
            actions: Vec::new(),
            search_terms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionSurfaceEvent {
    None,
    Selected(String),
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSurface {
    pub title: String,
    pub items: Vec<SelectionItem>,
    pub selected: usize,
    pub filter: String,
}

impl SelectionSurface {
    pub fn new(title: impl Into<String>, items: Vec<SelectionItem>) -> Self {
        Self {
            title: title.into(),
            items,
            selected: 0,
            filter: String::new(),
        }
    }

    #[cfg(test)]
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.selected = 0;
    }

    pub fn move_next(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1).min(visible.len() - 1);
        }
    }

    pub fn move_prev(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.saturating_sub(1).min(visible.len() - 1);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SelectionSurfaceEvent {
        if key.kind != KeyEventKind::Press {
            return SelectionSurfaceEvent::None;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_prev();
                SelectionSurfaceEvent::None
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                self.move_next();
                SelectionSurfaceEvent::None
            }
            KeyCode::Enter => self
                .selected_item()
                .filter(|item| item.enabled)
                .map(|item| SelectionSurfaceEvent::Selected(item.id.clone()))
                .unwrap_or(SelectionSurfaceEvent::None),
            KeyCode::Esc => SelectionSurfaceEvent::Closed,
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
                SelectionSurfaceEvent::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.clear();
                self.selected = 0;
                SelectionSurfaceEvent::None
            }
            KeyCode::Char(ch) if key.modifiers.is_empty() => {
                self.filter.push(ch);
                self.selected = 0;
                SelectionSurfaceEvent::None
            }
            _ => SelectionSurfaceEvent::None,
        }
    }

    pub fn selected_item(&self) -> Option<&SelectionItem> {
        let visible = self.visible_indices();
        visible
            .get(self.selected)
            .and_then(|idx| self.items.get(*idx))
    }

    #[cfg(test)]
    pub fn selected_enabled_action(&self, action_id: &str) -> Option<&SelectionAction> {
        self.selected_item()
            .filter(|item| item.enabled)?
            .actions
            .iter()
            .find(|action| action.id == action_id && action.enabled)
    }

    pub fn render_lines(&self, height: usize) -> Vec<String> {
        let search = SearchBox::new(&self.filter)
            .placeholder("Filter...")
            .borderless(true)
            .render();
        let mut lines = vec![format!("{} {}", self.title, search)];
        let mut rendered_rows = 0usize;
        for (visible_idx, item_idx) in self.visible_indices().into_iter().enumerate() {
            if rendered_rows >= height {
                break;
            }
            let item = &self.items[item_idx];
            let marker = if visible_idx == self.selected {
                ">"
            } else {
                " "
            };
            let state = if item.enabled {
                String::new()
            } else {
                item.disabled_reason
                    .as_deref()
                    .map(|reason| format!(" disabled: {reason}"))
                    .unwrap_or_else(|| " disabled".to_string())
            };
            lines.push(format!(
                "{marker} {} - {}{state}",
                item.label, item.description
            ));
            rendered_rows += 1;
            push_item_details(&mut lines, item, &mut rendered_rows, height);
        }
        lines
    }

    fn visible_indices(&self) -> Vec<usize> {
        let mut matches = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                let matched = best_fuzzy_match(item_search_terms(item), &self.filter)?;
                Some((idx, matched.score))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(idx, score)| (*score, *idx));
        matches.into_iter().map(|(idx, _)| idx).collect()
    }
}

#[cfg(test)]
#[path = "selection_surface_preview_tests.rs"]
mod selection_surface_preview_tests;

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

    fn ctrl_key(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn picker() -> SelectionSurface {
        SelectionSurface::new(
            "Commands",
            vec![
                SelectionItem::new("status", "/status"),
                SelectionItem::new("config", "/config"),
                SelectionItem {
                    enabled: false,
                    ..SelectionItem::new("disabled", "/disabled")
                },
            ],
        )
    }

    #[test]
    fn up_down_navigate_and_enter_selects() {
        let mut surface = picker();

        surface.handle_key(key(KeyCode::Down));
        assert_eq!(
            surface.selected_item().map(|item| item.id.as_str()),
            Some("config")
        );

        assert_eq!(
            surface.handle_key(key(KeyCode::Enter)),
            SelectionSurfaceEvent::Selected("config".to_string())
        );

        surface.handle_key(key(KeyCode::Up));
        assert_eq!(
            surface.selected_item().map(|item| item.id.as_str()),
            Some("status")
        );
    }

    #[test]
    fn esc_closes_and_disabled_items_do_not_submit() {
        let mut surface = picker();
        surface.handle_key(key(KeyCode::Down));
        surface.handle_key(key(KeyCode::Down));

        assert_eq!(
            surface.handle_key(key(KeyCode::Enter)),
            SelectionSurfaceEvent::None
        );
        assert_eq!(
            surface.handle_key(key(KeyCode::Esc)),
            SelectionSurfaceEvent::Closed
        );
    }

    #[test]
    fn typed_filter_edits_query() {
        let mut surface = picker();

        surface.handle_key(key(KeyCode::Char('c')));
        surface.handle_key(key(KeyCode::Char('o')));
        assert_eq!(
            surface.selected_item().map(|item| item.id.as_str()),
            Some("config")
        );

        surface.handle_key(key(KeyCode::Backspace));
        assert_eq!(surface.filter, "c");

        surface.handle_key(ctrl_key('u'));
        assert_eq!(surface.filter, "");
    }

    #[test]
    fn fuzzy_filter_orders_exact_before_subsequence() {
        let mut surface = SelectionSurface::new(
            "Commands",
            vec![
                SelectionItem {
                    description: "model context protocol".into(),
                    ..SelectionItem::new("protocol", "Model Context Protocol")
                },
                SelectionItem::new("mcp", "mcp"),
                SelectionItem::new("memory", "Memory"),
            ],
        );

        surface.set_filter("mcp");
        let rendered = surface.render_lines(3).join("\n");

        assert!(rendered.contains("> mcp - "));
        assert!(rendered.contains("Model Context Protocol"));
    }
}
