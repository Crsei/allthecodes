//! Render output for tool-use rejections (non-error refusal path).

use crate::ui::messages::user_tool_result_message::utils::format_tool_output;
use crate::ui::theme::Theme;
use allthecodes_engine::types::tool::Tool;
use ratatui::text::{Line, Span};
use serde_json::Value;

pub fn render_user_tool_reject_message(
    tool: Option<&dyn Tool>,
    input: &Value,
    theme: &Theme,
    _verbose: bool,
    _is_transcript_mode: bool,
) -> Vec<Line<'static>> {
    let display_name = match tool {
        Some(tool) => tool.user_facing_name(Some(input)).to_string(),
        None => "Tool".to_string(),
    };

    if display_name.is_empty() {
        return render_fallback(tool, theme);
    }

    let input_text = format_tool_output(&input.to_string());
    let mut lines = vec![Line::from(Span::styled(
        format!("{display_name} request rejected by user:"),
        theme.warning,
    ))];

    if input_text.is_empty() {
        lines.push(Line::from(Span::styled("(empty input)", theme.dim)));
    } else {
        lines.extend(
            input_text
                .lines()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme.dim))),
        );
    }

    lines
}
#[cfg(test)]
pub fn render_fallback_reject_message(theme: &Theme) -> Vec<Line<'static>> {
    render_fallback(None, theme)
}

fn render_fallback(tool: Option<&dyn Tool>, theme: &Theme) -> Vec<Line<'static>> {
    let label = match tool {
        Some(_) => "Tool call was rejected",
        None => "Tool use rejected by user (tool metadata unavailable)",
    };
    vec![Line::from(Span::styled(label, theme.warning))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::messages::user_tool_result_message::utils::line_to_text;

    #[test]
    fn fallback_reject_message_mentions_missing_metadata() {
        let lines = render_fallback_reject_message(&Theme::default());

        assert_eq!(
            line_to_text(&lines[0]),
            "Tool use rejected by user (tool metadata unavailable)"
        );
    }
}
