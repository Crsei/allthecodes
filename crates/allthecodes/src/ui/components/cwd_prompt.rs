//! Working-directory trust prompt model.

use std::path::{Path, PathBuf};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CwdPromptAction {
    Trust,
    ContinueOnce,
    Exit,
}

impl CwdPromptAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trust => "Trust this workspace",
            Self::ContinueOnce => "Continue once",
            Self::Exit => "Exit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdSelection {
    Current(PathBuf),
    Other(PathBuf),
}

impl CwdSelection {
    pub fn path(&self) -> &Path {
        match self {
            Self::Current(path) | Self::Other(path) => path,
        }
    }
    pub fn display_name(&self) -> String {
        self.path().display().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdPromptOutcome {
    Trusted(PathBuf),
    ContinueOnce(PathBuf),
    Exit,
}

pub fn resolve_cwd_prompt_outcome(
    selection: CwdSelection,
    action: CwdPromptAction,
) -> CwdPromptOutcome {
    match action {
        CwdPromptAction::Trust => CwdPromptOutcome::Trusted(selection.path().to_path_buf()),
        CwdPromptAction::ContinueOnce => {
            CwdPromptOutcome::ContinueOnce(selection.path().to_path_buf())
        }
        CwdPromptAction::Exit => CwdPromptOutcome::Exit,
    }
}
pub fn cwd_prompt_lines(selection: &CwdSelection) -> Vec<String> {
    BetterViewPanel::new("Workspace trust required")
        .summary(format!(
            "path={} status=untrusted",
            selection.display_name()
        ))
        .sections_title("Decisions")
        .sections(
            vec![
                "Trust this workspace".to_string(),
                "Continue once".to_string(),
                "Exit".to_string(),
            ],
            0,
        )
        .detail_title("Effect")
        .detail_lines(vec![
            selected_row("Trust this workspace", "trust", true),
            selected_row("Continue once", "no persistence", false),
            selected_row("Exit", "close session", false),
            String::new(),
            plain_row("Effect", "Trust allows project instructions and tools"),
        ])
        .footer("Up/Down decision | Enter confirm | Esc reject")
        .render_lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_trust_action_to_path() {
        let outcome = resolve_cwd_prompt_outcome(
            CwdSelection::Current(PathBuf::from("repo")),
            CwdPromptAction::Trust,
        );
        assert_eq!(outcome, CwdPromptOutcome::Trusted(PathBuf::from("repo")));
    }

    #[test]
    fn alternate_actions_and_other_selection_render() {
        let selection = CwdSelection::Other(PathBuf::from("/tmp/other"));
        assert_eq!(selection.display_name(), "/tmp/other");
        assert_eq!(CwdPromptAction::ContinueOnce.label(), "Continue once");
        assert_eq!(CwdPromptAction::Exit.label(), "Exit");
        assert_eq!(
            resolve_cwd_prompt_outcome(selection.clone(), CwdPromptAction::ContinueOnce),
            CwdPromptOutcome::ContinueOnce(PathBuf::from("/tmp/other"))
        );
        assert_eq!(
            resolve_cwd_prompt_outcome(selection.clone(), CwdPromptAction::Exit),
            CwdPromptOutcome::Exit
        );
        assert!(cwd_prompt_lines(&selection)
            .join("\n")
            .contains("Workspace trust required"));
    }
}
