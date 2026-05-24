//! Question navigation bar rendering.

pub fn render_question_navigation_bar(
    current: usize,
    total: usize,
    can_go_back: bool,
    can_submit: bool,
) -> String {
    let back = if can_go_back { "Back" } else { "Back disabled" };
    let submit = if can_submit {
        "Submit"
    } else {
        "Submit disabled"
    };
    format!(
        "Question {}/{} | {} | Next | {}",
        current.min(total),
        total,
        back,
        submit
    )
}
