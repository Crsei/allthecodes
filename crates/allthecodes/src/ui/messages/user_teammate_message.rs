//! Rust-side helper for teammate messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_teammate_message(teammate: &str, status: &str, _theme: &Theme) -> String {
    let teammate = if teammate.trim().is_empty() {
        "teammate"
    } else {
        teammate
    };
    let status = status.trim();
    if status.is_empty() {
        format!("Teammate {teammate}: update")
    } else {
        format!("Teammate {teammate}: {status}")
    }
}
