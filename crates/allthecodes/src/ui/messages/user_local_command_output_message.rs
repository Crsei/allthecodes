//! Rust-side helper for local command output messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_local_command_output_message(
    command: &str,
    exit_code: i32,
    output: &str,
    _theme: &Theme,
) -> String {
    let output = output.trim();
    let body = if output.is_empty() {
        "(no output)"
    } else {
        output
    };
    format!("Local command [{exit_code}]: {command} -> {body}")
}
