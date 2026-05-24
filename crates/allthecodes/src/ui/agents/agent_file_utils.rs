//! Filesystem-adjacent helpers for displaying and serializing agents.

use std::path::{Path, PathBuf};

use super::types::{AgentDefinition, AgentSource, AGENTS_DIR, AGENT_FOLDER_NAME};
use super::utils::{memory_label, tools_label};

pub fn format_agent_as_markdown(agent: &AgentDefinition) -> String {
    let mut header = vec![
        "---".to_string(),
        format!("name: {}", agent.agent_type),
        format!("description: \"{}\"", yaml_double_quote(&agent.when_to_use)),
    ];

    if let Some(tools) = &agent.tools {
        if !(tools.is_empty() || (tools.len() == 1 && tools[0] == "*")) {
            header.push(format!("tools: {}", tools.join(", ")));
        }
    }

    if let Some(model) = &agent.model {
        header.push(format!("model: {model}"));
    }
    if let Some(effort) = &agent.effort {
        header.push(format!("effort: {effort}"));
    }
    if let Some(color) = &agent.color {
        header.push(format!("color: {color}"));
    }
    if let Some(memory) = agent.memory {
        header.push(format!("memory: {}", memory.display_name()));
    }
    if let Some(mode) = &agent.permission_mode {
        header.push(format!("permission_mode: {mode}"));
    }

    header.push("---".to_string());
    header.push(String::new());
    header.push(agent.system_prompt.trim_end().to_string());
    header.push(String::new());
    header.join("\n")
}

pub fn get_new_agent_file_path(
    source: AgentSource,
    agent_type: &str,
    cwd: &Path,
    home: &Path,
    managed_root: &Path,
) -> PathBuf {
    agent_dir(source, cwd, home, managed_root)
        .join(format!("{}.md", sanitize_agent_filename(agent_type)))
}

pub fn get_actual_relative_agent_file_path(agent: &AgentDefinition) -> String {
    if agent.source == AgentSource::BuiltIn {
        return "Built-in".to_string();
    }
    if agent.source == AgentSource::Plugin {
        return format!("Plugin: {}", agent.plugin.as_deref().unwrap_or("Unknown"));
    }
    if agent.source == AgentSource::Flag {
        return "CLI argument".to_string();
    }

    let filename = agent.filename.as_deref().unwrap_or(&agent.agent_type);
    match agent.source {
        AgentSource::Project | AgentSource::Local => {
            format!("./{AGENT_FOLDER_NAME}/{AGENTS_DIR}/{filename}.md")
        }
        _ => agent
            .base_dir
            .as_ref()
            .map(|base| format!("{base}/{filename}.md").replace('\\', "/"))
            .unwrap_or_else(|| format!("{AGENT_FOLDER_NAME}/{AGENTS_DIR}/{filename}.md")),
    }
}

pub fn render_agent_file_summary(agent: &AgentDefinition) -> String {
    [
        format!("path: {}", get_actual_relative_agent_file_path(agent)),
        format!("source: {}", agent.source.display_name()),
        format!("tools: {}", tools_label(&agent.tools)),
        format!("memory: {}", memory_label(agent)),
    ]
    .join("\n")
}

pub fn sanitize_agent_filename(agent_type: &str) -> String {
    let mut sanitized = String::new();
    for ch in agent_type.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() {
            sanitized.push('-');
        }
    }
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    sanitized.trim_matches('-').to_string()
}

fn agent_dir(source: AgentSource, cwd: &Path, home: &Path, managed_root: &Path) -> PathBuf {
    match source {
        AgentSource::User => home.join(AGENT_FOLDER_NAME).join(AGENTS_DIR),
        AgentSource::Project | AgentSource::Local => cwd.join(AGENT_FOLDER_NAME).join(AGENTS_DIR),
        AgentSource::Policy => managed_root.join(AGENT_FOLDER_NAME).join(AGENTS_DIR),
        AgentSource::BuiltIn | AgentSource::Plugin | AgentSource::Flag => {
            cwd.join(AGENT_FOLDER_NAME).join(AGENTS_DIR)
        }
    }
}

fn yaml_double_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
