//! Plugin LSP declaration collector.
//!
//! Collects LSP server declarations from plugin manifests for consumption
//! by Lane F (LSP Recommendation). Reads the manifest's `lspServers` field.

use serde::{Deserialize, Serialize};

use crate::manifest::PluginManifest;
use crate::PluginEntry;

/// An LSP server declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLspDeclaration {
    /// Plugin ID that contributes this LSP server.
    pub plugin_id: String,
    /// Language identifier (e.g. "typescript", "rust").
    pub language: String,
    /// Server command or path.
    pub server_command: String,
    /// Server arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions associated with this language.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Configuration for the LSP server.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// Collect LSP declarations from all enabled plugins.
///
/// Iterates enabled plugins, reads their manifests, and extracts LSP
/// server declarations from the `lspServers` field.
pub fn collect_plugin_lsp_declarations(
    enabled_plugins: &[PluginEntry],
) -> Vec<PluginLspDeclaration> {
    let mut declarations = Vec::new();

    for plugin in enabled_plugins {
        let Some(cache_path) = &plugin.cache_path else {
            continue;
        };
        let manifest_path = cache_path.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let manifest: PluginManifest = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let lsp_servers = match &manifest.lsp_servers {
            Some(servers) => servers,
            None => continue,
        };

        let obj = match lsp_servers.as_object() {
            Some(o) => o,
            None => continue,
        };

        for (server_name, server_config) in obj {
            let language = server_config
                .get("languageId")
                .and_then(|v| v.as_str())
                .unwrap_or(server_name);
            let command = server_config
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args: Vec<String> = server_config
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let extensions: Vec<String> = server_config
                .get("extensions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let config = server_config.get("settings").cloned();

            declarations.push(PluginLspDeclaration {
                plugin_id: plugin.id.clone(),
                language: language.to_string(),
                server_command: command.to_string(),
                args,
                extensions,
                config,
            });
        }
    }

    declarations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin_entry(id: &str, cache_path: &str) -> PluginEntry {
        PluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: "".to_string(),
            source: crate::PluginSource::Local {
                path: cache_path.to_string(),
            },
            status: crate::PluginStatus::Installed,
            marketplace: None,
            cache_path: Some(std::path::PathBuf::from(cache_path)),
            installed_version: Some("1.0.0".to_string()),
            official: false,
            download_url: None,
            homepage: None,
            sha256: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_no_plugins_no_declarations() {
        let decls = collect_plugin_lsp_declarations(&[]);
        assert!(decls.is_empty());
    }

    #[test]
    fn test_plugin_without_cache_path() {
        let plugin = PluginEntry {
            cache_path: None,
            ..make_plugin_entry("test", "/tmp")
        };
        let decls = collect_plugin_lsp_declarations(&[plugin]);
        assert!(decls.is_empty());
    }

    #[test]
    fn test_plugin_with_lsp_servers() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = serde_json::json!({
            "name": "lsp-plugin",
            "version": "1.0.0",
            "description": "LSP plugin",
            "lspServers": {
                "demo-lsp": {
                    "languageId": "demo",
                    "extensions": [".demo"],
                    "command": "demo-language-server",
                    "args": ["--stdio"],
                    "settings": {
                        "maxProblems": 100
                    }
                },
                "other-lsp": {
                    "languageId": "other",
                    "command": "other-ls"
                }
            }
        });
        std::fs::write(
            dir.path().join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let plugin = make_plugin_entry("lsp-plugin", dir.path().to_str().unwrap());
        let decls = collect_plugin_lsp_declarations(&[plugin]);

        assert_eq!(decls.len(), 2);

        let demo = decls.iter().find(|d| d.language == "demo").unwrap();
        assert_eq!(demo.server_command, "demo-language-server");
        assert_eq!(demo.args, vec!["--stdio"]);
        assert_eq!(demo.extensions, vec![".demo"]);
        assert!(demo.config.is_some());

        let other = decls.iter().find(|d| d.language == "other").unwrap();
        assert_eq!(other.server_command, "other-ls");
        assert!(other.extensions.is_empty());
    }

    #[test]
    fn test_plugin_manifest_uses_aliased_lsp_field() {
        let dir = tempfile::tempdir().unwrap();
        // Test with snake_case alias
        let manifest = serde_json::json!({
            "name": "lsp-plugin",
            "version": "1.0.0",
            "description": "Test",
            "lsp_servers": {
                "rust-analyzer": {
                    "languageId": "rust",
                    "command": "rust-analyzer"
                }
            }
        });
        std::fs::write(
            dir.path().join("plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let plugin = make_plugin_entry("lsp-plugin", dir.path().to_str().unwrap());
        let decls = collect_plugin_lsp_declarations(&[plugin]);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].language, "rust");
    }
}
