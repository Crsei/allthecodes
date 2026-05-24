//! Top-level hooks configuration menu.

use allthecodes_types::hooks::HookEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookConfigSummary {
    pub event: HookEvent,
    pub matcher_count: usize,
    pub hook_count: usize,
}

pub fn render_hooks_config_menu(items: &[HookConfigSummary], selected_index: usize) -> String {
    let mut lines = vec!["Hooks".to_string()];
    if items.is_empty() {
        lines.push("No hooks configured".to_string());
        return lines.join("\n");
    }
    for (idx, item) in items.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        lines.push(format!(
            "{marker} {:<18} matchers={} hooks={}",
            item.event, item.matcher_count, item.hook_count
        ));
    }
    lines.push(
        "Read-only. Edit .allthecodes/settings.json or .allthecodes/settings.local.json."
            .to_string(),
    );
    lines.join("\n")
}
