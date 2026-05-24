//! Recent permission denials rendering.

use super::RecentDenial;

pub fn render_recent_denials_tab(denials: &[RecentDenial]) -> String {
    let mut lines = vec![format!("Recent denials ({})", denials.len())];
    if denials.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(denials.iter().map(|denial| {
            format!(
                "- {} matched {}: {}",
                denial.tool_name, denial.pattern, denial.reason
            )
        }));
    }
    lines.join("\n")
}
