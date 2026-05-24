//! Plugin configuration schema and user-configurable settings.
//!
//! Plugins can declare a JSON Schema describing their user-configurable
//! properties. This module provides parsing and validation of that schema.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A parsed configuration schema from a plugin manifest's `configuration` field.
///
/// This is a JSON Schema (draft-07) subset that describes the shape of
/// user-provided configuration values for a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSchema {
    /// JSON Schema type (typically "object").
    #[serde(default)]
    pub schema_type: String,

    /// Property definitions keyed by property name.
    #[serde(default)]
    pub properties: Vec<ConfigProperty>,

    /// Required property names.
    #[serde(default)]
    pub required: Vec<String>,

    /// Raw JSON Schema for passthrough.
    #[serde(skip)]
    pub raw: Option<Value>,
}

/// A single configuration property in a plugin's config schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigProperty {
    /// Property name.
    pub name: String,

    /// JSON Schema type (string, number, boolean, etc.).
    #[serde(default)]
    pub value_type: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// Default value, if any.
    #[serde(default)]
    pub default: Option<Value>,

    /// Whether this property is required.
    #[serde(default)]
    pub required: bool,

    /// Enum values, if constrained.
    #[serde(default)]
    pub enum_values: Vec<Value>,

    /// Minimum value (for numeric types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    /// Maximum value (for numeric types).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,

    /// Pattern (for string types).
    #[serde(default)]
    pub pattern: Option<String>,
}

/// Extract a configuration schema from a plugin manifest's `configuration` field.
///
/// The `configuration` field is expected to be a JSON Schema object.
/// Returns a parsed `ConfigSchema` or a default empty schema.
pub fn get_plugin_config_schema(config: Option<&Value>) -> ConfigSchema {
    let Some(config) = config else {
        return ConfigSchema::default();
    };

    let schema_type = config
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("object")
        .to_string();

    let required: Vec<String> = config
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let properties = config
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(name, prop)| ConfigProperty {
                    name: name.clone(),
                    value_type: prop
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("string")
                        .to_string(),
                    description: prop
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    default: prop.get("default").cloned(),
                    required: required.contains(name),
                    enum_values: prop
                        .get("enum")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    minimum: prop.get("minimum").and_then(|v| v.as_f64()),
                    maximum: prop.get("maximum").and_then(|v| v.as_f64()),
                    pattern: prop
                        .get("pattern")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                })
                .collect()
        })
        .unwrap_or_default();

    ConfigSchema {
        schema_type,
        properties,
        required,
        raw: Some(config.clone()),
    }
}

impl Default for ConfigSchema {
    fn default() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: Vec::new(),
            required: Vec::new(),
            raw: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_empty_config_returns_default() {
        let schema = get_plugin_config_schema(None);
        assert_eq!(schema.schema_type, "object");
        assert!(schema.properties.is_empty());
        assert!(schema.raw.is_none());
    }

    #[test]
    fn test_parse_simple_config() {
        let config = json!({
            "type": "object",
            "properties": {
                "apiKey": {
                    "type": "string",
                    "description": "API key for the service",
                    "default": ""
                },
                "timeout": {
                    "type": "number",
                    "description": "Request timeout in ms",
                    "default": 5000,
                    "minimum": 100,
                    "maximum": 60000
                },
                "region": {
                    "type": "string",
                    "description": "Deployment region",
                    "enum": ["us-east", "us-west", "eu-west"]
                }
            },
            "required": ["apiKey"]
        });

        let schema = get_plugin_config_schema(Some(&config));
        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.properties.len(), 3);
        assert_eq!(schema.required, vec!["apiKey"]);

        let find_prop = |name: &str| schema.properties.iter().find(|p| p.name == name).unwrap();

        let api_key = find_prop("apiKey");
        assert_eq!(api_key.value_type, "string");
        assert_eq!(api_key.default, Some(json!("")));
        assert!(api_key.required);

        let timeout = find_prop("timeout");
        assert_eq!(timeout.minimum, Some(100.0));
        assert_eq!(timeout.maximum, Some(60000.0));

        let region = find_prop("region");
        assert_eq!(region.enum_values.len(), 3);
    }

    #[test]
    fn test_parse_empty_properties() {
        let config = json!({"type": "object", "properties": {}});
        let schema = get_plugin_config_schema(Some(&config));
        assert!(schema.properties.is_empty());
    }

    #[test]
    fn test_raw_json_preserved() {
        let config = json!({"type": "object", "properties": {}});
        let schema = get_plugin_config_schema(Some(&config));
        assert!(schema.raw.is_some());
    }
}
