//! Rust-side helper for rate limit messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_rate_limit_message(feature: &str, retry_after_ms: u64, _theme: &Theme) -> String {
    let reason = if feature.trim().is_empty() {
        "request".to_string()
    } else {
        feature.to_string()
    };
    format!("Rate limit hit on {reason}; retry in {retry_after_ms}ms")
}
