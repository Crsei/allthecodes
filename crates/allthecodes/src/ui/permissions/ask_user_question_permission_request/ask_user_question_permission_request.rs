//! Ask-user-question permission request rendering.

use crate::ui::better_view_panel::BetterViewPanel;

use super::question_navigation_bar::render_question_navigation_bar;
use super::question_view::render_question_view;
use super::use_multiple_choice_state::MultipleChoiceState;

pub fn render_ask_user_question_permission_request(
    prompt: &str,
    state: &MultipleChoiceState,
    current: usize,
    total: usize,
) -> String {
    let mut lines = render_question_view(prompt, state)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    lines.push(String::new());
    lines.push("Navigation".to_string());
    lines.push(render_question_navigation_bar(
        current,
        total,
        current > 1,
        !state.submitted.is_empty(),
    ));
    BetterViewPanel::new("Need input")
        .summary(format!("question={current}/{total} source=tool request"))
        .sections_title("Choices")
        .sections(vec!["Answer".to_string(), "Navigation".to_string()], 0)
        .detail_title("Answer")
        .detail_lines(lines)
        .footer("Up/Down choice | Space toggle | Enter next | Esc cancel")
        .render()
}
