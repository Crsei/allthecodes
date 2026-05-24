//! Shared utilities for hook execution.
//!
//! Port of TypeScript `hookHelpers.ts`.

use regex::Regex;
use serde_json::Value;

use allthecodes_types::hooks::HookEntry;

/// Perform argument substitution in a prompt string.
///
/// Replaces `$ARGUMENTS` with the JSON input, and also supports
/// indexed access like `$ARGUMENTS[0]`, `$0`, `$1`, etc.
pub fn add_arguments_to_prompt(prompt: &str, json_input: &Value) -> String {
    let json_str = serde_json::to_string(json_input).unwrap_or_default();
    substitute_arguments(prompt, &json_str)
}

/// Substitute $ARGUMENTS placeholders in a string.
fn substitute_arguments(template: &str, json_input: &str) -> String {
    // Replace $ARGUMENTS with the full JSON string
    let s = template.replace("$ARGUMENTS", json_input);

    // Parse the JSON input and extract array elements if it's an array
    if let Ok(parsed) = serde_json::from_str::<Value>(json_input) {
        if let Some(arr) = parsed.as_array() {
            // Pattern: $ARGUMENTS[N]
            let re = Regex::new(r"\$ARGUMENTS\[(\d+)\]").unwrap();
            let s = re.replace_all(&s, |caps: &regex::Captures| {
                let idx: usize = caps[1].parse().unwrap_or(0);
                arr.get(idx)
                    .map(|v| {
                        v.as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| serde_json::to_string(v).unwrap_or_default())
                    })
                    .unwrap_or_default()
            });

            // Shorthand: $0, $1, $2, etc.
            let re2 = Regex::new(r"\$(\d+)").unwrap();
            return re2
                .replace_all(&s, |caps: &regex::Captures| {
                    let idx: usize = caps[1].parse().unwrap_or(0);
                    arr.get(idx)
                        .map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| serde_json::to_string(v).unwrap_or_default())
                        })
                        .unwrap_or_default()
                })
                .to_string();
        }
    }

    s
}

/// Get the display text for a hook entry.
pub fn get_hook_display_text(hook: &HookEntry) -> String {
    match hook {
        HookEntry::Command { command, .. } => command.clone(),
        HookEntry::Prompt { prompt, .. } => prompt.clone(),
        HookEntry::Agent { prompt, .. } => prompt.clone(),
        HookEntry::Http { url, .. } => url.clone(),
    }
}

/// Hook helpers utility struct.
pub struct HookHelpers;

impl HookHelpers {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_arguments_to_prompt_replaces_arguments() {
        let prompt = "Check: $ARGUMENTS";
        let json = serde_json::json!({"key": "value"});
        let result = add_arguments_to_prompt(prompt, &json);
        assert!(result.contains(r#""key":"value""#));
    }

    #[test]
    fn test_get_display_text_for_command() {
        let hook = HookEntry::Command {
            command: "echo hello".into(),
            timeout: 10,
            shell: None,
            if_condition: None,
        };
        assert_eq!(get_hook_display_text(&hook), "echo hello");
    }

    #[test]
    fn test_get_display_text_for_http() {
        let hook = HookEntry::Http {
            url: "https://example.com/hook".into(),
            timeout: 600,
            headers: None,
            if_condition: None,
            allowed_env_vars: None,
        };
        assert_eq!(get_hook_display_text(&hook), "https://example.com/hook");
    }
}
