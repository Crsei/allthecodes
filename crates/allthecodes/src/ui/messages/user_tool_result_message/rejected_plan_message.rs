//! Render output for explicitly rejected plans.

use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn render_rejected_plan_message(plan: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "User rejected Claude's plan:",
        theme.dim,
    ))];

    if plan.trim().is_empty() {
        lines.push(Line::from(Span::styled("  (no plan text)", theme.dim)));
        return lines;
    }

    lines.extend(
        plan.lines()
            .map(|line| Line::from(Span::styled(format!("  {line}"), theme.dim))),
    );

    lines
}
