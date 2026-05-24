//! Formatting utilities shared by the Rust-side agent UI.

use std::collections::BTreeMap;

use super::types::{AgentDefinition, AgentSource, AgentSourceFilter};

pub fn get_agent_source_display_name(source: AgentSourceFilter) -> String {
    match source {
        AgentSourceFilter::All => "Agents".to_string(),
        AgentSourceFilter::BuiltIn => "Built-in agents".to_string(),
        AgentSourceFilter::Plugin => "Plugin agents".to_string(),
        AgentSourceFilter::Source(source) => format!("{} agents", source.display_name()),
    }
}

pub fn source_rank(source: AgentSource) -> usize {
    match source {
        AgentSource::Project => 0,
        AgentSource::Local => 1,
        AgentSource::User => 2,
        AgentSource::Policy => 3,
        AgentSource::Flag => 4,
        AgentSource::Plugin => 5,
        AgentSource::BuiltIn => 6,
    }
}

pub fn sorted_agents(mut agents: Vec<AgentDefinition>) -> Vec<AgentDefinition> {
    agents.sort_by(|a, b| {
        source_rank(a.source)
            .cmp(&source_rank(b.source))
            .then_with(|| {
                a.agent_type
                    .to_lowercase()
                    .cmp(&b.agent_type.to_lowercase())
            })
            .then_with(|| a.agent_type.cmp(&b.agent_type))
    });
    agents
}

pub fn filter_agents(
    agents: &[AgentDefinition],
    filter: AgentSourceFilter,
) -> Vec<AgentDefinition> {
    let filtered = agents.iter().filter(|agent| match filter {
        AgentSourceFilter::All => true,
        AgentSourceFilter::BuiltIn => agent.source == AgentSource::BuiltIn,
        AgentSourceFilter::Plugin => agent.source == AgentSource::Plugin,
        AgentSourceFilter::Source(source) => agent.source == source,
    });
    sorted_agents(filtered.cloned().collect())
}

pub fn group_agents_by_source(
    agents: &[AgentDefinition],
) -> BTreeMap<AgentSource, Vec<AgentDefinition>> {
    let mut groups: BTreeMap<AgentSource, Vec<AgentDefinition>> = BTreeMap::new();
    for agent in agents {
        groups.entry(agent.source).or_default().push(agent.clone());
    }
    for group in groups.values_mut() {
        group.sort_by(|a, b| {
            a.agent_type
                .to_lowercase()
                .cmp(&b.agent_type.to_lowercase())
        });
    }
    groups
}

pub fn tools_label(tools: &Option<Vec<String>>) -> String {
    match tools {
        None => "All tools".to_string(),
        Some(tools) if tools.is_empty() => "None".to_string(),
        Some(tools) if tools.len() == 1 && tools[0] == "*" => "All tools".to_string(),
        Some(tools) => tools.join(", "),
    }
}

pub fn model_label(model: &Option<String>) -> String {
    model.as_deref().unwrap_or("default").to_string()
}

pub fn skills_label(skills: &[String]) -> String {
    if skills.is_empty() {
        "None".to_string()
    } else if skills.len() > 10 {
        format!("{} skills", skills.len())
    } else {
        skills.join(", ")
    }
}

pub fn hooks_label(hooks: &BTreeMap<String, Vec<String>>) -> String {
    if hooks.is_empty() {
        "None".to_string()
    } else {
        hooks.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

pub fn memory_label(agent: &AgentDefinition) -> String {
    agent
        .memory
        .map(|scope| scope.display_name().to_string())
        .unwrap_or_else(|| "default".to_string())
}

pub fn selection_marker(selected: bool) -> &'static str {
    if selected {
        ">"
    } else {
        " "
    }
}

pub fn truncate_middle(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return value.chars().take(max_chars).collect();
    }

    let left = (max_chars - 3) / 2;
    let right = max_chars - 3 - left;
    let prefix: String = value.chars().take(left).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(right)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut current = String::new();
        for word in raw.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current);
                current = word.to_string();
            }
        }
        if current.is_empty() {
            lines.push(String::new());
        } else {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn indent_lines(lines: impl IntoIterator<Item = String>, spaces: usize) -> Vec<String> {
    let prefix = " ".repeat(spaces);
    lines
        .into_iter()
        .map(|line| format!("{prefix}{line}"))
        .collect()
}

pub fn render_kv_rows(
    rows: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> String {
    rows.into_iter()
        .map(|(key, value)| format!("{:<16} {}", key.as_ref(), value.as_ref()))
        .collect::<Vec<_>>()
        .join("\n")
}
