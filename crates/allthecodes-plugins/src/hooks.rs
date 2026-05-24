//! Plugin hook contribution loader.
//!
//! Parses hook definitions from a plugin manifest's `hooks` field and
//! provides them for registration in the hook system.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;

/// A hook contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHookDefinition {
    /// Human-readable name for this hook.
    pub name: String,
    /// Trigger event that activates this hook.
    pub trigger: String,
    /// Action to perform when triggered.
    pub action: String,
    /// Conditions that must be met for the hook to fire.
    #[serde(default)]
    pub conditions: Vec<String>,
    /// Plugin ID that owns this hook.
    #[serde(default)]
    pub plugin_id: String,
}

/// Parse hook definitions from a plugin manifest.
///
/// Reads the manifest's `hooks` field, which is an optional array of
/// hook configuration objects.
pub fn load_plugin_hooks(
    manifest: &PluginManifest,
    _plugin_dir: &Path,
    plugin_id: &str,
) -> Vec<PluginHookDefinition> {
    match &manifest.hooks {
        Some(hooks) => hooks
            .iter()
            .map(|hook| PluginHookDefinition {
                name: hook
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                trigger: hook
                    .get("trigger")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                action: hook
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                conditions: hook
                    .get("conditions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                plugin_id: plugin_id.to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn manifest_with_hooks(hooks: serde_json::Value) -> PluginManifest {
        PluginManifest {
            name: "hook-plugin".into(),
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
            hooks: Some(hooks.as_array().unwrap().clone()),
            output_styles: None,
        }
    }

    #[test]
    fn test_load_no_hooks() {
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

        let hooks = load_plugin_hooks(&manifest, Path::new("/tmp"), "test");
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_single_hook() {
        let hooks_json = json!([{
            "name": "post-install",
            "trigger": "plugin:installed",
            "action": "notify",
            "conditions": ["plugin.source == 'marketplace'"]
        }]);

        let manifest = manifest_with_hooks(hooks_json);
        let hooks = load_plugin_hooks(&manifest, Path::new("/tmp"), "hook-plugin");

        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "post-install");
        assert_eq!(hooks[0].trigger, "plugin:installed");
        assert_eq!(hooks[0].action, "notify");
        assert_eq!(hooks[0].conditions.len(), 1);
        assert_eq!(hooks[0].plugin_id, "hook-plugin");
    }

    #[test]
    fn test_load_hook_without_conditions() {
        let hooks_json = json!([{
            "name": "simple-hook",
            "trigger": "tool:before_execute",
            "action": "log"
        }]);

        let manifest = manifest_with_hooks(hooks_json);
        let hooks = load_plugin_hooks(&manifest, Path::new("/tmp"), "hook-plugin");

        assert_eq!(hooks.len(), 1);
        assert!(hooks[0].conditions.is_empty());
    }
}
