//! Permission rule explanation rendering.

use super::utils::{PermissionDecision, PermissionScope};

pub fn render_permission_rule_explanation(
    pattern: &str,
    decision: PermissionDecision,
    scope: PermissionScope,
) -> String {
    format!(
        "Rule: {} {} in {} scope",
        decision.label(),
        pattern.trim(),
        scope.label()
    )
}
