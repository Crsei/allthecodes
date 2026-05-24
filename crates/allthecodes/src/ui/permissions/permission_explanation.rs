//! Permission explanation rendering.

use super::utils::render_bullets;

pub fn render_permission_explanation(reason: &str, consequences: &[impl AsRef<str>]) -> String {
    let mut lines = vec![format!("Why approval is needed: {reason}")];
    if !consequences.is_empty() {
        lines.push(render_bullets("Effects", consequences));
    }
    lines.join("\n")
}
