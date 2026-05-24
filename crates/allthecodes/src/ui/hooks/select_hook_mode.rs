//! Hook command selection.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookListItem {
    pub hook_type: String,
    pub display_text: String,
    pub source: String,
}

impl HookListItem {
    pub fn new(hook_type: impl Into<String>, display_text: impl Into<String>) -> Self {
        Self {
            hook_type: hook_type.into(),
            display_text: display_text.into(),
            source: "Effective Settings".to_string(),
        }
    }
}

pub fn render_select_hook_mode(
    title: &str,
    event_description: &str,
    hooks: &[HookListItem],
    selected_index: usize,
) -> String {
    let mut lines = vec![title.to_string()];
    lines.extend(event_description.lines().map(str::to_string));
    lines.push(String::new());
    if hooks.is_empty() {
        lines.push("No hooks configured for this event.".to_string());
        lines.push("To add hooks, edit .allthecodes/settings.json or ask Claude.".to_string());
        return lines.join("\n");
    }
    for (idx, hook) in hooks.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        lines.push(format!(
            "{marker} [{:<7}] {:<32} {}",
            hook.hook_type, hook.display_text, hook.source
        ));
    }
    lines.join("\n")
}
