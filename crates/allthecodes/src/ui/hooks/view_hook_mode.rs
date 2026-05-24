//! Read-only hook detail view.

use allthecodes_types::hooks::HookEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookView {
    pub event: HookEvent,
    pub matcher: Option<String>,
    pub event_supports_matcher: bool,
    pub hook_type: String,
    pub source: String,
    pub plugin_name: Option<String>,
    pub content_label: String,
    pub content_value: String,
    pub status_message: Option<String>,
}

pub fn render_view_hook_mode(view: &HookView) -> String {
    let mut lines = vec!["Hook details".to_string(), format!("Event: {}", view.event)];
    if view.event_supports_matcher {
        lines.push(format!(
            "Matcher: {}",
            view.matcher
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or("(all)")
        ));
    }
    lines.extend([
        format!("Type: {}", view.hook_type),
        format!("Source: {}", view.source),
    ]);
    if let Some(plugin_name) = &view.plugin_name {
        lines.push(format!("Plugin: {plugin_name}"));
    }
    lines.push(String::new());
    lines.push(format!("{}:", view.content_label));
    lines.push(format!("  {}", view.content_value));
    if let Some(status_message) = &view.status_message {
        lines.push(format!("Status message: {status_message}"));
    }
    lines.push(String::new());
    lines.push(
        "To modify or remove this hook, edit .allthecodes/settings.json or ask Claude.".to_string(),
    );
    lines.join("\n")
}
