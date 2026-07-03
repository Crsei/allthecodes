use crate::ui::command_surface::CommandSurface;
use crate::ui::history_search_dialog::HistorySearchDialog;
use crate::ui::permissions::{BypassPermissionsModeDialog, PermissionDialog, QuestionDialog};

use super::agent_tree_dialog::AgentTreeDialog;
use super::AppAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveOverlay {
    BypassPermissions,
    Question,
    Permission,
    AgentTree,
    HistorySearch,
    CommandSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OverlayOutcome {
    Inactive,
    Handled(AppAction),
}

impl OverlayOutcome {
    pub(super) fn into_app_action(self) -> Option<AppAction> {
        match self {
            Self::Inactive => None,
            Self::Handled(action) => Some(action),
        }
    }
}

#[derive(Default)]
pub(super) struct OverlayState {
    pub(super) bypass_permissions_mode_dialog: Option<BypassPermissionsModeDialog>,
    pub(super) permission_dialog: Option<PermissionDialog>,
    pub(super) question_dialog: Option<QuestionDialog>,
    pub(super) agent_tree_dialog: Option<AgentTreeDialog>,
    pub(super) history_search_dialog: Option<HistorySearchDialog>,
    pub(super) command_surface: Option<CommandSurface>,
}

impl OverlayState {
    pub(super) fn active_overlay(&self) -> Option<ActiveOverlay> {
        if self.bypass_permissions_mode_dialog.is_some() {
            return Some(ActiveOverlay::BypassPermissions);
        }
        if self.question_dialog.is_some() {
            return Some(ActiveOverlay::Question);
        }
        if self.permission_dialog.is_some() {
            return Some(ActiveOverlay::Permission);
        }
        if self.agent_tree_dialog.is_some() {
            return Some(ActiveOverlay::AgentTree);
        }
        if self.history_search_dialog.is_some() {
            return Some(ActiveOverlay::HistorySearch);
        }
        if self.command_surface.is_some() {
            return Some(ActiveOverlay::CommandSurface);
        }
        None
    }

    pub(super) fn clear_permission(&mut self) {
        self.permission_dialog = None;
    }

    pub(super) fn clear_question(&mut self) {
        self.question_dialog = None;
    }
}

#[cfg(test)]
impl OverlayState {
    fn set_command_surface_for_test(&mut self) {
        self.command_surface = Some(CommandSurface::Tasks(
            crate::ui::command_surface::TasksSurface::new(),
        ));
    }

    fn set_permission_for_test(&mut self) {
        self.permission_dialog = Some(PermissionDialog::new("Read", "{}", "Inspect file"));
    }

    fn set_question_for_test(&mut self) {
        self.question_dialog = Some(QuestionDialog::new(
            "question-id",
            allthecodes_types::callbacks::AskUserRequestPayload {
                question: "Continue?".to_string(),
                choices: Vec::new(),
                allow_free_text: true,
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_priority_prefers_question_permission_before_command_surface() {
        let mut overlays = OverlayState::default();
        overlays.set_command_surface_for_test();
        overlays.set_permission_for_test();
        overlays.set_question_for_test();

        assert_eq!(overlays.active_overlay(), Some(ActiveOverlay::Question));

        overlays.clear_question();
        assert_eq!(overlays.active_overlay(), Some(ActiveOverlay::Permission));

        overlays.clear_permission();
        assert_eq!(
            overlays.active_overlay(),
            Some(ActiveOverlay::CommandSurface)
        );
    }
}
