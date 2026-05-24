//! Render output for successful tool invocations.

use crate::ui::messages::user_tool_result_message::utils::format_tool_output;
use crate::ui::theme::Theme;
use allthecodes_engine::types::tool::Tool;
use ratatui::text::{Line, Span};

pub fn render_user_tool_success_message(
    tool: Option<&dyn Tool>,
    input: Option<&serde_json::Value>,
    tool_use_result: Option<&str>,
    theme: &Theme,
    _verbose: bool,
    _is_transcript_mode: bool,
) -> Vec<Line<'static>> {
    let output = tool_use_result.unwrap_or("<no output>");
    let formatted_output = format_tool_output(output);
    let tool_name = match tool {
        Some(tool) => tool.user_facing_name(input).trim().to_string(),
        None => String::new(),
    };

    if formatted_output.is_empty() {
        return vec![Line::from(Span::styled(
            if tool_name.is_empty() {
                "Tool completed with no output".to_string()
            } else {
                format!("{tool_name} completed with no output")
            },
            theme.dim,
        ))];
    }

    let mut lines = Vec::new();
    if !tool_name.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{tool_name} result:"),
            theme.tool_name,
        )));
    }

    lines.extend(
        formatted_output
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), theme.tool_result))),
    );

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("<empty tool output>", theme.dim)));
    }

    lines
}
