//! LSP recommendation menu rendering and install state tracking.
//!
//! Provides the [`LspRecommendationPromptState`] used by the TUI surface to
//! present a single-recommendation prompt. When the user selects "yes", the
//! prompt state records the install attempt and its outcome through the
//! [`InstallState`] enum.
//!
//! The actual installation is handled by the backend through the existing IPC
//! flow; the UI layer tracks status locally for immediate feedback.

use std::collections::HashMap;

/// Tracks the installation state of a recommended plugin within the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallState {
    /// User has not yet acted on this recommendation.
    Pending,
    /// Installation is in progress (stub — will be wired to backend).
    #[cfg(test)]
    Installing,
    /// Installation completed successfully.
    #[cfg(test)]
    Installed,
    /// Installation failed with an error message.
    #[cfg(test)]
    Failed { error: String },
}

impl InstallState {
    #[cfg(test)]
    pub fn is_terminal(&self) -> bool {
        matches!(self, InstallState::Installed | InstallState::Failed { .. })
    }

    #[cfg(test)]
    pub fn label(&self) -> &str {
        match self {
            InstallState::Pending => "pending",
            InstallState::Installing => "installing",
            InstallState::Installed => "installed",
            InstallState::Failed { .. } => "failed",
        }
    }
}

/// A simplified recommendation struct used for UI rendering.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRecommendation {
    pub language: String,
    pub server: String,
    pub install_command: Option<String>,
    pub reason: String,
    pub selected: bool,
}

#[cfg(test)]
impl LspRecommendation {
    pub fn new(language: impl Into<String>, server: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            server: server.into(),
            install_command: None,
            reason: String::new(),
            selected: false,
        }
    }
}

