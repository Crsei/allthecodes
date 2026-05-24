//! Wizard step for describing a generated agent.

use super::render_step_frame;

pub fn render_generate_step(goal: Option<&str>, generated: bool) -> String {
    let goal = goal.unwrap_or("").trim();
    let body = if goal.is_empty() {
        "goal: <empty>\nEnter what this agent should help with.".to_string()
    } else {
        format!(
            "goal: {goal}\ngenerated: {}",
            if generated { "yes" } else { "pending" }
        )
    };
    render_step_frame("Generate", body, !goal.is_empty() && generated)
}
