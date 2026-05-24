//! Rust-side helper for shutdown messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_shutdown_message(reason: &str, _theme: &Theme) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "Session shutting down".to_string()
    } else {
        format!("Session shutting down: {reason}")
    }
}
