//! Typed approval and permission overlay primitives.

#[cfg(test)]
use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalKind {
    Bash { command: String },
    FileEdit { path: String },
    WebFetch { url: String },
    Mcp { server: String, action: String },
    UserInput { prompt: String },
    Fallback { summary: String },
}

impl ApprovalKind {
    pub fn title(&self) -> &'static str {
        match self {
            ApprovalKind::Bash { .. } => "Run command?",
            ApprovalKind::FileEdit { .. } => "Edit file?",
            ApprovalKind::WebFetch { .. } => "Fetch web content?",
            ApprovalKind::Mcp { .. } => "Allow MCP action?",
            ApprovalKind::UserInput { .. } => "Answer request?",
            ApprovalKind::Fallback { .. } => "Approve action?",
        }
    }

    pub fn subject(&self) -> String {
        match self {
            ApprovalKind::Bash { command } => command.clone(),
            ApprovalKind::FileEdit { path } => path.clone(),
            ApprovalKind::WebFetch { url } => url.clone(),
            ApprovalKind::Mcp { server, action } => format!("{server}: {action}"),
            ApprovalKind::UserInput { prompt } => prompt.clone(),
            ApprovalKind::Fallback { summary } => summary.clone(),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalChoice {
    AllowOnce,
    AllowAlways,
    Deny,
    EditRequest,
}

#[cfg(test)]
impl ApprovalChoice {
    pub fn label(self) -> &'static str {
        match self {
            ApprovalChoice::AllowOnce => "allow once",
            ApprovalChoice::AllowAlways => "always allow",
            ApprovalChoice::Deny => "deny",
            ApprovalChoice::EditRequest => "edit",
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalOverlay {
    pub kind: ApprovalKind,
    pub choices: Vec<ApprovalChoice>,
    pub selected: usize,
    pub fail_closed: bool,
}

#[cfg(test)]
impl ApprovalOverlay {
    pub fn new(kind: ApprovalKind) -> Self {
        Self {
            kind,
            choices: vec![ApprovalChoice::AllowOnce, ApprovalChoice::Deny],
            selected: 0,
            fail_closed: true,
        }
    }

    pub fn with_choices(mut self, choices: Vec<ApprovalChoice>) -> Self {
        self.choices = if choices.is_empty() {
            vec![ApprovalChoice::Deny]
        } else {
            choices
        };
        self.selected = self.selected.min(self.choices.len().saturating_sub(1));
        self
    }

    pub fn select(&mut self, selected: usize) {
        self.selected = selected.min(self.choices.len().saturating_sub(1));
    }
    pub fn selected_choice(&self) -> ApprovalChoice {
        self.choices
            .get(self.selected)
            .copied()
            .unwrap_or(ApprovalChoice::Deny)
    }

    pub fn render_lines(&self, width: usize) -> Vec<String> {
        let mut detail_lines = vec![
            plain_row("Request", fit_line(&self.kind.subject(), width)),
            plain_row("Default", if self.fail_closed { "deny" } else { "allow" }),
            String::new(),
            "Decisions".to_string(),
        ];
        detail_lines.extend(self.choices.iter().enumerate().map(|(idx, choice)| {
            selected_row(choice.label(), choice.label(), idx == self.selected)
        }));

        BetterViewPanel::new(self.kind.title())
            .summary(format!("risk={}", approval_risk(&self.kind)))
            .sections_title("Request")
            .sections(vec!["Request".to_string(), "Decisions".to_string()], 0)
            .detail_title("Approval")
            .detail_lines(detail_lines)
            .footer("Up/Down decision | Enter confirm | Esc deny")
            .render_lines()
    }
}

#[cfg(test)]
fn approval_risk(kind: &ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::Bash { .. } => "shell command",
        ApprovalKind::FileEdit { .. } => "file edit",
        ApprovalKind::WebFetch { .. } => "network",
        ApprovalKind::Mcp { .. } => "mcp action",
        ApprovalKind::UserInput { .. } => "tool request",
        ApprovalKind::Fallback { .. } => "action",
    }
}

#[cfg(test)]
fn fit_line(text: &str, width: usize) -> String {
    if width == 0 || text.chars().count() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut out: String = text.chars().take(width - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_request_choice_can_be_selected() {
        let mut overlay = ApprovalOverlay::new(ApprovalKind::UserInput {
            prompt: "revise command".to_string(),
        })
        .with_choices(vec![
            ApprovalChoice::AllowOnce,
            ApprovalChoice::EditRequest,
            ApprovalChoice::Deny,
        ]);
        overlay.select(1);

        assert_eq!(overlay.selected_choice(), ApprovalChoice::EditRequest);
        assert_eq!(overlay.selected_choice().label(), "edit");
        assert!(overlay
            .render_lines(40)
            .iter()
            .any(|line| line.contains("edit")));
    }
}
