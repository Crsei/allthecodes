//! Comprehensive plugin validation.
//!
//! Validates plugin manifests, directory structure, file types, and
//! inter-plugin dependencies. Produces structured validation errors
//! with severity levels.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::manifest::{load_manifest, PluginManifest};

/// Severity of a validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationSeverity {
    /// Non-critical issue that should be reported but doesn't block loading.
    Warning,
    /// Critical issue that prevents plugin loading.
    Error,
}

/// A validation error or warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginValidationError {
    /// Severity level.
    pub severity: ValidationSeverity,
    /// The field path where the issue was found (e.g. "manifest.name").
    pub field: String,
    /// Human-readable message describing the issue.
    pub message: String,
    /// Optional suggestion for fixing the issue.
    #[serde(default)]
    pub suggestion: Option<String>,
}

/// Comprehensive plugin validator.
pub struct PluginValidator;

impl PluginValidator {
    /// Validate a plugin directory.
    ///
    /// Checks:
    /// - Manifest exists and parses correctly
    /// - Manifest fields are valid
    /// - Directory structure is reasonable
    /// - Referenced files exist
    pub fn validate_plugin(plugin_dir: &Path) -> Vec<PluginValidationError> {
        let mut errors = Vec::new();

        // Check directory exists
        if !plugin_dir.exists() {
            errors.push(PluginValidationError {
                severity: ValidationSeverity::Error,
                field: "plugin_dir".to_string(),
                message: format!("Plugin directory does not exist: {}", plugin_dir.display()),
                suggestion: Some("Ensure the plugin directory path is correct.".to_string()),
            });
            return errors;
        }

        // Check plugin.json exists
        let manifest_path = plugin_dir.join("plugin.json");
        if !manifest_path.exists() {
            errors.push(PluginValidationError {
                severity: ValidationSeverity::Error,
                field: "manifest".to_string(),
                message: format!("plugin.json not found at {}", manifest_path.display()),
                suggestion: Some(
                    "A plugin.json file is required in the plugin directory.".to_string(),
                ),
            });
            return errors;
        }

        // Load and validate manifest
        let manifest = match load_manifest(plugin_dir) {
            Ok(m) => m,
            Err(e) => {
                errors.push(PluginValidationError {
                    severity: ValidationSeverity::Error,
                    field: "manifest".to_string(),
                    message: format!("Failed to load plugin manifest: {}", e),
                    suggestion: Some(
                        "Check that plugin.json is valid JSON and all required fields are present."
                            .to_string(),
                    ),
                });
                return errors;
            }
        };

        // Extended validation
        errors.extend(Self::validate_manifest_ext(&manifest, plugin_dir));

        errors
    }

