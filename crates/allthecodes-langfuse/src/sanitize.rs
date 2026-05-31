use serde::Serialize;
use serde_json::{json, Map, Value};

use allthecodes_types::message::Usage;

const MAX_TEXT_LEN: usize = 40_000;
const MAX_TOOL_OUTPUT_LEN: usize = 500;
const REDACTED_FILE_TOOLS: &[&str] = &["Read", "Write", "Edit", "MultiEdit"];
const REDACTED_SENSITIVE_TOOLS: &[&str] = &["Config", "MCP"];
const REDACTED_SHELL_TOOLS: &[&str] = &["Bash", "PowerShell"];
const PATH_FIELDS: &[&str] = &[
    "file_path",
    "path",
    "directory",
    "output_path",
    "input_path",
];
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

pub fn sanitize_tool_input(tool_name: &str, input: &Value) -> Value {
    if REDACTED_SENSITIVE_TOOLS.contains(&tool_name) {
        return Value::String(format!("[{} input redacted]", tool_name));
    }

    if REDACTED_FILE_TOOLS.contains(&tool_name) {
        return sanitize_file_tool_input(input);
    }

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

pub fn metadata_json(entries: Vec<(&str, Value)>) -> String {
    let mut map = Map::new();
    for (key, value) in entries {
        if !value.is_null() {
            map.insert(key.to_string(), value);
        }
    }
    json!(map).to_string()
}

pub fn generation_observation_metadata(
    usage: Option<&Usage>,
    ttft_ms: Option<u64>,
    error: Option<&str>,
) -> Map<String, Value> {
    let mut metadata = Map::new();

    if let Some(ttft_ms) = ttft_ms {
        metadata.insert("ttftMs".to_string(), json!(ttft_ms));
    }

    if let Some(usage) = usage {
        metadata.insert(
            "cacheReadInputTokens".to_string(),
            json!(usage.cache_read_input_tokens),
        );
        metadata.insert(
            "cacheCreationInputTokens".to_string(),
            json!(usage.cache_creation_input_tokens),
        );
    }

    if let Some(error) = error {
        metadata.insert(
            "error".to_string(),
            Value::String(sanitize_global_string(error)),
        );
    }

    metadata
}

pub fn generation_observation_metadata_json(
    usage: Option<&Usage>,
    ttft_ms: Option<u64>,
    error: Option<&str>,
) -> Option<String> {
    let metadata = generation_observation_metadata(usage, ttft_ms, error);

    if metadata.is_empty() {
        None
    } else {
        Some(json!(metadata).to_string())
    }
}

fn sanitize_file_tool_input(input: &Value) -> Value {
    match input {
        Value::Object(map) => {
            let mut result = Map::new();
            for (key, value) in map {
                let key_lower = key.to_ascii_lowercase();
                if PATH_FIELDS.contains(&key_lower.as_str()) {
                    if let Value::String(path) = value {
                        result.insert(key.clone(), Value::String(replace_home_dir(path)));
                    } else {
                        result.insert(key.clone(), sanitize_global_value(value));
                    }
                } else if is_sensitive_key(&key_lower) {
                    result.insert(key.clone(), Value::String("[REDACTED]".to_string()));
                } else {
                    result.insert(key.clone(), sanitize_global_value(value));
                }
            }
            Value::Object(result)
        }
        other => sanitize_global_value(other),
    }
}

fn sanitize_object(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(key, value)| {
            let key_lower = key.to_ascii_lowercase();
            if is_sensitive_key(&key_lower) {
                (key.clone(), Value::String("[REDACTED]".to_string()))
            } else {
                (key.clone(), sanitize_global_value(value))
            }
        })
        .collect()
}

