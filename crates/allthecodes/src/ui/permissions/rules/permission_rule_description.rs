//! Permission rule description rendering.

use super::PermissionRule;

pub fn render_permission_rule_description(rule: &PermissionRule) -> String {
    format!(
        "{} {} ({}, source: {})",
        rule.decision.label(),
        rule.pattern,
        rule.scope.label(),
        rule.source
    )
}
