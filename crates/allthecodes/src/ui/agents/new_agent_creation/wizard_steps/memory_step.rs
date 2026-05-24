//! Wizard step for selecting agent memory scope.

use super::render_step_frame;
use crate::ui::agents::types::AgentMemoryScope;
use crate::ui::agents::utils::selection_marker;

pub fn render_memory_step(selected: Option<AgentMemoryScope>) -> String {
    let options = [
        (AgentMemoryScope::None, "No memory"),
        (AgentMemoryScope::Project, "Project memory"),
        (AgentMemoryScope::User, "User memory"),
        (AgentMemoryScope::Local, "Local memory"),
    ];
    let body = options
        .into_iter()
        .map(|(scope, label)| format!("{} {label}", selection_marker(Some(scope) == selected)))
        .collect::<Vec<_>>()
        .join("\n");
    render_step_frame("Memory", body, selected.is_some())
}
