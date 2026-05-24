//! Permission request title helpers.

pub fn render_permission_request_title(tool_name: &str, subject: &str, elevated: bool) -> String {
    let prefix = if elevated {
        "High-risk permission"
    } else {
        "Permission required"
    };
    if subject.trim().is_empty() {
        format!("{prefix}: {tool_name}")
    } else {
        format!("{prefix}: {tool_name} -> {subject}")
    }
}
