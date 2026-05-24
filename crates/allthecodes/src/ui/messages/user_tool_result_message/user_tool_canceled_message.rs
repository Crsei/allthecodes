//! Rendered output for tool invocations interrupted by user cancellation.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn render_user_tool_canceled_message(theme: &Theme) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        "Interrupted by user",
        theme.warning,
    ))]
}
