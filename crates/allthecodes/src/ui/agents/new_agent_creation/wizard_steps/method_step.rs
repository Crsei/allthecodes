//! Wizard step for selecting manual or generated creation.

use super::render_step_frame;
use crate::ui::agents::new_agent_creation::AgentCreationMethod;
use crate::ui::agents::utils::selection_marker;

pub fn render_method_step(selected: Option<AgentCreationMethod>) -> String {
    let options = [
        (AgentCreationMethod::Generate, "Generate with a goal"),
        (AgentCreationMethod::Manual, "Enter fields manually"),
    ];
    let body = options
        .into_iter()
        .map(|(method, label)| format!("{} {label}", selection_marker(Some(method) == selected)))
        .collect::<Vec<_>>()
        .join("\n");
    render_step_frame("Method", body, selected.is_some())
}
