//! State model for the Rust-side create-agent wizard.

use super::types::{AgentMemoryScope, AgentSource};
pub mod create_agent_wizard;
pub mod wizard_steps;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCreationMethod {
    Generate,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWizardData {
    pub location: Option<AgentSource>,
    pub method: Option<AgentCreationMethod>,
    pub generation_goal: Option<String>,
    pub agent_type: Option<String>,
    pub system_prompt: Option<String>,
    pub when_to_use: Option<String>,
    pub tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub color: Option<String>,
    pub memory: Option<AgentMemoryScope>,
}

impl AgentWizardData {
    pub fn empty() -> Self {
        Self {
            location: None,
            method: None,
            generation_goal: None,
            agent_type: None,
            system_prompt: None,
            when_to_use: None,
            tools: None,
            model: None,
            color: None,
            memory: None,
        }
    }

    pub fn is_ready_to_confirm(&self) -> bool {
        self.location.is_some()
            && self.agent_type.as_deref().unwrap_or("").trim().len() > 1
            && self.system_prompt.as_deref().unwrap_or("").trim().len() > 10
            && self.when_to_use.as_deref().unwrap_or("").trim().len() > 5
    }
}
