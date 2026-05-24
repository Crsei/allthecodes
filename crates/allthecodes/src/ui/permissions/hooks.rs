//! Permission hook status rendering.

use super::utils::render_bullets;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionHookEvent {
    pub hook_name: String,
    pub matcher: String,
    pub decision: String,
    pub notes: Vec<String>,
}

pub fn render_permission_hook_event(event: &PermissionHookEvent) -> String {
    let mut lines = vec![
        format!("hook: {}", event.hook_name),
        format!("matcher: {}", event.matcher),
        format!("decision: {}", event.decision),
    ];
    if !event.notes.is_empty() {
        lines.push(render_bullets("notes", &event.notes));
    }
    lines.join("\n")
}
