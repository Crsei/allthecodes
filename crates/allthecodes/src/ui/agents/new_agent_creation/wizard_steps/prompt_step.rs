//! Wizard step for writing the system prompt.

use super::render_step_frame;
use crate::ui::agents::utils::wrap_text;

pub fn render_prompt_step(prompt: Option<&str>, width: usize) -> String {
    let prompt = prompt.unwrap_or("");
    let body = if prompt.trim().is_empty() {
        "system prompt: <empty>".to_string()
    } else {
        let mut lines = vec![format!("chars: {}", prompt.chars().count())];
        lines.extend(wrap_text(prompt, width));
        lines.join("\n")
    };
    render_step_frame("Prompt", body, prompt.trim().len() >= 40)
}
