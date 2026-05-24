//! Preview question rendering.

use super::preview_box::render_preview_box;

pub fn render_preview_question_view(question: &str, options: &[impl AsRef<str>]) -> String {
    let mut body = vec![question.to_string()];
    body.extend(
        options
            .iter()
            .map(|option| format!("- {}", option.as_ref())),
    );
    render_preview_box("Question", &body.join("\n"))
}
