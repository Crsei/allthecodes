//! Preview box rendering for ask-user-question prompts.

pub fn render_preview_box(title: &str, body: &str) -> String {
    let body = if body.trim().is_empty() {
        "<empty preview>"
    } else {
        body.trim()
    };
    format!("Preview: {title}\n{body}")
}
