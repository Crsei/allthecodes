//! Create-agent wizard sequencing.

use super::AgentWizardData;

pub const BASE_AGENT_WIZARD_STEPS: &[&str] = &[
    "Location",
    "Method",
    "Generate",
    "Type",
    "Prompt",
    "Description",
    "Tools",
    "Model",
    "Color",
    "Confirm",
];

pub fn wizard_steps(auto_memory_enabled: bool) -> Vec<&'static str> {
    let mut steps = BASE_AGENT_WIZARD_STEPS.to_vec();
    if auto_memory_enabled {
        steps.insert(9, "Memory");
    }
    steps
}

pub fn render_create_agent_wizard(
    data: &AgentWizardData,
    current_step: usize,
    auto_memory_enabled: bool,
) -> String {
    let steps = wizard_steps(auto_memory_enabled);
    let current = steps.get(current_step).copied().unwrap_or("Confirm");
    let mut lines = vec![
        "Create new agent".to_string(),
        format!("step: {}/{} {current}", current_step + 1, steps.len()),
        format!("ready: {}", data.is_ready_to_confirm()),
    ];
    for (idx, step) in steps.iter().enumerate() {
        let marker = if idx == current_step { ">" } else { " " };
        lines.push(format!("{marker} {step}"));
    }
    lines.join("\n")
}
