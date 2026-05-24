//! Rust-side helper for channel messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_channel_message(channel: &str, message: &str, _theme: &Theme) -> String {
    let channel = if channel.trim().is_empty() {
        "default"
    } else {
        channel
    };
    let message = message.trim();
    if message.is_empty() {
        format!("Channel {channel}: <empty>")
    } else {
        format!("Channel {channel}: {message}")
    }
}
