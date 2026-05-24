//! Agent editor menu and change summary helpers.

use super::types::AgentDefinition;
use super::utils::selection_marker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEditMode {
    Menu,
    EditTools,
    EditColor,
    EditModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentSaveChanges {
    pub tools: Option<Vec<String>>,
    pub color: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEditorState {
    pub agent: AgentDefinition,
    pub mode: AgentEditMode,
    pub selected_menu_index: usize,
    pub error: Option<String>,
}

impl AgentEditorState {
    pub fn new(agent: AgentDefinition) -> Self {
        Self {
            agent,
            mode: AgentEditMode::Menu,
            selected_menu_index: 0,
            error: None,
        }
    }

    pub fn render_menu(&self) -> String {
        let menu_items = ["Open in editor", "Edit tools", "Edit model", "Edit color"];
        let mut lines = vec![format!("Source: {}", self.agent.source.display_name())];
        for (idx, item) in menu_items.iter().enumerate() {
            lines.push(format!(
                "{} {}",
                selection_marker(idx == self.selected_menu_index),
                item
            ));
        }
        if let Some(error) = &self.error {
            lines.push(format!("error: {error}"));
        }
        lines.join("\n")
    }
}

pub fn render_save_change_summary(agent: &AgentDefinition, changes: &AgentSaveChanges) -> String {
    let mut rows = vec![format!("agent: {}", agent.agent_type)];
    if let Some(tools) = &changes.tools {
        rows.push(format!("tools: {}", tools.join(", ")));
    }
    if let Some(model) = &changes.model {
        rows.push(format!("model: {model}"));
    }
    if let Some(color) = &changes.color {
        rows.push(format!("color: {color}"));
    }
    if rows.len() == 1 {
        rows.push("changes: none".to_string());
    }
    rows.join("\n")
}
