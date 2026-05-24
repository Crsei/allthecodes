//! Effort level to thinking budget tokens mapping.
//!
//! Used by `build_messages_request` to translate a user-facing effort
//! label or numeric override into the `thinking.budget_tokens` value
//! sent to the model API.

/// Default budget when thinking is enabled but no explicit effort level is set.
pub const DEFAULT_THINKING_BUDGET: u32 = 10_240;

/// Highest fixed budget supported by the budget-based thinking path.
pub const MAX_THINKING_BUDGET: u32 = 32_768;

/// Normalize a user-provided effort value.
///
/// Accepts:
///   - `"low"` / `"medium"` / `"high"` / `"xhigh"` / `"auto"` / `"max"` (case-insensitive)
///   - `"med"` as a shorthand for `"medium"`
///   - A positive numeric string used directly as the budget
pub fn normalize_effort_value(effort: &str) -> Option<String> {
    let trimmed = effort.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(n) = trimmed.parse::<u32>() {
        if n == 0 {
            return None;
        }
        return Some(n.to_string());
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "low" => Some("low".to_string()),
        "medium" | "med" => Some("medium".to_string()),
        "high" => Some("high".to_string()),
        "xhigh" => Some("xhigh".to_string()),
        "auto" => Some("auto".to_string()),
        "max" => Some("max".to_string()),
        _ => None,
    }
}

/// Normalize a value that can be sent as `output_config.effort`.
///
/// Claude's `output_config.effort` path is intentionally narrower than the
/// token-budget path. For compatibility with existing UI/settings values,
/// low/medium collapse to high, xhigh collapses to max, and any other non-empty
/// value is treated as max.
pub fn normalize_output_effort_value(effort: &str) -> Option<String> {
    let trimmed = effort.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "low" | "medium" | "med" | "high" => Some("high".to_string()),
        "xhigh" | "max" => Some("max".to_string()),
        _ => Some("max".to_string()),
    }
}

/// Normalize a JSON settings value into `output_config.effort`.
pub fn normalize_output_effort_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => normalize_output_effort_value(s),
        serde_json::Value::Null => None,
        _ => Some("max".to_string()),
    }
}

/// Map an effort label to a `thinking.budget_tokens` value.
///
/// `auto` resets to the model default budget used by this fork.
/// `max` selects the highest fixed budget supported by this budget-based path.
pub fn effort_to_budget_tokens(effort: &str) -> Option<u32> {
    let normalized = normalize_effort_value(effort)?;

    match normalized.as_str() {
        "low" => Some(4_096),
        "medium" => Some(10_240),
        "high" => Some(24_576),
        "xhigh" => Some(MAX_THINKING_BUDGET),
        "auto" => Some(DEFAULT_THINKING_BUDGET),
        "max" => Some(MAX_THINKING_BUDGET),
        numeric => numeric.parse::<u32>().ok(),
    }
}

