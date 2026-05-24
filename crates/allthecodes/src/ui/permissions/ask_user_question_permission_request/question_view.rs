//! Ask-user-question question rendering.

use super::use_multiple_choice_state::{render_multiple_choice_state, MultipleChoiceState};

pub fn render_question_view(prompt: &str, state: &MultipleChoiceState) -> String {
    format!("{}\n{}", prompt, render_multiple_choice_state(state))
}
