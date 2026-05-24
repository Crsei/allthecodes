//! Agent list rendering and selection state.

use super::types::{AgentDefinition, AgentSourceFilter};
use super::utils::filter_agents;
use super::utils::group_agents_by_source;
use super::utils::{get_agent_source_display_name, memory_label, model_label, selection_marker};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsListState {
    pub source: AgentSourceFilter,
    pub agents: Vec<AgentDefinition>,
    pub selected_index: usize,
    pub create_new_selected: bool,
    pub show_create_new: bool,
}

impl AgentsListState {
    pub fn new(source: AgentSourceFilter, agents: Vec<AgentDefinition>) -> Self {
        Self {
            source,
            agents,
            selected_index: 0,
            create_new_selected: true,
            show_create_new: true,
        }
    }

    pub fn visible_agents(&self) -> Vec<AgentDefinition> {
        filter_agents(&self.agents, self.source)
    }

    pub fn move_next(&mut self) {
        let total = self.visible_agents().len() + usize::from(self.show_create_new);
        if total == 0 {
            return;
        }
        let current = if self.create_new_selected {
            0
        } else {
            self.selected_index + usize::from(self.show_create_new)
        };
        let next = (current + 1) % total;
        self.create_new_selected = self.show_create_new && next == 0;
        self.selected_index = next.saturating_sub(usize::from(self.show_create_new));
    }

    pub fn move_prev(&mut self) {
        let total = self.visible_agents().len() + usize::from(self.show_create_new);
        if total == 0 {
            return;
        }
        let current = if self.create_new_selected {
            0
        } else {
            self.selected_index + usize::from(self.show_create_new)
        };
        let previous = if current == 0 { total - 1 } else { current - 1 };
        self.create_new_selected = self.show_create_new && previous == 0;
        self.selected_index = previous.saturating_sub(usize::from(self.show_create_new));
    }

    pub fn selected_agent(&self) -> Option<AgentDefinition> {
        if self.create_new_selected {
            return None;
        }
        self.visible_agents().get(self.selected_index).cloned()
    }

    pub fn render(&self) -> String {
        let mut lines = vec![get_agent_source_display_name(self.source)];
        if self.show_create_new {
            lines.push(format!(
                "{} Create new agent",
                selection_marker(self.create_new_selected)
            ));
        }

        let visible = self.visible_agents();
        let groups = group_agents_by_source(&visible);
        for (source, agents) in groups {
            lines.push(format!("{}:", source.display_name()));
            for agent in agents {
                let selected = !self.create_new_selected
                    && visible
                        .get(self.selected_index)
                        .map(|selected| {
                            selected.agent_type == agent.agent_type
                                && selected.source == agent.source
                        })
                        .unwrap_or(false);
                let mut suffix = format!(" - {}", model_label(&agent.model));
                if agent.memory.is_some() {
                    suffix.push_str(&format!(" - {} memory", memory_label(&agent)));
                }
                if let Some(overridden_by) = agent.overridden_by {
                    suffix.push_str(&format!(" - shadowed by {}", overridden_by.display_name()));
                }
                lines.push(format!(
                    "{} {}{}",
                    selection_marker(selected),
                    agent.agent_type,
                    suffix
                ));
            }
        }

        lines.join("\n")
    }
}