    /// Extended manifest validation beyond what `validate_manifest()` provides.
    pub fn validate_manifest_ext(
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> Vec<PluginValidationError> {
        let mut errors = Vec::new();

        // Validate skill paths exist
        for skill in &manifest.skills {
            let skill_path = plugin_dir.join(&skill.path);
            if !skill_path.exists() {
                errors.push(PluginValidationError {
                    severity: ValidationSeverity::Warning,
                    field: format!("skills.{}.path", skill.name),
                    message: format!("Skill file not found: {}", skill_path.display()),
                    suggestion: Some(
                        "Ensure the skill file exists at the specified path.".to_string(),
                    ),
                });
            }
        }

        // Validate MCP server commands are not empty
        for mcp in &manifest.mcp_servers {
            if mcp.command.trim().is_empty() {
                errors.push(PluginValidationError {
                    severity: ValidationSeverity::Error,
                    field: format!("mcp_servers.{}.command", mcp.name),
                    message: format!("MCP server '{}' has empty command", mcp.name),
                    suggestion: Some("Provide a valid command for the MCP server.".to_string()),
                });
            }
        }

        // Validate tool runtime commands
        for tool in &manifest.tools {
            if let Some(ref runtime) = tool.runtime {
                if matches!(
                    runtime,
                    crate::manifest::ToolRuntime::Stdio(r)
                    if r.command.trim().is_empty()
                ) {
                    errors.push(PluginValidationError {
                        severity: ValidationSeverity::Error,
                        field: format!("tools.{}.runtime.command", tool.name),
                        message: format!("Tool '{}' has an empty runtime command", tool.name),
                        suggestion: Some(
                            "Provide a valid command for the tool runtime.".to_string(),
                        ),
                    });
                }
            }
        }

        // Validate command names
        for cmd in &manifest.commands {
            if cmd.name.trim().is_empty() {
                errors.push(PluginValidationError {
                    severity: ValidationSeverity::Error,
                    field: "commands.name".to_string(),
                    message: "A command has an empty name".to_string(),
                    suggestion: Some("Each command must have a non-empty name.".to_string()),
                });
            }
        }

        // Check for deprecated/invalid characters in plugin name
        if manifest.name.contains(' ') {
            errors.push(PluginValidationError {
                severity: ValidationSeverity::Warning,
                field: "manifest.name".to_string(),
                message: format!("Plugin name '{}' contains spaces", manifest.name),
                suggestion: Some(
                    "Use hyphens or underscores instead of spaces in plugin names.".to_string(),
                ),
            });
        }

        // Validate description length
        if manifest.description.len() > 500 {
            errors.push(PluginValidationError {
                severity: ValidationSeverity::Warning,
                field: "manifest.description".to_string(),
                message: "Plugin description exceeds 500 characters".to_string(),
                suggestion: Some("Keep descriptions concise (under 500 characters).".to_string()),
            });
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    fn valid_manifest_json() -> serde_json::Value {
        serde_json::json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "description": "A test plugin"
        })
    }

    fn create_plugin_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let manifest = valid_manifest_json();
        let mut f = std::fs::File::create(dir.path().join("plugin.json")).unwrap();
        write!(f, "{}", serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        dir
    }

    #[test]
    fn test_validate_nonexistent_dir() {
        let errors = PluginValidator::validate_plugin(Path::new("/nonexistent/path"));
        assert!(!errors.is_empty());
        assert_eq!(errors[0].severity, ValidationSeverity::Error);
        assert!(errors[0].field.contains("plugin_dir"));
    }

    #[test]
    fn test_validate_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let errors = PluginValidator::validate_plugin(dir.path());
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.field == "manifest"));
    }

    #[test]
    fn test_validate_valid_plugin() {
        let dir = create_plugin_dir();
        let errors = PluginValidator::validate_plugin(dir.path());
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_skill_missing() {
        let dir = create_plugin_dir();
        let manifest_path = dir.path().join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let mut m: PluginManifest = serde_json::from_str(&content).unwrap();
        m.skills = vec![crate::manifest::SkillContribution {
            name: "test-skill".into(),
            path: "skills/missing.md".into(),
            description: None,
        }];
        let updated = serde_json::to_string_pretty(&m).unwrap();
        std::fs::write(&manifest_path, &updated).unwrap();

        let errors = PluginValidator::validate_plugin(dir.path());
        assert!(errors.iter().any(|e| e.field.contains("skills")));
    }

    #[test]
    fn test_validate_empty_mcp_command() {
        let dir = create_plugin_dir();
        let manifest_path = dir.path().join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let mut m: PluginManifest = serde_json::from_str(&content).unwrap();
        m.mcp_servers = vec![crate::manifest::McpServerContribution {
            name: "empty-server".into(),
            command: "".into(),
            args: vec![],
            env: HashMap::new(),
        }];
        let updated = serde_json::to_string_pretty(&m).unwrap();
        std::fs::write(&manifest_path, &updated).unwrap();

        let errors = PluginValidator::validate_plugin(dir.path());
        assert!(errors.iter().any(|e| e.field.contains("mcp_servers")));
    }

    #[test]
    fn test_validate_empty_command_name() {
        let dir = create_plugin_dir();
        let manifest_path = dir.path().join("plugin.json");
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let mut m: PluginManifest = serde_json::from_str(&content).unwrap();
        m.commands = vec![crate::manifest::CommandContribution {
            name: "".into(),
            description: "empty name".into(),
            aliases: vec![],
        }];
        let updated = serde_json::to_string_pretty(&m).unwrap();
        std::fs::write(&manifest_path, &updated).unwrap();

        let errors = PluginValidator::validate_plugin(dir.path());
        assert!(errors.iter().any(|e| e.field == "commands.name"));
    }

    #[test]
    fn test_validate_name_with_spaces() {
        // Test validate_manifest_ext directly, bypassing validate_manifest
        // which would reject the name before we reach the extended check.
        let dir = tempfile::tempdir().unwrap();
        let manifest = PluginManifest {
            name: "test plugin".into(),
            version: "1.0.0".into(),
            description: "Has spaces".into(),
            ..Default::default()
        };

        let errors = PluginValidator::validate_manifest_ext(&manifest, dir.path());
        assert!(errors.iter().any(|e| e.field == "manifest.name"));
        assert_eq!(
            errors
                .iter()
                .find(|e| e.field == "manifest.name")
                .unwrap()
                .severity,
            ValidationSeverity::Warning
        );
    }
}
