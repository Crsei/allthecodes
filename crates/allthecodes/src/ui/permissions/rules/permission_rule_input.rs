//! Permission rule input validation and rendering.

use crate::ui::permissions::utils::PermissionDecision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuleInputState {
    pub pattern: String,
    pub decision: PermissionDecision,
    pub error: Option<String>,
}

impl PermissionRuleInputState {
    pub fn new(pattern: impl Into<String>, decision: PermissionDecision) -> Self {
        let pattern = pattern.into();
        let error = validate_rule_pattern(&pattern).err();
        Self {
            pattern,
            decision,
            error,
        }
    }
}

pub fn validate_rule_pattern(pattern: &str) -> Result<(), String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        Err("pattern is required".to_string())
    } else if trimmed.contains('\n') {
        Err("pattern must be one line".to_string())
    } else {
        Ok(())
    }
}

pub fn render_permission_rule_input(state: &PermissionRuleInputState) -> String {
    let mut lines = vec![
        format!("pattern: {}", state.pattern),
        format!("decision: {}", state.decision.label()),
    ];
    if let Some(error) = &state.error {
        lines.push(format!("error: {error}"));
    }
    lines.join("\n")
}
