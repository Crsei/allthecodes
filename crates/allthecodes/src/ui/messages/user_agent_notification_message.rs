//! Rust-side helper for user agent notification messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_agent_notification_message(notification: &str, _theme: &Theme) -> String {
    let notification = notification.trim();
    if notification.is_empty() {
        "User agent notification".to_string()
    } else {
        format!("Notification: {notification}")
    }
}
