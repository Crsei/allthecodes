//! Rust-side helper for upstream advisor messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_advisor_message(
    model: Option<&str>,
    prompt: &str,
    advisory: &str,
    _theme: &Theme,
) -> String {
    let model_part = model.unwrap_or("default");
    let mut lines = vec![
        format!("Advisor[{model_part}] Prompt: {prompt}"),
        format!("Advisory: {advisory}"),
    ];
    if advisory.trim().is_empty() {
        lines.push("No advisory text".to_string());
    }
    lines.join("\n")
}
