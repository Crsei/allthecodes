//! Wizard step for selecting an agent model.

use super::render_step_frame;
use crate::ui::agents::model_selector::render_model_selector;

pub fn render_model_step(model: Option<&str>) -> String {
    render_step_frame("Model", render_model_selector(model), true)
}
