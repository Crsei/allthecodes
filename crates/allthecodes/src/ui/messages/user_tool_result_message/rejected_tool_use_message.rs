//! Render output for generic rejected tool use fallback.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn render_rejected_tool_use_message(theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        "Tool use rejected by user (tool metadata unavailable)",
        theme.warning,
    ))]
}
