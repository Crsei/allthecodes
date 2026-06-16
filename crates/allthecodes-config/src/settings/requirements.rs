use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::effective::EffectiveSettings;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequirementRule {
    #[serde(default, rename = "allowedValues", alias = "allowed_values")]
    pub allowed_values: Vec<Value>,
    #[serde(default, rename = "deniedValues", alias = "denied_values")]
    pub denied_values: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequirementViolation {
    pub path: String,
    pub value: Value,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequirementsDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    pub violations: Vec<RequirementViolation>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequirementsFile {
    Wrapped {
        requirements: HashMap<String, RequirementRule>,
    },
    Direct(HashMap<String, RequirementRule>),
}

pub(crate) fn load_requirements() -> Result<(Option<PathBuf>, HashMap<String, RequirementRule>)> {
    let path = requirements_path();
    if !path.exists() {
        return Ok((None, HashMap::new()));
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let file = match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => {
            let toml_value: toml::Value = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            let json_value = serde_json::to_value(toml_value)
                .with_context(|| format!("Failed to convert {}", path.display()))?;
            serde_json::from_value::<RequirementsFile>(json_value)
                .with_context(|| format!("Failed to decode {}", path.display()))?
        }
        Some("json") | None => serde_json::from_str::<RequirementsFile>(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?,
        Some(other) => bail!("unsupported requirements file extension: {other}"),
    };

    let rules = match file {
        RequirementsFile::Wrapped { requirements } => requirements,
        RequirementsFile::Direct(rules) => rules,
    };
    Ok((Some(path), rules))
}

pub(crate) fn evaluate_requirements(
    effective: &EffectiveSettings,
    rules: &HashMap<String, RequirementRule>,
) -> Vec<RequirementViolation> {
    let mut violations = Vec::new();
    for (path, rule) in rules {
        let Some(value) = setting_value(effective, path) else {
            continue;
        };
        if !rule.allowed_values.is_empty() && !rule.allowed_values.iter().any(|v| v == &value) {
            violations.push(RequirementViolation {
                path: path.clone(),
                value: value.clone(),
                message: format!("{}={} is not in allowedValues", path, display_value(&value)),
            });
        }
        if rule.denied_values.iter().any(|v| v == &value) {
            violations.push(RequirementViolation {
                path: path.clone(),
                value: value.clone(),
                message: format!("{}={} is denied", path, display_value(&value)),
            });
        }
    }
    violations
}

pub(crate) fn requirements_path() -> PathBuf {
    if let Ok(path) = std::env::var("ALLTHECODES_REQUIREMENTS") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    default_requirements_path()
}

fn default_requirements_path() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        base.join("allthecodes").join("requirements.toml")
    }
    #[cfg(not(windows))]
    {
        Path::new("/etc")
            .join("allthecodes")
            .join("requirements.toml")
    }
}

fn setting_value(effective: &EffectiveSettings, path: &str) -> Option<Value> {
    match path {
        "permissions.defaultMode" => effective
            .permissions
            .default_mode
            .as_ref()
            .or(effective.permission_mode.as_ref())
            .map(|value| Value::String(value.clone())),
        "sandbox.mode" => effective
            .sandbox
            .mode
            .as_ref()
            .map(|value| Value::String(value.clone())),
        "apiProvider" => effective
            .api_provider
            .as_ref()
            .map(|value| Value::String(value.clone())),
        "backend" => effective
            .backend
            .as_ref()
            .map(|value| Value::String(value.clone())),
        _ => None,
    }
}

fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}
