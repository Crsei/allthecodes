//! Plugin output style contribution loader.
//!
//! Parses output style definitions from a plugin manifest's `outputStyles`
//! field. Output styles control how plugin tool results are formatted and
//! displayed.

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;

/// An output style contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginOutputStyle {
    /// Style name for identification.
    pub name: String,
    /// Matcher pattern to determine when this style applies.
    #[serde(default)]
    pub matcher: Option<String>,
    /// Format configuration as a JSON value.
    #[serde(default)]
    pub format_config: Option<serde_json::Value>,
}

/// Parse output style definitions from a plugin manifest.
///
/// Reads the manifest's `outputStyles` field, which is an optional array
/// of output style configuration objects.
pub fn load_plugin_output_styles(manifest: &PluginManifest) -> Vec<PluginOutputStyle> {
    match &manifest.output_styles {
        Some(styles) => styles
            .iter()
            .map(|style| PluginOutputStyle {
                name: style
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                matcher: style
                    .get("matcher")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                format_config: style
                    .get("formatConfig")
                    .or_else(|| style.get("format_config"))
                    .cloned(),
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

    fn manifest_with_styles(styles: serde_json::Value) -> PluginManifest {
        PluginManifest {
            name: "style-plugin".into(),
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
            output_styles: Some(styles.as_array().unwrap().clone()),
        }
    }

    #[test]
    fn test_load_no_styles() {
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

        let styles = load_plugin_output_styles(&manifest);
        assert!(styles.is_empty());
    }

    #[test]
    fn test_load_single_style() {
        let styles_json = json!([{
            "name": "json-pretty",
            "matcher": "*.json",
            "formatConfig": {
                "indent": 2,
                "sortKeys": true
            }
        }]);

        let manifest = manifest_with_styles(styles_json);
        let styles = load_plugin_output_styles(&manifest);

        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].name, "json-pretty");
        assert_eq!(styles[0].matcher.as_deref(), Some("*.json"));
        assert!(styles[0].format_config.is_some());
    }

    #[test]
    fn test_load_style_without_matcher() {
        let styles_json = json!([{
            "name": "default",
            "formatConfig": {"truncate": 1000}
        }]);

        let manifest = manifest_with_styles(styles_json);
        let styles = load_plugin_output_styles(&manifest);

        assert_eq!(styles.len(), 1);
        assert!(styles[0].matcher.is_none());
    }

    #[test]
    fn test_format_config_uses_camel_case() {
        let styles_json = json!([{
            "name": "styled",
            "formatConfig": {"colorScheme": "dark"}
        }]);

        let manifest = manifest_with_styles(styles_json);
        let styles = load_plugin_output_styles(&manifest);

        let config = styles[0].format_config.as_ref().unwrap();
        assert_eq!(config["colorScheme"], "dark");
    }
}
