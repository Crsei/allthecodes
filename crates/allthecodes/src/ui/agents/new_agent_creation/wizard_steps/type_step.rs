//! Wizard step for choosing the agent type/name.

use super::render_step_frame;
use crate::ui::agents::validate_agent::{render_validation_result, validate_agent_definition};

pub fn render_type_step(
    agent_type: Option<&str>,
    existing_agent_types: &[String],
    when_to_use: &str,
    system_prompt: &str,
) -> String {
    let value = agent_type.unwrap_or("");
    let validation =
        validate_agent_definition(value, when_to_use, system_prompt, existing_agent_types);
    let body = format!(
        "name: {}\n{}",
        if value.is_empty() { "<empty>" } else { value },
        render_validation_result(&validation)
    );
    render_step_frame(
        "Type",
        body,
        !value.trim().is_empty() && validation.errors.is_empty(),
    )
}
