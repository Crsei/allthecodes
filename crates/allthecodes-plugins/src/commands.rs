//! Plugin command contribution loader.
//!
//! Parses command contributions from a plugin manifest and produces
//! `PluginCommandCandidate` values suitable for registration via Lane C's
//! `DynamicRegistry`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;

/// A command contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommandDefinition {
    /// Command name (without leading `/`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Alternative names for the command.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Optional frontmatter / usage hints.
    #[serde(default)]
    pub usage: Option<String>,
    /// Execution type hint.
    #[serde(default)]
    pub execution_hint: String,
}

/// A candidate for registration in the dynamic command registry.
///
/// This is consumed by the integration layer to call
/// `allthecodes_commands::plugin_commands::register_plugin_commands()`.
#[derive(Debug, Clone)]
pub struct PluginCommandCandidate {
    /// Command name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Plugin ID that owns this command.
    pub plugin_id: String,
}

/// Parse command contributions from a plugin manifest.
///
/// Iterates the manifest's `commands` field and produces candidate
/// command definitions for registration.
pub fn load_plugin_commands(
    manifest: &PluginManifest,
    _plugin_dir: &Path,
) -> Vec<PluginCommandDefinition> {
    manifest
        .commands
        .iter()
        .map(|cmd| PluginCommandDefinition {
            name: cmd.name.clone(),
            description: cmd.description.clone(),
            aliases: cmd.aliases.clone(),
            usage: None,
            execution_hint: "plugin".to_string(),
        })
        .collect()
}

/// Convert plugin command definitions to candidates for registration.
pub fn to_candidates(
    definitions: &[PluginCommandDefinition],
    plugin_id: &str,
) -> Vec<PluginCommandCandidate> {
    definitions
        .iter()
        .map(|def| PluginCommandCandidate {
            name: def.name.clone(),
            description: def.description.clone(),
            plugin_id: plugin_id.to_string(),
        })
        .collect()
}

/// Parse Markdown frontmatter for additional command metadata.
///
/// Some plugins define commands in Markdown files with YAML frontmatter.
/// This function extracts the frontmatter fields relevant to commands.
pub fn parse_command_frontmatter(content: &str) -> Option<PluginCommandDefinition> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end];

    let mut name = None;
    let mut description = None;
    let mut aliases = Vec::new();
    let mut execution_hint = String::new();

    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("alias:") {
            aliases.push(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("aliases:") {
            let list = value.trim().trim_start_matches('[').trim_end_matches(']');
            for item in list.split(',') {
                let item = item.trim().trim_matches('"').trim_matches('\'');
                if !item.is_empty() {
                    aliases.push(item.to_string());
                }
            }
        } else if let Some(value) = line.strip_prefix("execution:") {
            execution_hint = value.trim().to_string();
        }
    }

    Some(PluginCommandDefinition {
        name: name.unwrap_or_default(),
        description: description.unwrap_or_default(),
        aliases,
        usage: None,
        execution_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::CommandContribution;
    use std::collections::HashMap;

    fn test_manifest() -> PluginManifest {
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
            commands: vec![
                CommandContribution {
                    name: "my-cmd".into(),
                    description: "My custom command".into(),
                    aliases: vec!["mc".into()],
                },
                CommandContribution {
                    name: "another-cmd".into(),
                    description: "Another command".into(),
                    aliases: vec![],
                },
            ],
            dependencies: HashMap::new(),
            configuration: None,
            agents: None,
            hooks: None,
            output_styles: None,
        }
    }

    #[test]
    fn test_load_plugin_commands() {
        let manifest = test_manifest();
        let commands = load_plugin_commands(&manifest, Path::new("/tmp"));
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "my-cmd");
        assert_eq!(commands[0].aliases, vec!["mc"]);
        assert_eq!(commands[1].name, "another-cmd");
    }

    #[test]
    fn test_to_candidates() {
        let defs = vec![PluginCommandDefinition {
            name: "test".into(),
            description: "Test command".into(),
            aliases: vec![],
            usage: None,
            execution_hint: "plugin".into(),
        }];

        let candidates = to_candidates(&defs, "my-plugin");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "test");
        assert_eq!(candidates[0].plugin_id, "my-plugin");
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
name: my-command
description: Does something useful
alias: mc
execution: plugin
---
Command content here
"#;

        let result = parse_command_frontmatter(content).unwrap();
        assert_eq!(result.name, "my-command");
        assert_eq!(result.description, "Does something useful");
        assert_eq!(result.aliases, vec!["mc"]);
        assert_eq!(result.execution_hint, "plugin");
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "Just regular content";
        assert!(parse_command_frontmatter(content).is_none());
    }

    #[test]
    fn test_parse_frontmatter_multi_aliases() {
        let content = r#"---
name: multi
aliases: [a, b, c]
---
"#;

        let result = parse_command_frontmatter(content).unwrap();
        assert_eq!(result.name, "multi");
        assert_eq!(result.aliases, vec!["a", "b", "c"]);
    }
}
