//! Plugin manifest — `plugin.json` schema and validation.
//!
//! Every plugin directory must contain a `plugin.json` file that declares the
//! plugin's identity, capabilities, and requirements.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level plugin.json structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    /// Plugin identifier (unique within marketplace).
    pub name: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Version (semver).
    pub version: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Author name or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// License identifier.
    #[serde(default)]
    pub license: Option<String>,
    /// Minimum Claude Code version required.
    #[serde(default)]
    pub min_app_version: Option<String>,
    /// Tools contributed by this plugin.
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
    /// Skills contributed by this plugin.
    #[serde(default)]
    pub skills: Vec<SkillContribution>,
    /// MCP server definitions.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerContribution>,
    /// LSP server definitions. Supports inline maps or file references,
    /// mirroring the upstream `lspServers` plugin field.
    #[serde(default, rename = "lspServers", alias = "lsp_servers")]
    pub lsp_servers: Option<serde_json::Value>,
    /// Commands contributed by this plugin.
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    /// Dependencies on other plugins.
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    /// Configuration schema (user-configurable settings).
    #[serde(default)]
    pub configuration: Option<serde_json::Value>,
    /// Agent definitions contributed by this plugin.
    #[serde(default)]
    pub agents: Option<Vec<serde_json::Value>>,
    /// Hook definitions contributed by this plugin.
    #[serde(default)]
    pub hooks: Option<Vec<serde_json::Value>>,
    /// Output style definitions contributed by this plugin.
    #[serde(default, rename = "outputStyles", alias = "output_styles")]
    pub output_styles: Option<Vec<serde_json::Value>>,
}

/// A tool contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContribution {
    /// Tool name.
    pub name: String,
    /// Tool description.
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
    /// Whether this tool is read-only.
    #[serde(default)]
    pub read_only: bool,
    /// Whether this tool may run concurrently with sibling tool calls.
    #[serde(default)]
    pub concurrency_safe: bool,
    /// Executable runtime configuration for this tool.
    ///
    /// When omitted, the contribution is metadata-only and will not be
    /// registered as an executable runtime tool.
    #[serde(default)]
    pub runtime: Option<ToolRuntime>,
}

/// Executable runtime for a plugin-contributed tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolRuntime {
    /// Spawn a subprocess, write JSON input to stdin, and read stdout/stderr.
    Stdio(StdioToolRuntime),
}

/// Stdio runtime configuration for a plugin tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioToolRuntime {
    /// Executable name or relative path inside the plugin directory.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables for the subprocess.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional working directory for the subprocess.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Optional timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// A skill contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContribution {
    /// Skill name.
    pub name: String,
    /// Relative path to the SKILL.md file within the plugin directory.
    pub path: String,
    /// Description override.
    #[serde(default)]
    pub description: Option<String>,
}

/// An MCP server contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerContribution {
    /// Server name.
    pub name: String,
    /// Command to launch the server.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// A command contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandContribution {
    /// Command name (without leading /).
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Aliases.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Validate a plugin manifest.
pub fn validate_manifest(manifest: &PluginManifest) -> Result<()> {
    if manifest.name.is_empty() {
        bail!("Plugin name is required");
    }
    if manifest.name.len() > 100 {
        bail!("Plugin name exceeds 100 characters");
    }
    if manifest.version.is_empty() {
        bail!("Plugin version is required");
    }

    let valid_name = manifest
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || "-_@/".contains(c));
    if !valid_name {
        bail!(
            "Plugin name '{}' contains invalid characters. \
             Only alphanumeric, hyphens, underscores, @ and / are allowed.",
            manifest.name
        );
    }

    let parts: Vec<&str> = manifest.version.split('.').collect();
    if parts.len() < 2 || parts.len() > 4 {
        bail!(
            "Plugin version '{}' should be semver (e.g. 1.0.0)",
            manifest.version
        );
    }

    let mut tool_names: Vec<&str> = manifest.tools.iter().map(|t| t.name.as_str()).collect();
    tool_names.sort();
    let unique_len = {
        let mut v = tool_names.clone();
        v.dedup();
        v.len()
    };
    if unique_len != tool_names.len() {
        bail!("Duplicate tool names in manifest");
    }

    for tool in &manifest.tools {
        if let Some(ToolRuntime::Stdio(runtime)) = &tool.runtime {
            if runtime.command.trim().is_empty() {
                bail!(
                    "Plugin tool '{}' has an empty stdio runtime command",
                    tool.name
                );
            }
        }
    }

    Ok(())
}

