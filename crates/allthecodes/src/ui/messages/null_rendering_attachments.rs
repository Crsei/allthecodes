//! Rust-side helper for null-rendering attachment summaries.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_null_rendering_attachments(reason: &str, _theme: &Theme) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "No attachments to render".to_string()
    } else {
        format!("Skipped attachments: {reason}")
    }
}
