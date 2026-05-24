#[cfg(feature = "telemetry")]
use serde::Serialize;
#[cfg(any(test, feature = "telemetry"))]
use serde_json::json;
use serde_json::{Map, Value};

const MAX_TEXT_LEN: usize = 40_000;
const MAX_TOOL_OUTPUT_LEN: usize = 500;
const REDACTED_FILE_TOOLS: &[&str] = &["Read", "Write", "Edit", "MultiEdit"];
const REDACTED_SENSITIVE_TOOLS: &[&str] = &["Config", "MCP"];
const REDACTED_SHELL_TOOLS: &[&str] = &["Bash", "PowerShell"];
const SENSITIVE_KEYWORDS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "token",
    "secret",
    "password",
    "credential",
    "auth_header",
    "authorization",
];

/// Only consumed by the OpenTelemetry-backed tracing path; the compiler
/// would otherwise flag it as dead code for default builds.
#[cfg(feature = "telemetry")]
pub fn sanitize_global<T: Serialize>(value: &T) -> Value {
    match serde_json::to_value(value) {
        Ok(value) => sanitize_global_value(&value),
        Err(_) => Value::Null,
    }
}

pub fn sanitize_global_value(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        Value::Number(value) => Value::Number(value.clone()),
        Value::String(value) => Value::String(sanitize_global_string(value)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(sanitize_global_value)
                .collect::<Vec<Value>>(),
        ),
        Value::Object(map) => Value::Object(sanitize_object(map)),
    }
}

pub fn sanitize_global_string(value: &str) -> String {
    truncate_string(&replace_home_dir(value), MAX_TEXT_LEN)
}

#[cfg(feature = "telemetry")]
pub fn sanitize_tool_input(tool_name: &str, input: &Value) -> Value {
    let _ = tool_name;
    sanitize_global_value(input)
}

pub fn sanitize_tool_output(tool_name: &str, output: &str) -> String {
    if REDACTED_FILE_TOOLS.contains(&tool_name) {
        return format!("[file content redacted, {} chars]", output.chars().count());
    }

    if REDACTED_SENSITIVE_TOOLS.contains(&tool_name) {
        return format!(
            "[{} output redacted, {} chars]",
            tool_name,
            output.chars().count()
        );
    }

    let sanitized = sanitize_global_string(output);
    if REDACTED_SHELL_TOOLS.contains(&tool_name) {
        return truncate_string(&sanitized, MAX_TOOL_OUTPUT_LEN);
    }

    sanitized
}

pub fn serialize_sanitized_value(value: &Value) -> String {
    match value {
        Value::String(value) => sanitize_global_string(value),
        other => {
            let serialized = serde_json::to_string(other).unwrap_or_else(|_| "null".to_string());
            truncate_string(&serialized, MAX_TEXT_LEN)
        }
    }
}

#[cfg(feature = "telemetry")]
pub fn metadata_json(entries: Vec<(&str, Value)>) -> String {
    let mut map = Map::new();
    for (key, value) in entries {
        if !value.is_null() {
            map.insert(key.to_string(), value);
        }
    }
    json!(map).to_string()
}

fn sanitize_object(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| {
            let key_lower = key.to_ascii_lowercase();
            if SENSITIVE_KEYWORDS
                .iter()
                .any(|candidate| key_lower.contains(candidate))
            {
                (key.clone(), Value::String("[REDACTED]".to_string()))
            } else {
                (key.clone(), sanitize_global_value(value))
            }
        })
        .collect()
}

fn replace_home_dir(value: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return value.to_string();
    };

    let home = home.to_string_lossy();
    if home.is_empty() {
        return value.to_string();
    }

    value.replace(home.as_ref(), "~")
}

fn truncate_string(value: &str, max_len: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_len).collect();
    if chars.next().is_some() {
        format!("{}\n[truncated]", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_global_redacts_sensitive_keys() {
        let value = json!({
            "token": "abc",
            "nested": {
                "api_key": "secret",
                "safe": "visible",
            }
        });

        let sanitized = sanitize_global_value(&value);
        assert_eq!(sanitized["token"], "[REDACTED]");
        assert_eq!(sanitized["nested"]["api_key"], "[REDACTED]");
        assert_eq!(sanitized["nested"]["safe"], "visible");
    }

    #[test]
    fn sanitize_tool_output_redacts_file_tools() {
        let output = sanitize_tool_output("Read", "secret file body");
        assert!(output.contains("file content redacted"));
    }

    #[test]
    fn sanitize_tool_output_truncates_shell_tools() {
        let output = sanitize_tool_output("Bash", &"a".repeat(600));
        assert!(output.contains("[truncated]"));
    }
}
