//! Enter-plan-mode permission request rendering.

pub fn render_enter_plan_mode_permission_request(reason: &str) -> String {
    format!("Enter plan mode\nreason: {reason}\nNo file edits will be made while planning")
}
