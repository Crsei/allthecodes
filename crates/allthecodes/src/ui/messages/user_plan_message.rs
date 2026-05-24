//! Rust-side helper for user plan confirmation messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_user_plan_message(plan: &str, _theme: &Theme) -> String {
    let plan = plan.trim();
    if plan.is_empty() {
        "No plan provided".to_string()
    } else {
        format!("User plan: {plan}")
    }
}
