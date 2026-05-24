//! Builds the confirm-step agent from wizard data.

use crate::ui::agents::types::{AgentDefinition, AgentSource};

use super::confirm_step::render_confirm_step;
use crate::ui::agents::new_agent_creation::AgentWizardData;

pub fn agent_from_wizard_data(data: &AgentWizardData) -> Option<AgentDefinition> {
    let source = data.location.unwrap_or(AgentSource::Project);
    let mut agent = AgentDefinition::new(
        data.agent_type.as_ref()?.clone(),
        data.when_to_use.as_ref()?.clone(),
        data.system_prompt.as_ref()?.clone(),
        source,
    );
    agent.tools = data.tools.clone();
    agent.model = data.model.clone();
    agent.color = data.color.clone();
    agent.memory = data.memory;
    Some(agent)
}

pub fn render_confirm_step_wrapper(data: &AgentWizardData) -> String {
    match agent_from_wizard_data(data) {
        Some(agent) => render_confirm_step(&agent),
        None => "Confirm\nstatus: needs input\nmissing required agent fields".to_string(),
    }
}
