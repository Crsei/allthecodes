//! Permission prompt text and option rendering.

use super::utils::{render_permission_options, PermissionOption};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPromptState {
    pub question: String,
    pub options: Vec<PermissionOption>,
    pub selected_index: usize,
    pub hint: String,
}

impl PermissionPromptState {
    pub fn new(
        question: impl Into<String>,
        options: Vec<PermissionOption>,
        selected_index: usize,
    ) -> Self {
        Self {
            question: question.into(),
            options,
            selected_index,
            hint: "Enter confirms, Esc denies".to_string(),
        }
    }
}

pub fn render_permission_prompt(state: &PermissionPromptState) -> String {
    let mut lines = vec![state.question.clone()];
    lines.extend(render_permission_options(
        &state.options,
        state.selected_index,
    ));
    lines.push(state.hint.clone());
    lines.join("\n")
}
