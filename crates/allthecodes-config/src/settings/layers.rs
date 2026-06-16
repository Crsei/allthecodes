use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::source::SettingsSource;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigLayerStatus {
    Applied,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigLayerDisabledReason {
    ProjectNotTrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigLayer {
    pub source: SettingsSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    pub status: ConfigLayerStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<ConfigLayerDisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigLayerEntry {
    pub path: String,
    pub source: SettingsSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    pub raw_value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<ConfigLayerDisabledReason>,
}

pub(crate) fn flatten_layer_entries(
    value: &Value,
    source: SettingsSource,
    source_path: Option<&Path>,
    disabled_reason: Option<ConfigLayerDisabledReason>,
) -> Vec<ConfigLayerEntry> {
    let mut entries = Vec::new();
    flatten_value(
        "",
        value,
        source,
        source_path,
        disabled_reason,
        &mut entries,
    );
    entries
}

pub(crate) fn attach_effective_values(entries: &mut [ConfigLayerEntry], effective: &Value) {
    for entry in entries {
        entry.effective_value =
            get_dotted_value(effective, &entry.path).map(|value| redact_value(&entry.path, value));
    }
}

fn flatten_value(
    prefix: &str,
    value: &Value,
    source: SettingsSource,
    source_path: Option<&Path>,
    disabled_reason: Option<ConfigLayerDisabledReason>,
    entries: &mut Vec<ConfigLayerEntry>,
) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if value.is_null() || key == "configProfiles" {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_value(
                    &path,
                    value,
                    source,
                    source_path,
                    disabled_reason.clone(),
                    entries,
                );
            }
        }
        value => entries.push(ConfigLayerEntry {
            path: prefix.to_string(),
            source,
            source_path: source_path.map(Path::to_path_buf),
            raw_value: redact_value(prefix, value),
            effective_value: None,
            disabled_reason,
        }),
    }
}

pub(crate) fn redact_value(path: &str, value: &Value) -> Value {
    if is_sensitive_path(path) {
        Value::String("[redacted]".to_string())
    } else {
        let mut value = value.clone();
        redact_in_place(path, &mut value);
        value
    }
}

fn redact_in_place(path: &str, value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if is_sensitive_path(&child) {
                    *value = Value::String("[redacted]".to_string());
                } else {
                    redact_in_place(&child, value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_in_place(path, item);
            }
        }
        _ => {}
    }
}

fn get_dotted_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn is_sensitive_path(path: &str) -> bool {
    path.split('.').any(|part| {
        let normalized = part
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        normalized.contains("apikey")
            || normalized.contains("token")
            || normalized.contains("secret")
            || normalized.contains("password")
            || normalized.contains("credential")
    })
}
