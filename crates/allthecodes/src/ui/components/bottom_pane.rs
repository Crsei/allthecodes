//! Bottom pane container and view stack.

use ratatui::layout::{Constraint, Layout, Rect};

#[cfg(test)]
use super::approval_overlay::ApprovalOverlay;
#[cfg(test)]
use super::chat_composer::ChatComposerState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BottomPaneHeights {
    pub expanded_view: u16,
    pub spinner: u16,
    pub suggestions: u16,
    pub input: u16,
    pub completion_popup: u16,
    pub command_palette: u16,
    pub command_arg_help: u16,
    pub notification: u16,
    pub context: u16,
    pub agent_footer: u16,
    pub status: u16,
}

impl BottomPaneHeights {
    pub fn total(self) -> u16 {
        self.expanded_view
            + self.spinner
            + self.suggestions
            + self.input
            + self.completion_popup
            + self.command_palette
            + self.command_arg_help
            + self.notification
            + self.context
            + self.agent_footer
            + self.status
    }

    pub fn split(self, area: Rect) -> BottomPaneAreas {
        let chunks = Layout::vertical([
            Constraint::Length(self.expanded_view),
            Constraint::Length(self.spinner),
            Constraint::Length(self.suggestions),
            Constraint::Length(self.completion_popup),
            Constraint::Length(self.command_palette),
            Constraint::Length(self.command_arg_help),
            Constraint::Length(self.input),
            Constraint::Length(self.notification),
            Constraint::Length(self.context),
            Constraint::Length(self.agent_footer),
            Constraint::Length(self.status),
        ])
        .split(area);

        BottomPaneAreas {
            expanded_view: chunks[0],
            spinner: chunks[1],
            suggestions: chunks[2],
            completion_popup: chunks[3],
            command_palette: chunks[4],
            command_arg_help: chunks[5],
            input: chunks[6],
            notification: chunks[7],
            context: chunks[8],
            agent_footer: chunks[9],
            status: chunks[10],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BottomPaneAreas {
    pub expanded_view: Rect,
    pub spinner: Rect,
    pub suggestions: Rect,
    pub input: Rect,
    pub completion_popup: Rect,
    pub command_palette: Rect,
    pub command_arg_help: Rect,
    pub notification: Rect,
    pub context: Rect,
    pub agent_footer: Rect,
    pub status: Rect,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BottomPaneView {
    Composer,
    Approval(ApprovalOverlay),
    Selection { title: String, count: usize },
    Status { message: String },
}

#[cfg(test)]
impl BottomPaneView {
    pub fn name(&self) -> &'static str {
        match self {
            BottomPaneView::Composer => "composer",
            BottomPaneView::Approval(_) => "approval",
            BottomPaneView::Selection { .. } => "selection",
            BottomPaneView::Status { .. } => "status",
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BottomPane {
    pub composer: ChatComposerState,
    stack: Vec<BottomPaneView>,
}

#[cfg(test)]
impl BottomPane {
    pub fn new(composer: ChatComposerState) -> Self {
        Self {
            composer,
            stack: vec![BottomPaneView::Composer],
        }
    }

    pub fn push(&mut self, view: BottomPaneView) {
        self.stack.push(view);
    }
    pub fn pop(&mut self) -> Option<BottomPaneView> {
        if self.stack.len() <= 1 {
            return None;
        }
        self.stack.pop()
    }

    pub fn focused_view(&self) -> &BottomPaneView {
        self.stack.last().unwrap_or(&BottomPaneView::Composer)
    }

    pub fn render_lines(&self, width: usize) -> Vec<String> {
        let mut lines = vec![format!(
            "bottom-pane focus={} depth={}",
            self.focused_view().name(),
            self.stack.len()
        )];
        match self.focused_view() {
            BottomPaneView::Composer => lines.extend(self.composer.render_lines(width)),
            BottomPaneView::Approval(overlay) => lines.extend(overlay.render_lines(width)),
            BottomPaneView::Selection { title, count } => {
                lines.push(format!("select: {title} ({count} items)"));
            }
            BottomPaneView::Status { message } => lines.push(format!("status: {message}")),
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_supports_selection_status_and_pop() {
        let mut pane = BottomPane::new(ChatComposerState::new());
        pane.push(BottomPaneView::Selection {
            title: "Sessions".to_string(),
            count: 3,
        });
        assert_eq!(pane.focused_view().name(), "selection");
        assert!(pane.render_lines(80).join("\n").contains("Sessions"));

        pane.push(BottomPaneView::Status {
            message: "busy".to_string(),
        });
        assert_eq!(pane.focused_view().name(), "status");
        assert_eq!(pane.pop().unwrap().name(), "status");
        assert_eq!(pane.pop().unwrap().name(), "selection");
        assert!(pane.pop().is_none());
    }

    #[test]
    fn height_model_splits_terminal_regions() {
        let heights = BottomPaneHeights {
            expanded_view: 2,
            spinner: 1,
            suggestions: 1,
            input: 3,
            completion_popup: 2,
            command_palette: 4,
            command_arg_help: 2,
            notification: 1,
            context: 1,
            agent_footer: 1,
            status: 1,
        };

        assert_eq!(heights.total(), 19);
        let areas = heights.split(Rect::new(0, 0, 80, 24));
        assert_eq!(areas.expanded_view.height, 2);
        assert_eq!(areas.spinner.height, 1);
        assert_eq!(areas.input.height, 3);
        assert_eq!(areas.status.height, 1);
        assert!(areas.completion_popup.y < areas.input.y);
        assert!(areas.command_palette.y < areas.input.y);
        assert!(areas.notification.y > areas.input.y);
        assert!(areas.context.y > areas.input.y);
        assert!(areas.context.y < areas.status.y);
        assert!(areas.status.y > areas.input.y);
    }
}
