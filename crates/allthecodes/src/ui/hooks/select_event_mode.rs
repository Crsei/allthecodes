//! Hook event selection.

pub use allthecodes_types::hooks::HookEvent;
pub use allthecodes_types::hooks::HOOK_EVENTS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEventRow {
    pub event: HookEvent,
    pub summary: String,
    pub hook_count: usize,
}

impl HookEventRow {
    pub fn new(event: HookEvent, summary: impl Into<String>, hook_count: usize) -> Self {
        Self {
            event,
            summary: summary.into(),
            hook_count,
        }
    }
}

pub fn render_select_event_mode(
    rows: &[HookEventRow],
    selected_index: usize,
    total_hooks_count: usize,
    restricted_by_policy: bool,
) -> String {
    let mut lines = vec![
        "Select hook event".to_string(),
        format!("{total_hooks_count} hooks configured"),
    ];
    if restricted_by_policy {
        lines.push(
            "Hooks are restricted by policy; user/project/local hooks are blocked.".to_string(),
        );
    }
    lines.push(
        "Read-only. Edit .allthecodes/settings.json or .allthecodes/settings.local.json."
            .to_string(),
    );
    lines.push("Use /hooks open user or /hooks open project to edit a settings layer.".to_string());
    lines.push(String::new());

    for (idx, row) in rows.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        let count = if row.hook_count > 0 {
            format!(" ({})", row.hook_count)
        } else {
            String::new()
        };
        lines.push(format!(
            "{marker} {:<18} {:<5} {}",
            row.event, count, row.summary
        ));
    }

    lines.join("\n")
}
