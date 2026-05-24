//! Rust-side helper for structured user command messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_command_message(command: &str, cwd: Option<&str>, _theme: &Theme) -> String {
    let cwd = cwd.unwrap_or("<unknown>");
    if command.trim().is_empty() {
        format!("Command ({cwd}): <empty>")
    } else {
        format!("Command ({cwd}): {command}")
    }
}
