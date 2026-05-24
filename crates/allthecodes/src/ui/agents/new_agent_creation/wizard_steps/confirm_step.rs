//! Wizard confirmation summary.

use super::render_step_frame;
use crate::ui::agents::agent_file_utils::format_agent_as_markdown;
use crate::ui::agents::types::AgentDefinition;

pub fn render_confirm_step(agent: &AgentDefinition) -> String {
    let body = format!(
        "name: {}\nsource: {}\nmarkdown:\n{}",
        agent.agent_type,
        agent.source.display_name(),
        format_agent_as_markdown(agent)
    );
    render_step_frame("Confirm", body, true)
}
