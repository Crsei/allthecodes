//! Wizard step for choosing where to save an agent.

use crate::ui::agents::types::AgentSource;

use super::render_step_frame;
use crate::ui::agents::utils::selection_marker;

pub fn render_location_step(selected: Option<AgentSource>) -> String {
    let options = [
        (AgentSource::User, "Available across projects"),
        (AgentSource::Project, "Stored in this workspace"),
        (AgentSource::Local, "Workspace-local override"),
    ];
    let mut lines = Vec::new();
    for (source, description) in options {
        lines.push(format!(
            "{} {:<8} {}",
            selection_marker(Some(source) == selected),
            source.display_name(),
            description
        ));
    }
    render_step_frame("Location", lines.join("\n"), selected.is_some())
}
