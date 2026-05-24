//! Rust-side helper for explicit user prompt messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_prompt_message(prompt: &str, _theme: &Theme) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        "Prompt: <empty>".to_string()
    } else {
        format!("Prompt: {prompt}")
    }
}
