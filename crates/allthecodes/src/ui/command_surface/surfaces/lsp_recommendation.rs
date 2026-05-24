use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::lsp_recommendation::lsp_recommendation_menu::{
    LspRecommendationDecision, LspRecommendationPromptState,
};
use allthecodes_ipc_protocol::subsystem_types::LspRecommendationPayload;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRecommendationSurface {
    pub(crate) request_id: String,
    pub(crate) state: LspRecommendationPromptState,
}

impl LspRecommendationSurface {
    pub(crate) fn new(payload: LspRecommendationPayload) -> Self {
        Self {
            request_id: payload.request_id,
            state: LspRecommendationPromptState::new(
                payload.plugin_name,
                payload.plugin_description,
                payload.file_extension,
            ),
        }
    }
    pub(crate) fn render(&self) -> String {
        let choices = [
            (
                LspRecommendationDecision::Yes,
                format!("Yes, install {}", self.state.plugin_name),
            ),
            (LspRecommendationDecision::No, "No, not now".to_string()),
            (
                LspRecommendationDecision::Never,
                format!("Never for {}", self.state.plugin_name),
            ),
            (
                LspRecommendationDecision::Disable,
                "Disable recommendations".to_string(),
            ),
        ];
        let mut detail_lines = choices
            .iter()
            .enumerate()
            .map(|(idx, (_, label))| selected_row(label, "", idx == self.state.selected_index))
            .collect::<Vec<_>>();
        detail_lines.push(String::new());
        detail_lines.push("Plugin".to_string());
        detail_lines.push(plain_row("name:", &self.state.plugin_name));
        detail_lines.push(plain_row(
            "install prompt:",
            format!("/plugin install {} ", self.state.plugin_name),
        ));
        if let Some(description) = &self.state.plugin_description {
            detail_lines.push(plain_row("description:", description));
        }
        BetterViewPanel::new("LSP plugin recommendation")
            .summary(format!(
                "language={} reason={} files detected",
                self.state.file_extension, self.state.file_extension
            ))
            .sections_title("Choices")
            .sections(
                choices
                    .iter()
                    .map(|(_, label)| label.clone())
                    .collect::<Vec<_>>(),
                self.state.selected_index,
            )
            .detail_title("Plugin")
            .detail_lines(detail_lines)
            .footer("Up/Down choice | Enter submit | Esc no")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.state.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.response(self.state.selected_decision()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn cancel(&self) -> CommandSurfaceOutcome {
        self.response(LspRecommendationDecision::No)
    }

    pub(crate) fn response(&self, decision: LspRecommendationDecision) -> CommandSurfaceOutcome {
        let install_prompt = (decision == LspRecommendationDecision::Yes)
            .then(|| format!("/plugin install {} ", self.state.plugin_name));
        CommandSurfaceOutcome::LspRecommendationResponse {
            request_id: self.request_id.clone(),
            plugin_name: self.state.plugin_name.clone(),
            decision: decision.value().to_string(),
            install_prompt,
        }
    }
}
