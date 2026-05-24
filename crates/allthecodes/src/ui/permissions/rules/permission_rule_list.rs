//! Permission rule list rendering.

use super::permission_rule_description::render_permission_rule_description;
use super::PermissionRule;

pub fn render_permission_rule_list(rules: &[PermissionRule], selected_index: usize) -> String {
    let mut lines = vec![format!("Permission rules ({})", rules.len())];
    if rules.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(rules.iter().enumerate().map(|(idx, rule)| {
            let marker = if idx == selected_index.min(rules.len().saturating_sub(1)) {
                ">"
            } else {
                " "
            };
            format!("{marker} {}", render_permission_rule_description(rule))
        }));
    }
    lines.join("\n")
}
