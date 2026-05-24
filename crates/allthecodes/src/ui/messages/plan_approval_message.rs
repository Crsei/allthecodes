//! Rust-side helper for plan approval messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_plan_approval_message(plan_name: &str, approved: bool, _theme: &Theme) -> String {
    let state = if approved { "approved" } else { "rejected" };
    format!("Plan '{plan_name}' {state}")
}
