//! Render output for tool-use error blocks.

use crate::ui::messages::user_tool_result_message::{
    rejected_plan_message::render_rejected_plan_message,
    utils::{
        format_tool_output, tool_display_name, CLASSIFIER_DENIAL_PREFIX, PLAN_REJECTION_PREFIX,
        REJECT_MESSAGE_WITH_REASON_PREFIX,
    },
};
use crate::ui::theme::Theme;
use allthecodes_engine::types::tool::Tool;
use ratatui::text::{Line, Span};
use serde_json::Value;

pub fn render_user_tool_error_message(
    content: &str,
    tool: Option<&dyn Tool>,
    input: Option<&Value>,
    theme: &Theme,
    _is_transcript_mode: bool,
    _verbose: bool,
) -> Vec<Line<'static>> {
    if let Some(rest) = content.strip_prefix(PLAN_REJECTION_PREFIX) {
        return render_rejected_plan_message(rest, theme);
    }
    if let Some(rest) = content.strip_prefix(REJECT_MESSAGE_WITH_REASON_PREFIX) {
        let mut lines = vec![Line::from(Span::styled(
            "Tool use rejected by user (reason included):",
            theme.warning,
        ))];
        lines.extend(
            rest.lines()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme.warning))),
        );
        return lines;
    }
    if let Some(rest) = content.strip_prefix(CLASSIFIER_DENIAL_PREFIX) {
        let mut lines = vec![Line::from(Span::styled(
            "Tool use denied by classifier",
            theme.error,
        ))];
        lines.extend(
            format_tool_output(rest)
                .lines()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme.dim))),
        );
        return lines;
    }

    let tool_name = tool_display_name(tool, input);
    let mut lines = vec![Line::from(Span::styled(
        format!("{tool_name} failed with error:"),
        theme.error,
    ))];

    if content.is_empty() {
        lines.push(Line::from(Span::styled("  (no error detail)", theme.dim)));
        return lines;
    }

    let formatted = format_tool_output(content);
    lines.extend(
        formatted
            .lines()
            .map(|line| Line::from(Span::styled(format!("  {line}"), theme.dim))),
    );

    lines
}