/// Load and validate a plugin.json from a directory.
pub fn load_manifest(plugin_dir: &Path) -> Result<PluginManifest> {
    let manifest_path = plugin_dir.join("plugin.json");
    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;

    let manifest: PluginManifest = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

    validate_manifest(&manifest)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_manifest() -> PluginManifest {
        PluginManifest {
            name: "test-plugin".into(),
            display_name: None,
            version: "1.0.0".into(),
            description: "A test plugin".into(),
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
        }
    }

    #[test]
    fn test_validate_valid_manifest() {
        let m = minimal_manifest();
        assert!(validate_manifest(&m).is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let mut m = minimal_manifest();
        m.name = String::new();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_validate_invalid_name_chars() {
        let mut m = minimal_manifest();
        m.name = "bad plugin name!".into();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_validate_scoped_name() {
        let mut m = minimal_manifest();
        m.name = "@scope/my-plugin".into();
        assert!(validate_manifest(&m).is_ok());
    }

    #[test]
    fn test_validate_empty_version() {
        let mut m = minimal_manifest();
        m.version = String::new();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_validate_bad_version() {
        let mut m = minimal_manifest();
        m.version = "not-a-version".into();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_validate_duplicate_tools() {
        let mut m = minimal_manifest();
        m.tools = vec![
            ToolContribution {
                name: "dup".into(),
                description: "".into(),
                input_schema: None,
                read_only: false,
                concurrency_safe: false,
                runtime: None,
            },
            ToolContribution {
                name: "dup".into(),
                description: "".into(),
                input_schema: None,
                read_only: false,
                concurrency_safe: false,
                runtime: None,
            },
        ];
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_manifest_serde_roundtrip() {
        let mut m = minimal_manifest();
        m.tools = vec![ToolContribution {
            name: "my-tool".into(),
            description: "Does stuff".into(),
            input_schema: Some(serde_json::json!({"type": "object"})),
            read_only: true,
            concurrency_safe: true,
            runtime: Some(ToolRuntime::Stdio(StdioToolRuntime {
                command: "bin/my-tool".into(),
                args: vec!["--json".into()],
                env: HashMap::new(),
                cwd: Some(".".into()),
                timeout_ms: Some(30_000),
            })),
        }];
        m.skills = vec![SkillContribution {
            name: "my-skill".into(),
            path: "skills/my-skill/SKILL.md".into(),
            description: Some("A skill".into()),
        }];
        m.mcp_servers = vec![McpServerContribution {
            name: "my-server".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@mcp/server".into()],
            env: HashMap::new(),
        }];
        m.lsp_servers = Some(serde_json::json!({
            "demo-lsp": {
                "languageId": "demo",
                "extensions": ["demo"],
                "command": "demo-ls"
            }
        }));
        m.dependencies
            .insert("other-plugin".into(), "^1.0.0".into());

        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test-plugin");
        assert_eq!(back.tools.len(), 1);
        assert_eq!(back.skills.len(), 1);
        assert_eq!(back.mcp_servers.len(), 1);
        assert!(back.lsp_servers.is_some());
        assert_eq!(back.dependencies.len(), 1);
        assert!(matches!(back.tools[0].runtime, Some(ToolRuntime::Stdio(_))));
    }

    #[test]
    fn test_validate_stdio_runtime_requires_command() {
        let mut m = minimal_manifest();
        m.tools = vec![ToolContribution {
            name: "runtime-tool".into(),
            description: "".into(),
            input_schema: None,
            read_only: false,
            concurrency_safe: false,
            runtime: Some(ToolRuntime::Stdio(StdioToolRuntime {
                command: "   ".into(),
                args: vec![],
                env: HashMap::new(),
                cwd: None,
                timeout_ms: None,
            })),
        }];

        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn test_load_manifest_missing_file() {
        let result = load_manifest(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_with_config() {
        let mut m = minimal_manifest();
        m.configuration = Some(serde_json::json!({
            "type": "object",
            "properties": {
                "apiKey": { "type": "string", "description": "API key" }
            }
        }));
        let json = serde_json::to_string(&m).unwrap();
        let back: PluginManifest = serde_json::from_str(&json).unwrap();
        assert!(back.configuration.is_some());
    }
}
