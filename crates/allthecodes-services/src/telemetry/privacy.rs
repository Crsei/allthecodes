//! PII redaction for telemetry events.
//!
//! Applies configurable redaction rules to telemetry event data before
//! export or persistence. The `TelemetryRedaction` config controls which
//! categories of data are redacted.

use serde_json::Value;

use super::TelemetryRedaction;

// ---------------------------------------------------------------------------
// Redaction patterns
// ---------------------------------------------------------------------------

/// Patterns that indicate sensitive content in JSON keys.
const SENSITIVE_KEY_PATTERNS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "token",
    "secret",
    "password",
    "credential",
    "auth_header",
    "authorization",
    "bearer",
    "oauth",
];

/// File path patterns that may contain user names.
const FILE_PATH_PATTERNS: &[&str] = &["/home/", "/Users/", "~", "C:\\Users\\"];

// ---------------------------------------------------------------------------
// Redaction entry point
// ---------------------------------------------------------------------------

/// Redact sensitive fields from a telemetry event.
///
/// Modifies the event in place according to the `config` settings.
pub fn redact_for_telemetry(value: &mut Value, config: &TelemetryRedaction) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let key_lower = key.to_lowercase();

                // Always redact known-sensitive keys
                if is_sensitive_key(&key_lower) {
                    if let Some(val) = map.get_mut(&key) {
                        *val = Value::String("[REDACTED]".to_string());
                    }
                    continue;
                }

                // Redact tool inputs if configured
                if config.redact_tool_inputs && key_lower == "input" {
                    if let Some(val) = map.get_mut(&key) {
                        if val.is_object() || val.is_string() {
                            *val = Value::String("[INPUT REDACTED]".to_string());
                        }
                    }
                    continue;
                }

                // Redact tool outputs if configured
                if config.redact_tool_outputs && key_lower == "output" {
                    if let Some(val) = map.get_mut(&key) {
                        if val.is_object() || val.is_string() {
                            *val = Value::String("[OUTPUT REDACTED]".to_string());
                        }
                    }
                    continue;
                }

                // Redact file paths if configured
                if config.redact_file_paths
                    && (key_lower == "path" || key_lower == "file_path" || key_lower == "directory")
                {
                    if let Some(Value::String(path)) = map.get(&key) {
                        if contains_file_path_pattern(path) {
                            if let Some(val) = map.get_mut(&key) {
                                *val = Value::String("[PATH REDACTED]".to_string());
                            }
                        }
                    }
                    continue;
                }

                // Recurse into nested objects
                if let Some(val) = map.get_mut(&key) {
                    redact_for_telemetry(val, config);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_for_telemetry(item, config);
            }
        }
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            if SENSITIVE_KEY_PATTERNS
                .iter()
                .any(|pattern| lower.contains(pattern))
                || (config.redact_environment && lower.contains("env"))
            {
                *text = "[REDACTED]".to_string();
            } else if config.redact_file_paths && contains_file_path_pattern(text) {
                *text = "[PATH REDACTED]".to_string();
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a (lowercased) key matches a sensitive pattern.
fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEY_PATTERNS
        .iter()
        .any(|pattern| key.contains(pattern))
}

/// Check if a string value contains a file path pattern.
fn contains_file_path_pattern(s: &str) -> bool {
    FILE_PATH_PATTERNS.iter().any(|pattern| s.contains(pattern))
}

/// Summarize an input string for telemetry (truncate + sanitize).
pub fn summarize_input(input: &str) -> String {
    let sanitized = input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>();
    if sanitized.chars().count() > 200 {
        format!(
            "{}...[truncated {} chars]",
            sanitized.chars().take(200).collect::<String>(),
            sanitized.chars().count()
        )
    } else {
        sanitized
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_sensitive_keys_in_object() {
        let config = TelemetryRedaction {
            redact_tool_inputs: false,
            redact_tool_outputs: false,
            redact_file_paths: false,
            redact_environment: false,
        };
        let mut value = serde_json::json!({
            "api_key": "sk-ant-12345",
            "tool_name": "Bash",
            "nested": {
                "secret": "sensitive",
                "safe": "visible"
            }
        });

        redact_for_telemetry(&mut value, &config);

        assert_eq!(value["api_key"], "[REDACTED]");
        assert_eq!(value["nested"]["secret"], "[REDACTED]");
        assert_eq!(value["nested"]["safe"], "visible");
        assert_eq!(value["tool_name"], "Bash");
    }

    #[test]
    fn redact_tool_inputs_when_configured() {
        let config = TelemetryRedaction {
            redact_tool_inputs: true,
            ..Default::default()
        };
        let mut value = serde_json::json!({
            "tool_name": "Read",
            "input": {"path": "/home/user/file.txt"}
        });

        redact_for_telemetry(&mut value, &config);
        assert_eq!(value["input"], "[INPUT REDACTED]");
    }

    #[test]
    fn redact_file_paths_when_configured() {
        let config = TelemetryRedaction {
            redact_file_paths: true,
            ..Default::default()
        };
        let mut value = serde_json::json!({
            "path": "/home/user/project/src/main.rs"
        });

        redact_for_telemetry(&mut value, &config);
        assert_eq!(value["path"], "[PATH REDACTED]");
    }

    #[test]
    fn do_not_redact_without_file_path() {
        let config = TelemetryRedaction {
            redact_file_paths: true,
            ..Default::default()
        };
        let mut value = serde_json::json!({
            "path": "/tmp/build/output.txt"
        });

        redact_for_telemetry(&mut value, &config);
        // /tmp is not in the file path patterns
        assert_eq!(value["path"], "/tmp/build/output.txt");
    }

    #[test]
    fn summarize_input_truncates_long_text() {
        let long = "a".repeat(500);
        let summarized = summarize_input(&long);
        assert!(summarized.contains("[truncated"));
        assert!(summarized.chars().count() < 250);
    }

    #[test]
    fn summarize_short_input_preserved() {
        let short = "hello world";
        assert_eq!(summarize_input(short), short);
    }
}