fn is_sensitive_key(key_lower: &str) -> bool {
    SENSITIVE_KEYWORDS
        .iter()
        .any(|candidate| key_lower.contains(candidate))
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
    fn sanitize_tool_input_redacts_sensitive_tools() {
        let input = json!({"command": "ls"});
        let result = sanitize_tool_input("Config", &input);
        assert_eq!(result, "[Config input redacted]");
    }

    #[test]
    fn sanitize_tool_input_replaces_home_dir_in_path() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let input = json!({"file_path": format!("{}/secret.txt", home.to_string_lossy())});
        let result = sanitize_tool_input("Read", &input);
        assert_eq!(result["file_path"], "~/secret.txt");
    }

    #[test]
    fn sanitize_tool_input_ignores_non_path_fields() {
        let input = json!({"content": "visible", "mode": "write"});
        let result = sanitize_tool_input("Write", &input);
        assert_eq!(result["content"], "visible");
        assert_eq!(result["mode"], "write");
    }

    #[test]
    fn file_tool_output_fully_redacted() {
        let output = "secret content 12345";
        for tool in &["Read", "Write", "Edit", "MultiEdit"] {
            let sanitized = sanitize_tool_output(tool, output);
            assert!(sanitized.contains("file content redacted"), "{}", tool);
            assert!(
                sanitized.contains(&format!("{} chars", output.chars().count())),
                "{} should report char count",
                tool
            );
        }
    }

    #[test]
    fn sensitive_tool_output_fully_redacted() {
        for tool in &["Config", "MCP"] {
            let output = sanitize_tool_output(tool, "anything");
            assert!(
                output.contains(&format!("[{} output redacted", tool)),
                "{}",
                tool
            );
        }
    }

    #[test]
    fn shell_tool_output_truncated() {
        let long = "a".repeat(600);
        let output = sanitize_tool_output("Bash", &long);
        assert!(output.contains("[truncated]"));
        assert!(output.chars().count() <= 500 + "\n[truncated]".len());
    }

    #[test]
    fn other_tool_global_sanitize_only() {
        let long = "a".repeat(41_000);
        let output = sanitize_tool_output("CustomTool", &long);
        assert!(output.contains("[truncated]"));
    }

    #[test]
    fn sensitive_keys_redacted_in_object() {
        let value = json!({"api_key": "sk-xxx", "token": "abc", "safe": "ok"});
        let sanitized = sanitize_global_value(&value);
        assert_eq!(sanitized["api_key"], "[REDACTED]");
        assert_eq!(sanitized["token"], "[REDACTED]");
        assert_eq!(sanitized["safe"], "ok");
    }

    #[test]
    fn home_dir_replaced() {
        if let Some(home) = dirs::home_dir() {
            let s = format!("{}/.config/file", home.to_string_lossy());
            assert_eq!(replace_home_dir(&s), "~/.config/file");
        }
    }

    #[test]
    fn output_under_max_len_not_truncated() {
        let s = "a".repeat(39_000);
        assert_eq!(sanitize_global_string(&s).len(), 39_000);
    }

    #[test]
    fn unicode_char_count() {
        let output = sanitize_tool_output("Read", "你好世界");
        let expected_chars = "你好世界".chars().count();
        assert_eq!(
            output,
            format!("[file content redacted, {} chars]", expected_chars)
        );
    }

    #[test]
    fn generation_metadata_merges_usage_ttft_and_error() {
        let usage = Usage {
            cache_read_input_tokens: 11,
            cache_creation_input_tokens: 7,
            ..Usage::default()
        };

        let metadata = generation_observation_metadata_json(
            Some(&usage),
            Some(123),
            Some("failed with token abc"),
        )
        .expect("metadata should be present");
        let metadata: Value = serde_json::from_str(&metadata).expect("valid metadata json");

        assert_eq!(metadata["ttftMs"], 123);
        assert_eq!(metadata["cacheReadInputTokens"], 11);
        assert_eq!(metadata["cacheCreationInputTokens"], 7);
        assert_eq!(metadata["error"], "failed with token abc");
    }

    #[test]
    fn generation_metadata_absent_when_empty() {
        assert!(generation_observation_metadata_json(None, None, None).is_none());
    }
}
