//! Deterministic draft generation helpers for the create-agent flow.

use super::types::{AgentDefinition, AgentSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateAgentRequest {
    pub goal: String,
    pub source: AgentSource,
    pub preferred_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAgentDraft {
    pub agent: AgentDefinition,
    pub rationale: String,
}

pub fn generate_agent_draft(request: &GenerateAgentRequest) -> GeneratedAgentDraft {
    let normalized_goal = request.goal.trim();
    let agent_type = derive_agent_type(normalized_goal);
    let tools = if request.preferred_tools.is_empty() {
        None
    } else {
        Some(request.preferred_tools.clone())
    };
    let when_to_use = if normalized_goal.is_empty() {
        "Use when a bounded specialist should handle a clearly scoped task.".to_string()
    } else {
        format!("Use when the task is to {normalized_goal}.")
    };
    let system_prompt = format!(
        "You are the {agent_type} agent. Stay inside the assigned scope, report blockers clearly, and produce concise implementation notes."
    );

    let mut agent = AgentDefinition::new(agent_type, when_to_use, system_prompt, request.source);
    agent.tools = tools;

    GeneratedAgentDraft {
        agent,
        rationale: "Generated from the requested goal and selected tools.".to_string(),
    }
}

pub fn render_generated_agent_preview(draft: &GeneratedAgentDraft) -> String {
    [
        format!("name: {}", draft.agent.agent_type),
        format!("description: {}", draft.agent.when_to_use),
        format!("tools: {}", super::utils::tools_label(&draft.agent.tools)),
        format!("rationale: {}", draft.rationale),
    ]
    .join("\n")
}

fn derive_agent_type(goal: &str) -> String {
    let mut words = goal
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(3)
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if words.is_empty() {
        words.push("specialist".to_string());
    }
    words.join("-")
}
