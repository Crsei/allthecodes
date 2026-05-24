//! Submit view for ask-user-question prompts.

pub fn render_submit_questions_view(answer_count: usize, total_questions: usize) -> String {
    format!("Ready to submit {answer_count}/{total_questions} answer(s)\nEnter submit | Esc cancel")
}
