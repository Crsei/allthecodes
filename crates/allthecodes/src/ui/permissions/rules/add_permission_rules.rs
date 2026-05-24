//! Add permission rules rendering.

use super::permission_rule_input::{render_permission_rule_input, PermissionRuleInputState};

pub fn render_add_permission_rules(inputs: &[PermissionRuleInputState]) -> String {
    let mut lines = vec![format!("Add permission rules ({})", inputs.len())];
    if inputs.is_empty() {
        lines.push("  <empty>".to_string());
    } else {
        for (idx, input) in inputs.iter().enumerate() {
            lines.push(format!("rule {}", idx + 1));
            lines.extend(
                render_permission_rule_input(input)
                    .lines()
                    .map(|line| format!("  {line}")),
            );
        }
    }
    lines.join("\n")
}
