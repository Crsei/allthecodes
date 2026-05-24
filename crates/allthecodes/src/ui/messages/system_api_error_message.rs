//! Rust-side helper for API error messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_system_api_error_message(status: u16, detail: &str, _theme: &Theme) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        format!("API error {status}")
    } else {
        format!("API error {status}: {detail}")
    }
}
