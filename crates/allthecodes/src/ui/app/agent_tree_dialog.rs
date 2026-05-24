use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::ui::theme::Theme;

use super::agent_navigation::{short_thread_id, AgentNavigationDirection, AgentNavigationState};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentTreeDialog {
    selected_thread_id: Option<String>,
}

impl AgentTreeDialog {
    pub fn from_state(state: &AgentNavigationState, current_thread_id: &str) -> Self {
        let mut dialog = Self::default();
        dialog.sync_selection(state, current_thread_id);
        dialog
    }

    pub fn move_next(&mut self, state: &AgentNavigationState, current_thread_id: &str) {
        self.sync_selection(state, current_thread_id);
        if let Some(current) = self.selected_thread_id.as_deref() {
            if let Some(next) = state.adjacent_thread_id(current, AgentNavigationDirection::Next) {
                self.selected_thread_id = Some(next);
            }
        }
    }

    pub fn move_prev(&mut self, state: &AgentNavigationState, current_thread_id: &str) {
        self.sync_selection(state, current_thread_id);
        if let Some(current) = self.selected_thread_id.as_deref() {
            if let Some(previous) =
                state.adjacent_thread_id(current, AgentNavigationDirection::Previous)
            {
                self.selected_thread_id = Some(previous);
            }
        }
    }

    pub fn selected_thread_id(
        &mut self,
        state: &AgentNavigationState,
        current_thread_id: &str,
    ) -> Option<String> {
        self.sync_selection(state, current_thread_id);
        self.selected_thread_id.clone()
    }

    pub fn render_lines(
        &mut self,
        state: &AgentNavigationState,
        current_thread_id: &str,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
        self.sync_selection(state, current_thread_id);

        let ordered = state.ordered_threads();
        if ordered.is_empty() {
            return vec![Line::from(Span::styled("No active agents", theme.dim))];
        }

        let mut lines = Vec::with_capacity(ordered.len() + 3);
        lines.push(Line::from(Span::styled(
            format!("Threads: {}", ordered.len()),
            theme.dim,
        )));
        lines.push(Line::default());

        for entry in ordered {
            let is_selected = self.selected_thread_id.as_deref() == Some(entry.thread_id.as_str());
            let is_current = entry.thread_id == current_thread_id;
            let marker = if is_selected { ">" } else { " " };
            let status = state
                .runtime_info(&entry.thread_id)
                .map(|runtime| runtime.status.label())
                .unwrap_or(if entry.is_closed { "closed" } else { "active" });
            let role = entry.agent_role.as_deref().unwrap_or("agent");

            let mut label_style = if entry.is_closed {
                theme.warning
            } else {
                theme.bold
            };
            if is_current {
                label_style = label_style.add_modifier(Modifier::BOLD);
            }

            let line = Line::from(vec![
                Span::styled(
                    format!("{marker} "),
                    if is_selected { theme.info } else { theme.dim },
                ),
                Span::styled(entry.label(), label_style),
                Span::styled(format!("  [{status}]"), theme.dim),
                Span::styled(format!("  role:{role}"), theme.dim),
                Span::styled(
                    format!("  id:{}", short_thread_id(&entry.thread_id)),
                    theme.dim,
                ),
            ]);
            lines.push(line);
        }

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Up/Down navigate | Enter inspect output | Esc close",
            theme.dim,
        )));
        lines
    }

    fn sync_selection(&mut self, state: &AgentNavigationState, current_thread_id: &str) {
        if state.thread_count() == 0 {
            self.selected_thread_id = None;
            return;
        }
        if self
            .selected_thread_id
            .as_deref()
            .is_some_and(|id| state.contains_thread(id))
        {
            return;
        }
        if state.contains_thread(current_thread_id) {
            self.selected_thread_id = Some(current_thread_id.to_string());
            return;
        }
        self.selected_thread_id = state
            .ordered_threads()
            .first()
            .map(|entry| entry.thread_id.clone());
    }
}
