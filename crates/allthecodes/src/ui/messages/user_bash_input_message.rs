//! Rust-side helper for bash input messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_bash_input_message(command: &str, _theme: &Theme) -> String {
    if command.trim().is_empty() {
        "You ran a bash command (empty)".to_string()
    } else {
        format!("> {command}")
    }
}
