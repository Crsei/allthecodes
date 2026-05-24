//! Permission decision debug info rendering.

use super::utils::render_key_values;

pub fn render_permission_decision_debug_info(
    tool_name: &str,
    matcher: &str,
    source: &str,
    matched_rule: Option<&str>,
) -> String {
    let matched = matched_rule.unwrap_or("<none>");
    let rows = [
        ("tool", tool_name),
        ("matcher", matcher),
        ("source", source),
        ("matched rule", matched),
    ];
    render_key_values("Permission decision", &rows)
}
