//! Agent detail rendering.

use super::agent_file_utils::get_actual_relative_agent_file_path;
use super::types::AgentDefinition;
use super::utils::{
    hooks_label, memory_label, model_label, render_kv_rows, skills_label, tools_label, wrap_text,
};

pub fn render_agent_detail(agent: &AgentDefinition, width: usize) -> String {
    let mut lines = Vec::new();
    lines.push(get_actual_relative_agent_file_path(agent));
    lines.push(String::new());
    lines.push("Description".to_string());
    lines.extend(
        wrap_text(&agent.when_to_use, width.saturating_sub(2))
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    lines.push(String::new());
    lines.push(render_kv_rows([
        ("Tools", tools_label(&agent.tools)),
        ("Model", model_label(&agent.model)),
        (
            "Permission mode",
            agent
                .permission_mode
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        ),
        ("Memory", memory_label(agent)),
        ("Hooks", hooks_label(&agent.hooks)),
        ("Skills", skills_label(&agent.skills)),
        (
            "Color",
            agent
                .color
                .clone()
                .unwrap_or_else(|| "automatic".to_string()),
        ),
    ]));

    if !agent.is_built_in() {
        lines.push(String::new());
        lines.push("System prompt".to_string());
        lines.extend(
            wrap_text(&agent.system_prompt, width.saturating_sub(2))
                .into_iter()
                .map(|line| format!("  {line}")),
        );
    }

    lines.join("\n")
}
