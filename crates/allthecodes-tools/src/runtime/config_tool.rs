//! Config tool -- runtime settings read/write.
//!
//! Provides get/set/list operations on the project `.allthecodes/settings.json` file.
//! - "get"  : returns the value for a specific key
//! - "set"  : modifies a setting (writes back to the project config file)
//! - "list" : returns all current settings

use std::path::{Path, PathBuf};

use allthecodes_config::settings;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

use crate::tool::*;
use allthecodes_types::message::AssistantMessage;

/// ConfigTool -- read and write runtime configuration.
pub struct ConfigTool;

impl ConfigTool {
    fn current_dir() -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    fn project_settings_path_for_cwd(cwd: &Path) -> PathBuf {
        settings::project_settings_path(cwd)
    }

    /// Load the settings file as a serde_json::Value (Object).
    /// Returns an empty object if the file does not exist.
    fn load_settings(path: &Path) -> Result<Value> {
        if !path.exists() {
            return Ok(json!({}));
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let val: Value = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(val)
    }

    /// Save a Value (Object) back to the settings file, creating parent dirs
    /// if necessary.
    fn save_settings(path: &Path, settings: &Value) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let pretty =
            serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;
        std::fs::write(path, pretty)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }
}

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "Config"
    }

    async fn description(&self, input: &Value) -> String {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        match action {
            "get" => "Get a configuration setting value.".to_string(),
            "set" => "Set a configuration setting value.".to_string(),
            "list" => "List all configuration settings.".to_string(),
            _ => "Manage runtime configuration settings.".to_string(),
        }
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "list"],
                    "description": "The action to perform: get, set, or list"
                },
                "key": {
                    "type": "string",
                    "description": "The setting key (required for get and set)"
                },
                "value": {
                    "type": "string",
                    "description": "The setting value (required for set)"
                }
            },
            "required": ["action"]
        })
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        matches!(action, "get" | "list")
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        // Read operations are safe; write operations are not.
        self.is_read_only(input)
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");

        match action {
            "get" => {
                if input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    return ValidationResult::Error {
                        message: "\"key\" is required for action \"get\"".to_string(),
                        error_code: 1,
                    };
                }
            }
            "set" => {
                if input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    return ValidationResult::Error {
                        message: "\"key\" is required for action \"set\"".to_string(),
                        error_code: 1,
                    };
                }
                // value can be an empty string (to clear), but must be present
                if input.get("value").is_none() {
                    return ValidationResult::Error {
                        message: "\"value\" is required for action \"set\"".to_string(),
                        error_code: 1,
                    };
                }
            }
            "list" => { /* no extra params needed */ }
            _ => {
                return ValidationResult::Error {
                    message: format!("Unknown action \"{}\". Must be get, set, or list.", action),
                    error_code: 1,
                };
            }
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value_str = input.get("value").and_then(|v| v.as_str());

        let cwd = Self::current_dir();
        let settings_path = Self::project_settings_path_for_cwd(&cwd);

        match action {
            "get" => {
                let settings = Self::load_settings(&settings_path)?;
                let val = settings.get(key).cloned().unwrap_or(Value::Null);
                debug!(key = key, "Config get");
                Ok(ToolResult {
                    data: json!({ "key": key, "value": val }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            "set" => {
                let mut settings = Self::load_settings(&settings_path)?;
                // Ensure settings is an object
                if !settings.is_object() {
                    settings = json!({});
                }

                // Try to parse as JSON first; otherwise store as string
                let parsed_value: Value = if let Some(vs) = value_str {
                    serde_json::from_str(vs).unwrap_or_else(|_| Value::String(vs.to_string()))
                } else {
                    Value::Null
                };

                settings
                    .as_object_mut()
                    .expect("settings guaranteed to be an object")
                    .insert(key.to_string(), parsed_value.clone());
                Self::save_settings(&settings_path, &settings)?;

                debug!(key = key, "Config set");
                Ok(ToolResult {
                    data: json!({
                        "message": format!("Setting \"{}\" updated", key),
                        "key": key,
                        "value": parsed_value,
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            "list" => {
                let settings = Self::load_settings(&settings_path)?;
                debug!("Config list");
                Ok(ToolResult {
                    data: json!({
                        "settings": settings,
                        "path": settings_path.display().to_string(),
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            _ => Ok(ToolResult {
                data: json!({ "error": format!("Unknown action: {}", action) }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        "Use the Config tool to read and modify runtime settings.\n\n\
Actions:\n\
- \"get\": Retrieve a specific setting by key.\n\
- \"set\": Update a specific setting. The value is stored in `.allthecodes/settings.json`.\n\
- \"list\": List all current settings.\n\n\
Settings are persisted per-project in `.allthecodes/settings.json`."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "Config".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_tool_name() {
        let tool = ConfigTool;
        assert_eq!(tool.name(), "Config");
    }

    #[test]
    fn test_project_settings_path_uses_allthecodes_project_file() {
        let dir = tempfile::tempdir().expect("tempdir");

        let path = ConfigTool::project_settings_path_for_cwd(dir.path());

        assert_eq!(path, dir.path().join(".allthecodes").join("settings.json"));
    }

    #[test]
    fn test_config_schema() {
        let tool = ConfigTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("key"));
        assert!(props.contains_key("value"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("action")));
    }

    #[test]
    fn test_config_is_read_only_for_get() {
        let tool = ConfigTool;
        assert!(tool.is_read_only(&json!({ "action": "get" })));
    }

    #[test]
    fn test_config_is_read_only_for_list() {
        let tool = ConfigTool;
        assert!(tool.is_read_only(&json!({ "action": "list" })));
    }

    #[test]
    fn test_config_is_not_read_only_for_set() {
        let tool = ConfigTool;
        assert!(!tool.is_read_only(&json!({ "action": "set" })));
    }

    #[test]
    fn test_config_concurrency_safe_for_reads() {
        let tool = ConfigTool;
        assert!(tool.is_concurrency_safe(&json!({ "action": "get" })));
        assert!(tool.is_concurrency_safe(&json!({ "action": "list" })));
        assert!(!tool.is_concurrency_safe(&json!({ "action": "set" })));
    }
}
