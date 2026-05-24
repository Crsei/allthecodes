//! Wizard step for the agent usage description.

use super::render_step_frame;
use crate::ui::agents::utils::wrap_text;

pub fn render_description_step(description: Option<&str>, width: usize) -> String {
    let description = description.unwrap_or("");
    let body = if description.trim().is_empty() {
        "description: <empty>".to_string()
    } else {
        wrap_text(description, width).join("\n")
    };
    render_step_frame("Description", body, description.trim().len() >= 12)
}
