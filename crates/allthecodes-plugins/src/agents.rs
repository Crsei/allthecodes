//! Plugin agent contribution loader.
//!
//! Parses agent definitions from a plugin manifest's `agents` field and
//! provides them for registration in the agent system.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;

/// An agent contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAgentDefinition {
    /// Unique agent identifier within the plugin.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the agent's purpose.
    pub description: String,
    /// Recommended model for this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Tools available to this agent.
    #[serde(default)]
    pub tools: Vec<String>,
    /// System instructions for the agent.
    #[serde(default)]
    pub instructions: Option<String>,
    /// Plugin ID that owns this agent.
    #[serde(default)]
    pub plugin_id: String,
}

/// Parse agent definitions from a plugin manifest.
///
/// Reads the manifest's `agents` field, which is an optional array of
/// agent configuration objects.
pub fn load_plugin_agents(
    manifest: &PluginManifest,
    _plugin_dir: &Path,
    plugin_id: &str,
) -> Vec<PluginAgentDefinition> {
    match &manifest.agents {
        Some(agents) => agents
            .iter()
            .map(|agent| PluginAgentDefinition {
                id: agent
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: agent
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: agent
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                model: agent
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                tools: agent
                    .get("tools")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                instructions: agent
                    .get("instructions")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                plugin_id: plugin_id.to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;
    use serde_json::json;
    use std::collections::HashMap;

    fn test_manifest_with_agents(agents: serde_json::Value) -> PluginManifest {
        PluginManifest {
            name: "test-plugin".into(),
            display_name: None,
            version: "1.0.0".into(),
            description: "Test".into(),
            author: None,
            license: None,
            min_app_version: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            lsp_servers: None,
            commands: vec![],
            dependencies: HashMap::new(),
            configuration: None,
            agents: Some(agents.as_array().unwrap().clone()),
            hooks: None,
            output_styles: None,
        }
    }

    #[test]
    fn test_load_no_agents() {
        let manifest = PluginManifest {
            name: "test".into(),
            display_name: None,
            version: "1.0.0".into(),
            description: "".into(),
            author: None,
            license: None,
            min_app_version: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            lsp_servers: None,
            commands: vec![],
            dependencies: HashMap::new(),
            configuration: None,
            agents: None,
            hooks: None,
            output_styles: None,
        };

        let agents = load_plugin_agents(&manifest, Path::new("/tmp"), "test-plugin");
        assert!(agents.is_empty());
    }

    #[test]
    fn test_load_single_agent() {
        let agents_json = json!([{
            "id": "code-reviewer",
            "name": "Code Reviewer",
            "description": "Reviews code for issues",
            "model": "claude-sonnet-4",
            "tools": ["Read", "Write", "Bash"],
            "instructions": "Review code carefully."
        }]);

        let manifest = test_manifest_with_agents(agents_json);
        let agents = load_plugin_agents(&manifest, Path::new("/tmp"), "test-plugin");

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "code-reviewer");
        assert_eq!(agents[0].name, "Code Reviewer");
        assert_eq!(agents[0].model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(agents[0].tools.len(), 3);
        assert!(agents[0].tools.contains(&"Read".to_string()));
        assert_eq!(agents[0].plugin_id, "test-plugin");
    }

    #[test]
    fn test_load_multiple_agents() {
        let agents_json = json!([
            {"id": "agent-a", "name": "Agent A", "description": "First agent"},
            {"id": "agent-b", "name": "Agent B", "description": "Second agent", "model": "haiku"}
        ]);

        let manifest = test_manifest_with_agents(agents_json);
        let agents = load_plugin_agents(&manifest, Path::new("/tmp"), "test-plugin");

        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].id, "agent-a");
        assert_eq!(agents[1].id, "agent-b");
        assert_eq!(agents[1].model.as_deref(), Some("haiku"));
    }
}
