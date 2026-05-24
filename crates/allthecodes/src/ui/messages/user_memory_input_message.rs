//! Rust-side helper for memory input messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_memory_input_message(key: &str, value: &str, _theme: &Theme) -> String {
    let value = value.trim();
    if key.trim().is_empty() {
        if value.is_empty() {
            "Memory input: <empty>".to_string()
        } else {
            format!("Memory input: value={value}")
        }
    } else {
        format!("Memory input: {key}={value}")
    }
}