/// Resolve the effective thinking budget for a request.
///
/// Returns the budget in priority order:
///   1. `effort_to_budget_tokens(effort_value)` if it parses
///   2. `max_output_tokens_fallback` if provided
///   3. [`DEFAULT_THINKING_BUDGET`]
pub fn resolve_thinking_budget(
    effort_value: Option<&str>,
    max_output_tokens_fallback: Option<u32>,
) -> u32 {
    effort_value
        .and_then(effort_to_budget_tokens)
        .or(max_output_tokens_fallback)
        .unwrap_or(DEFAULT_THINKING_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_labels_map_to_budgets() {
        assert_eq!(effort_to_budget_tokens("low"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("medium"), Some(10_240));
        assert_eq!(effort_to_budget_tokens("high"), Some(24_576));
        assert_eq!(effort_to_budget_tokens("xhigh"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn auto_and_max_map_to_supported_budgets() {
        assert_eq!(
            effort_to_budget_tokens("auto"),
            Some(DEFAULT_THINKING_BUDGET)
        );
        assert_eq!(effort_to_budget_tokens("max"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn labels_are_case_insensitive() {
        assert_eq!(effort_to_budget_tokens("LOW"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("Medium"), Some(10_240));
        assert_eq!(effort_to_budget_tokens("HIGH"), Some(24_576));
        assert_eq!(effort_to_budget_tokens("XHIGH"), Some(MAX_THINKING_BUDGET));
        assert_eq!(
            effort_to_budget_tokens("AUTO"),
            Some(DEFAULT_THINKING_BUDGET)
        );
        assert_eq!(effort_to_budget_tokens("MAX"), Some(MAX_THINKING_BUDGET));
    }

    #[test]
    fn numeric_override_returned_directly() {
        assert_eq!(effort_to_budget_tokens("4096"), Some(4_096));
        assert_eq!(effort_to_budget_tokens("32000"), Some(32_000));
        assert_eq!(effort_to_budget_tokens(" 8000 "), Some(8_000));
    }

    #[test]
    fn zero_and_empty_return_none() {
        assert_eq!(effort_to_budget_tokens(""), None);
        assert_eq!(effort_to_budget_tokens("   "), None);
        assert_eq!(effort_to_budget_tokens("0"), None);
    }

    #[test]
    fn unknown_labels_return_none() {
        assert_eq!(effort_to_budget_tokens("ultra"), None);
        assert_eq!(effort_to_budget_tokens("not-a-number-or-label"), None);
    }

    #[test]
    fn normalize_effort_value_canonicalizes_labels() {
        assert_eq!(normalize_effort_value("med"), Some("medium".to_string()));
        assert_eq!(normalize_effort_value("XHIGH"), Some("xhigh".to_string()));
        assert_eq!(normalize_effort_value(" AUTO "), Some("auto".to_string()));
        assert_eq!(normalize_effort_value(" 12000 "), Some("12000".to_string()));
    }

    #[test]
    fn output_effort_maps_to_claude_supported_values() {
        assert_eq!(
            normalize_output_effort_value("med"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("low"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("medium"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("high"),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("xhigh"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("max"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("12000"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("auto"),
            Some("max".to_string())
        );
        assert_eq!(
            normalize_output_effort_value("ultra"),
            Some("max".to_string())
        );
        assert_eq!(normalize_output_effort_value("   "), None);
    }

    #[test]
    fn output_effort_json_maps_non_string_values_to_max() {
        assert_eq!(
            normalize_output_effort_json(&serde_json::json!("low")),
            Some("high".to_string())
        );
        assert_eq!(
            normalize_output_effort_json(&serde_json::json!(12000)),
            Some("max".to_string())
        );
        assert_eq!(normalize_output_effort_json(&serde_json::Value::Null), None);
    }

    #[test]
    fn resolve_prefers_effort_label() {
        assert_eq!(resolve_thinking_budget(Some("high"), Some(99_999)), 24_576);
    }

    #[test]
    fn resolve_supports_auto_and_max() {
        assert_eq!(
            resolve_thinking_budget(Some("auto"), Some(99_999)),
            DEFAULT_THINKING_BUDGET
        );
        assert_eq!(
            resolve_thinking_budget(Some("max"), Some(99_999)),
            MAX_THINKING_BUDGET
        );
    }

    #[test]
    fn resolve_falls_back_to_max_tokens() {
        assert_eq!(resolve_thinking_budget(None, Some(8_000)), 8_000);
        assert_eq!(resolve_thinking_budget(Some("ultra"), Some(8_000)), 8_000);
    }

    #[test]
    fn resolve_falls_back_to_default() {
        assert_eq!(resolve_thinking_budget(None, None), DEFAULT_THINKING_BUDGET);
        assert_eq!(
            resolve_thinking_budget(Some(""), None),
            DEFAULT_THINKING_BUDGET
        );
    }
}
