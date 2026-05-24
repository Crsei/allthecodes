//! Wizard step for selecting an agent color.

use super::render_step_frame;
use crate::ui::agents::color_picker::ColorPickerState;

pub fn render_color_step(agent_name: &str, current_color: Option<&str>) -> String {
    let state = ColorPickerState::new(agent_name, current_color);
    render_step_frame("Color", state.render(), true)
}
