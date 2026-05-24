//! Wizard step for tool selection.

use super::render_step_frame;
use crate::ui::agents::tool_selector::{default_agent_tools, ToolSelectorState};

pub fn render_tools_step(
    selected_tools: Option<Vec<String>>,
    show_individual_tools: bool,
) -> String {
    let mut state = ToolSelectorState::new(default_agent_tools(), selected_tools);
    state.show_individual_tools = show_individual_tools;
    render_step_frame("Tools", state.render(), true)
}