#[cfg(test)]
pub fn render_lsp_recommendation_menu(recommendations: &[LspRecommendation]) -> String {
    if recommendations.is_empty() {
        return "No language server recommendations".to_string();
    }

    let mut lines = vec!["Language servers".to_string()];
    for recommendation in recommendations {
        let marker = if recommendation.selected { ">" } else { " " };
        lines.push(format!(
            "{marker} {:<12} {}",
            recommendation.language, recommendation.server
        ));
        if !recommendation.reason.is_empty() {
            lines.push(format!("  reason: {}", recommendation.reason));
        }
        if let Some(command) = &recommendation.install_command {
            lines.push(format!("  install: {command}"));
        }
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspRecommendationDecision {
    Yes,
    No,
    Never,
    Disable,
}

impl LspRecommendationDecision {
    pub fn value(self) -> &'static str {
        match self {
            LspRecommendationDecision::Yes => "yes",
            LspRecommendationDecision::No => "no",
            LspRecommendationDecision::Never => "never",
            LspRecommendationDecision::Disable => "disable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRecommendationPromptState {
    pub plugin_name: String,
    pub plugin_description: Option<String>,
    pub file_extension: String,
    pub selected_index: usize,
    /// Track installation status for the current plugin.
    pub install_state: InstallState,
    /// Track installation status for previously recommended plugins by ID.
    pub installation_status: HashMap<String, InstallState>,
}

impl LspRecommendationPromptState {
    pub fn new(
        plugin_name: impl Into<String>,
        plugin_description: Option<String>,
        file_extension: impl Into<String>,
    ) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            plugin_description,
            file_extension: file_extension.into(),
            selected_index: 0,
            install_state: InstallState::Pending,
            installation_status: HashMap::new(),
        }
    }

    pub fn move_next(&mut self) {
        self.selected_index = (self.selected_index + 1) % LSP_RECOMMENDATION_CHOICES.len();
    }

    pub fn move_prev(&mut self) {
        self.selected_index = if self.selected_index == 0 {
            LSP_RECOMMENDATION_CHOICES.len() - 1
        } else {
            self.selected_index - 1
        };
    }

    pub fn selected_decision(&self) -> LspRecommendationDecision {
        LSP_RECOMMENDATION_CHOICES[self.selected_index].decision
    }

    /// Mark the current plugin as being installed.
    #[cfg(test)]
    pub fn mark_installing(&mut self) {
        self.install_state = InstallState::Installing;
        self.installation_status
            .insert(self.plugin_name.clone(), InstallState::Installing);
    }

    /// Mark the current plugin as successfully installed.
    #[cfg(test)]
    pub fn mark_installed(&mut self) {
        self.install_state = InstallState::Installed;
        self.installation_status
            .insert(self.plugin_name.clone(), InstallState::Installed);
    }

    /// Mark the current plugin as failed with an error message.
    #[cfg(test)]
    pub fn mark_failed(&mut self, error: String) {
        let state = InstallState::Failed { error };
        self.install_state = state.clone();
        self.installation_status
            .insert(self.plugin_name.clone(), state);
    }

    /// Reset the prompt state to allow re-selection after a failure.
    #[cfg(test)]
    pub fn reset_for_retry(&mut self) {
        self.install_state = InstallState::Pending;
        self.selected_index = 0;
    }

    /// Check the current install state for the current plugin.
    #[cfg(test)]
    pub fn install_state_for(&self, plugin_name: &str) -> Option<&InstallState> {
        self.installation_status.get(plugin_name)
    }

    #[cfg(test)]
    pub fn render(&self) -> String {
        let mut lines = vec![
            "LSP Plugin Recommendation".to_string(),
            "LSP provides code intelligence like go-to-definition and error checking".to_string(),
            format!("Plugin: {}", self.plugin_name),
        ];
        if let Some(description) = &self.plugin_description {
            lines.push(description.clone());
        }
        lines.push(format!("Triggered by: {} files", self.file_extension));

        // Show install status if applicable
        match &self.install_state {
            InstallState::Installing => {
                lines.push("Status: Installing...".to_string());
            }
            InstallState::Installed => {
                lines.push("Status: Installed successfully.".to_string());
            }
            InstallState::Failed { error } => {
                lines.push(format!("Status: Failed — {error}"));
                lines.push("Press Enter to retry or Esc to cancel.".to_string());
            }
            InstallState::Pending => {
                lines.push("Would you like to install this LSP plugin?".to_string());
            }
        }

        // Only show choices when in Pending state
        if self.install_state == InstallState::Pending {
            for (idx, choice) in LSP_RECOMMENDATION_CHOICES.iter().enumerate() {
                let marker = if idx == self.selected_index { ">" } else { " " };
                lines.push(format!("{marker} {}", choice.label(&self.plugin_name)));
            }
            lines.push("Enter choose | Esc no".to_string());
        } else if self.install_state.is_terminal() {
            lines.push("Press Enter to close.".to_string());
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Copy)]
struct LspRecommendationChoice {
    decision: LspRecommendationDecision,
    #[cfg(test)]
    label: &'static str,
}

impl LspRecommendationChoice {
    #[cfg(test)]
    fn label(self, plugin_name: &str) -> String {
        match self.decision {
            LspRecommendationDecision::Yes => format!("Yes, install {plugin_name}"),
            LspRecommendationDecision::Never => format!("Never for {plugin_name}"),
            _ => self.label.to_string(),
        }
    }
}

const LSP_RECOMMENDATION_CHOICES: &[LspRecommendationChoice] = &[
    LspRecommendationChoice {
        decision: LspRecommendationDecision::Yes,
        #[cfg(test)]
        label: "Yes",
    },
    LspRecommendationChoice {
        decision: LspRecommendationDecision::No,
        #[cfg(test)]
        label: "No, not now",
    },
    LspRecommendationChoice {
        decision: LspRecommendationDecision::Never,
        #[cfg(test)]
        label: "Never",
    },
    LspRecommendationChoice {
        decision: LspRecommendationDecision::Disable,
        #[cfg(test)]
        label: "Disable all LSP recommendations",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_state_pending() {
        let state = InstallState::Pending;
        assert!(!state.is_terminal());
        assert_eq!(state.label(), "pending");
    }

    #[test]
    fn test_install_state_installing() {
        let state = InstallState::Installing;
        assert!(!state.is_terminal());
        assert_eq!(state.label(), "installing");
    }

    #[test]
    fn test_install_state_installed() {
        let state = InstallState::Installed;
        assert!(state.is_terminal());
        assert_eq!(state.label(), "installed");
    }

    #[test]
    fn test_install_state_failed() {
        let state = InstallState::Failed {
            error: "timeout".to_string(),
        };
        assert!(state.is_terminal());
        assert_eq!(state.label(), "failed");
    }

    #[test]
    fn test_install_state_equality() {
        assert_eq!(InstallState::Pending, InstallState::Pending);
        assert_eq!(
            InstallState::Failed {
                error: "err".to_string()
            },
            InstallState::Failed {
                error: "err".to_string()
            }
        );
        assert_ne!(InstallState::Pending, InstallState::Installing);
    }

    #[test]
    fn test_prompt_state_initial() {
        let state = LspRecommendationPromptState::new("rust-analyzer", None, ".rs");
        assert_eq!(state.plugin_name, "rust-analyzer");
        assert_eq!(state.install_state, InstallState::Pending);
        assert!(state.installation_status.is_empty());
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_prompt_state_mark_installing() {
        let mut state = LspRecommendationPromptState::new("python-lsp", None, ".py");
        state.mark_installing();
        assert_eq!(state.install_state, InstallState::Installing);
        assert_eq!(
            state.install_state_for("python-lsp"),
            Some(&InstallState::Installing)
        );
    }

    #[test]
    fn test_prompt_state_mark_installed() {
        let mut state = LspRecommendationPromptState::new("gopls", None, ".go");
        state.mark_installed();
        assert_eq!(state.install_state, InstallState::Installed);
    }

    #[test]
    fn test_prompt_state_mark_failed() {
        let mut state = LspRecommendationPromptState::new("clangd", None, ".cpp");
        state.mark_failed("binary not found".to_string());
        assert!(matches!(state.install_state, InstallState::Failed { .. }));
    }

    #[test]
    fn test_prompt_state_reset_for_retry() {
        let mut state = LspRecommendationPromptState::new("java-lsp", None, ".java");
        state.mark_failed("network error".to_string());
        state.reset_for_retry();
        assert_eq!(state.install_state, InstallState::Pending);
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_prompt_state_move_next_prev() {
        let mut state = LspRecommendationPromptState::new("test", None, ".ts");
        assert_eq!(state.selected_index, 0);
        state.move_next();
        assert_eq!(state.selected_index, 1);
        state.move_prev();
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn test_decision_values() {
        assert_eq!(LspRecommendationDecision::Yes.value(), "yes");
        assert_eq!(LspRecommendationDecision::No.value(), "no");
        assert_eq!(LspRecommendationDecision::Never.value(), "never");
        assert_eq!(LspRecommendationDecision::Disable.value(), "disable");
    }

    #[test]
    fn test_render_with_install_state_pending() {
        let state = LspRecommendationPromptState::new("rust-analyzer", None, ".rs");
        let rendered = state.render();
        assert!(rendered.contains("Would you like to install"));
        assert!(rendered.contains("Yes, install rust-analyzer"));
    }

    #[test]
    fn test_render_with_install_state_installed() {
        let mut state = LspRecommendationPromptState::new("rust-analyzer", None, ".rs");
        state.mark_installed();
        let rendered = state.render();
        assert!(rendered.contains("Installed successfully"));
    }

    #[test]
    fn test_render_with_install_state_failed() {
        let mut state = LspRecommendationPromptState::new("rust-analyzer", None, ".rs");
        state.mark_failed("connection refused".to_string());
        let rendered = state.render();
        assert!(rendered.contains("Failed"));
        assert!(rendered.contains("connection refused"));
    }

    #[test]
    fn test_render_with_install_state_installing() {
        let mut state = LspRecommendationPromptState::new("rust-analyzer", None, ".rs");
        state.mark_installing();
        let rendered = state.render();
        assert!(rendered.contains("Installing"));
    }

    #[test]
    fn test_lsp_recommendation_new() {
        let rec = LspRecommendation::new("Rust", "rust-analyzer");
        assert_eq!(rec.language, "Rust");
        assert_eq!(rec.server, "rust-analyzer");
        assert!(!rec.selected);
    }

    #[test]
    fn test_render_lsp_recommendation_menu_empty() {
        let rendered = render_lsp_recommendation_menu(&[]);
        assert_eq!(rendered, "No language server recommendations");
    }

    #[test]
    fn test_render_lsp_recommendation_menu_with_items() {
        let mut rec = LspRecommendation::new("Rust", "rust-analyzer");
        rec.selected = true;
        rec.reason = "Cargo.toml detected".to_string();
        let rendered = render_lsp_recommendation_menu(&[rec]);
        assert!(rendered.contains("Language servers"));
        assert!(rendered.contains("rust-analyzer"));
        assert!(rendered.contains(">"));
    }
}
