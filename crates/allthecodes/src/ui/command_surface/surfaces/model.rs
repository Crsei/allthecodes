use crossterm::event::KeyEvent;

use crate::ui::better_view_panel::BetterViewPanel;
use crate::ui::command_surface::{CommandSurfaceOutcome, CommandSurfaceTarget};
use crate::ui::selection_surface::{SelectionSurface, SelectionSurfaceEvent};
use allthecodes_engine::types::app_state::AppState;

use super::config::build_model_picker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSurface {
    picker: SelectionSurface,
}

impl ModelSurface {
    pub(crate) fn new(state: &AppState) -> Self {
        Self {
            picker: build_model_picker(state),
        }
    }

    pub(crate) fn render(&self) -> String {
        BetterViewPanel::new("Model")
            .summary("Select the active model")
            .detail_title("Available models")
            .detail_lines(self.picker.render_lines(12))
            .footer("Type filter | Up/Down navigate | Enter select | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match self.picker.handle_key(key) {
            SelectionSurfaceEvent::Selected(id) => CommandSurfaceOutcome::SubmitThenOpen {
                command: format!("/model {id}"),
                next_surface: CommandSurfaceTarget::Effort,
            },
            SelectionSurfaceEvent::Closed => CommandSurfaceOutcome::Close,
            SelectionSurfaceEvent::None => CommandSurfaceOutcome::None,
        }
    }
}
