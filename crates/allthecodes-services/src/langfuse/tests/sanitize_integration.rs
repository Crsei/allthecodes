use serde_json::{json, Value};

use allthecodes_types::message::Usage;

use crate::langfuse::sanitize::{
    generation_observation_metadata_json, sanitize_global_value, sanitize_tool_input,
    sanitize_tool_output,
};

#[test]
fn sensitive_tool_inputs_are_redacted_by_tool_name() {
    let input = json!({"token": "sk-secret", "command": "show"});

    assert_eq!(
        sanitize_tool_input("Config", &input),
        "[Config input redacted]"
    );
    assert_eq!(sanitize_tool_input("MCP", &input), "[MCP input redacted]");
}

#[test]
fn file_tool_inputs_keep_paths_but_redact_sensitive_fields() {
    let home = dirs::home_dir().expect("home dir available");
    let input = json!({
        "file_path": format!("{}/project/secret.txt", home.to_string_lossy()),
        "content": "visible text",
        "api_key": "sk-secret",
    });

    let sanitized = sanitize_tool_input("Write", &input);

    assert_eq!(sanitized["file_path"], "~/project/secret.txt");
    assert_eq!(sanitized["content"], "visible text");
    assert_eq!(sanitized["api_key"], "[REDACTED]");
}

#[test]
fn file_tool_path_field_names_are_case_insensitive() {
    let home = dirs::home_dir().expect("home dir available");
    let input = json!({
        "Directory": format!("{}/workspace", home.to_string_lossy()),
        "output_path": format!("{}/out.json", home.to_string_lossy()),
    });

    let sanitized = sanitize_tool_input("Read", &input);

    assert_eq!(sanitized["Directory"], "~/workspace");
    assert_eq!(sanitized["output_path"], "~/out.json");
}

#[test]
fn shell_tool_outputs_are_sanitized_and_truncated() {
    let home = dirs::home_dir().expect("home dir available");
    let output = format!("{}/secret.log\n{}", home.to_string_lossy(), "a".repeat(600));
    let sanitized = sanitize_tool_output("Bash", &output);

    assert!(sanitized.contains("[truncated]"));
    assert!(sanitized.contains("~/secret.log"));
    assert!(!sanitized.contains(home.to_string_lossy().as_ref()));
}

#[test]
fn file_tool_outputs_hide_file_contents_and_report_chars() {
    let sanitized = sanitize_tool_output("Read", "你好世界");

    assert_eq!(sanitized, "[file content redacted, 4 chars]");
}

#[test]
fn sensitive_tool_outputs_hide_contents_and_report_chars() {
    let sanitized = sanitize_tool_output("Config", "secret");

    assert_eq!(sanitized, "[Config output redacted, 6 chars]");
}

#[test]
fn global_sanitizer_redacts_nested_sensitive_keys() {
    let value = json!({
        "safe": "visible",
        "nested": {
            "authorization": "Bearer token",
            "password": "secret",
        },
    });

    let sanitized = sanitize_global_value(&value);

    assert_eq!(sanitized["safe"], "visible");
    assert_eq!(sanitized["nested"]["authorization"], "[REDACTED]");
    assert_eq!(sanitized["nested"]["password"], "[REDACTED]");
}

#[test]
fn generation_metadata_preserves_cache_tokens_when_error_is_present() {
    let usage = Usage {
        cache_read_input_tokens: 12,
        cache_creation_input_tokens: 34,
        ..Usage::default()
    };

    let metadata = generation_observation_metadata_json(
        Some(&usage),
        Some(56),
        Some("failed with /home/user/token"),
    )
    .expect("metadata should be present");
    let metadata: Value = serde_json::from_str(&metadata).expect("valid metadata json");

    assert_eq!(metadata["ttftMs"], 56);
    assert_eq!(metadata["cacheReadInputTokens"], 12);
    assert_eq!(metadata["cacheCreationInputTokens"], 34);
    assert!(metadata["error"].as_str().is_some());
}
