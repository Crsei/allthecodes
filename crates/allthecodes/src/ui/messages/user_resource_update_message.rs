//! Rust-side helper for user resource update messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_resource_update_message(resource: &str, delta: &str, _theme: &Theme) -> String {
    let resource = if resource.trim().is_empty() {
        "resource"
    } else {
        resource
    };
    let delta = delta.trim();
    if delta.is_empty() {
        format!("Resource update: {resource} unchanged")
    } else {
        format!("Resource update: {resource} {delta}")
    }
}
