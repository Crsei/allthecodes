//! JSON body manipulation for cache control and provider compatibility.

use serde_json::Value;

use super::types::{PromptCacheCapability, PromptCachePolicy};

// ---------------------------------------------------------------------------
// Cache-field stripping
// ---------------------------------------------------------------------------

/// Recursively remove all Anthropic-specific cache fields from a JSON value.
pub(crate) fn strip_anthropic_cache_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("cache_control");
            map.remove("cache_reference");
            map.remove("cache_edits");
            for value in map.values_mut() {
                strip_anthropic_cache_fields(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_anthropic_cache_fields(value);
            }
        }
        _ => {}
    }
}

/// Strip everything that is Anthropic-only (cache fields, thinking blocks,
/// output_config, context_management) so the body is safe for
/// non-Anthropic providers.
pub(crate) fn strip_anthropic_compatible_only_fields(value: &mut Value) {
    strip_anthropic_cache_fields(value);
    if let Value::Object(map) = value {
        map.remove("thinking");
        map.remove("output_config");
        map.remove("context_management");
    }
    strip_anthropic_thinking_blocks(value);
}

/// Remove `thinking` and `redacted_thinking` blocks from a content array.
pub(super) fn strip_anthropic_thinking_blocks(value: &mut Value) {
    match value {
        Value::Array(values) => {
            values.retain(|item| {
                item.get("type")
                    .and_then(Value::as_str)
                    .map(|kind| kind != "thinking" && kind != "redacted_thinking")
                    .unwrap_or(true)
            });
            for value in values {
                strip_anthropic_thinking_blocks(value);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                strip_anthropic_thinking_blocks(value);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Prompt cache policy application
// ---------------------------------------------------------------------------

/// Apply the environment-configured prompt cache policy to a request body.
pub(crate) fn apply_prompt_cache_policy_to_body(
    body: &mut Value,
    capability: PromptCacheCapability,
) {
    let policy = PromptCachePolicy::from_env(capability);
    if !policy.enabled {
        return;
    }
    let cache_control = prompt_cache_marker_value(policy);
    replace_cache_control_markers(body, &cache_control);
}

pub fn prompt_cache_marker_value(policy: PromptCachePolicy) -> Value {
    serde_json::to_value(policy.cache_control()).unwrap_or_else(|_| {
        serde_json::json!({
            "type": "ephemeral"
        })
    })
}

/// Replace every `cache_control` marker object in the body with the
/// fully-resolved cache control value.
pub(super) fn replace_cache_control_markers(value: &mut Value, cache_control: &Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("cache_control") {
                map.insert("cache_control".to_string(), cache_control.clone());
            }
            for value in map.values_mut() {
                replace_cache_control_markers(value, cache_control);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_cache_control_markers(value, cache_control);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

/// Return `true` when `base_url` points to the official Anthropic API
/// (`api.anthropic.com`).
pub(crate) fn is_official_anthropic_base_url(base_url: &str) -> bool {
    crate::api::providers::base_url_host(base_url)
        .map(|host| host.eq_ignore_ascii_case("api.anthropic.com"))
        .unwrap_or(false)
}
