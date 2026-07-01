//! Rust-side helper for grouped tool-use output.

use crate::ui::theme::Theme;
use crate::ui::tool_activity::user_facing_tool_name;
use ratatui::text::{Line, Span};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GroupedToolUseView {
    pub tool_name: String,
    pub count: usize,
    pub resolved_count: usize,
    pub error_count: usize,
}

#[allow(dead_code)]
pub fn render_grouped_tool_use_lines(
    view: &GroupedToolUseView,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if view.count == 0 {
        return Vec::new();
    }
    let display_name = user_facing_tool_name(&view.tool_name);
    let mut status = format!("  ● {} {} calls", view.count, display_name);
    if view.error_count > 0 {
        status.push_str(&format!(" · {} failed", view.error_count));
    } else if view.resolved_count >= view.count {
        status.push_str(" · completed");
    } else if view.resolved_count > 0 {
        status.push_str(&format!(
            " · {}/{} completed",
            view.resolved_count, view.count
        ));
    }
    vec![Line::from(Span::styled(status, theme.tool_name))]
}

#[cfg(test)]
pub fn render_grouped_tool_use_content(tool_names: &[&str], _theme: &Theme) -> String {
    use crate::ui::tool_activity::{render_grouped_activity, ToolActivity, ToolState};

    let activities = tool_names
        .iter()
        .map(|name| ToolActivity::from_tool_use(name, "", ToolState::Queued))
        .collect::<Vec<_>>();
    format!(
        "Grouped tool uses:\n{}",
        render_grouped_activity(&activities)
    )
}
