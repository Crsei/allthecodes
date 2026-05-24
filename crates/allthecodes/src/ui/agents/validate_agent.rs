//! Validation helpers for editable agent definitions.

use super::types::AgentValidationResult;

pub fn validate_agent_definition(
    agent_type: &str,
    when_to_use: &str,
    system_prompt: &str,
    existing_agent_types: &[String],
) -> AgentValidationResult {
    let mut result = AgentValidationResult::ok();

    let name = agent_type.trim();
    if name.is_empty() {
        result = result.with_error("Agent name is required");
    } else {
        if name.len() > 64 {
            result = result.with_error("Agent name must be 64 characters or fewer");
        }
        if !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            result = result.with_error("Agent name may contain only letters, numbers, '-' and '_'");
        }
        if existing_agent_types
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            result = result.with_error("Agent name already exists");
        }
    }

    if when_to_use.trim().is_empty() {
        result = result.with_error("Description is required");
    } else if when_to_use.trim().len() < 12 {
        result = result.with_warning("Description is very short");
    }

    if system_prompt.trim().is_empty() {
        result = result.with_error("System prompt is required");
    } else if system_prompt.trim().len() < 40 {
        result = result.with_warning("System prompt may be too short for reliable delegation");
    }

    result
}

pub fn render_validation_result(result: &AgentValidationResult) -> String {
    let mut lines = vec![format!(
        "status: {}",
        if result.is_valid { "valid" } else { "invalid" }
    )];
    for error in &result.errors {
        lines.push(format!("error: {error}"));
    }
    for warning in &result.warnings {
        lines.push(format!("warning: {warning}"));
    }
    lines.join("\n")
}
