//! Dispatcher for user tool result message rendering.

use crate::ui::messages::user_tool_result_message::utils::{
    find_tool_from_messages, ToolResultBlock, UserToolResultLookups, CANCEL_MESSAGE,
    INTERRUPT_MESSAGE_FOR_TOOL_USE, REJECT_MESSAGE, REJECT_MESSAGE_WITH_REASON_PREFIX,
};
use crate::ui::messages::user_tool_result_message::{
    rejected_plan_message::render_rejected_plan_message,
    rejected_tool_use_message::render_rejected_tool_use_message,
    user_tool_canceled_message::render_user_tool_canceled_message,
    user_tool_error_message::render_user_tool_error_message,
    user_tool_success_message::render_user_tool_success_message,
};
use crate::ui::theme::Theme;
use allthecodes_engine::types::tool::Tool;
use allthecodes_engine::types::tool::Tools;
use ratatui::text::Line;

pub fn render_user_tool_result_message(
    param: &ToolResultBlock,
    lookups: &UserToolResultLookups,
    tools: &Tools,
    theme: &Theme,
    _verbose: bool,
    _width: usize,
    _is_transcript_mode: bool,
) -> Vec<Line<'static>> {
    if param.content.starts_with(CANCEL_MESSAGE) {
        return render_user_tool_canceled_message(theme);
    }

    if param.content == INTERRUPT_MESSAGE_FOR_TOOL_USE {
        return render_user_tool_canceled_message(theme);
    }

    if param.content.starts_with(REJECT_MESSAGE) {
        if let Some(tool_resolution) = find_tool_from_messages(&param.tool_use_id, tools, lookups) {
            return user_tool_reject_with_tool(&tool_resolution, param, theme);
        }
        if param.content.starts_with(REJECT_MESSAGE_WITH_REASON_PREFIX) {
            return render_user_tool_error_message(
                &param.content,
                None,
                None,
                theme,
                _is_transcript_mode,
                _verbose,
            );
        }
        return render_rejected_tool_use_message(theme);
    }

    if param.is_error {
        if let Some(tool_resolution) = find_tool_from_messages(&param.tool_use_id, tools, lookups) {
            return render_user_tool_error_message(
                &param.content,
                Some(tool_resolution.tool.as_ref() as &dyn Tool),
                Some(&tool_resolution.input),
                theme,
                _is_transcript_mode,
                _verbose,
            );
        }
        if param
            .content
            .starts_with("The agent proposed a plan that was rejected by the user.")
        {
            return render_rejected_plan_message(
                "The agent proposed a plan that was rejected by the user.",
                theme,
            );
        }
        return render_user_tool_error_message(
            &param.content,
            None,
            None,
            theme,
            _is_transcript_mode,
            _verbose,
        );
    }

    if let Some(tool_resolution) = find_tool_from_messages(&param.tool_use_id, tools, lookups) {
        return render_user_tool_success_message(
            Some(tool_resolution.tool.as_ref() as &dyn Tool),
            Some(&tool_resolution.input),
            param.tool_use_result.as_deref(),
            theme,
            _verbose,
            _is_transcript_mode,
        );
    }

    if let Some(content) = param.tool_use_result.as_deref() {
        return render_user_tool_success_message(
            None,
            None,
            Some(content),
            theme,
            _verbose,
            _is_transcript_mode,
        );
    }

    render_fallback_result(param, theme)
}

fn user_tool_reject_with_tool(
    tool_resolution: &crate::ui::messages::user_tool_result_message::utils::ToolResolution,
    param: &ToolResultBlock,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let _tool_use_id = &tool_resolution.tool_use_id;
    if let Ok(input) = serde_json::from_str::<serde_json::Value>(&param.content) {
        let tool: &dyn Tool = tool_resolution.tool.as_ref();
        return crate::ui::messages::user_tool_result_message::user_tool_reject_message::render_user_tool_reject_message(
            Some(tool),
            &input,
            theme,
            false,
            false,
        );
    }

    let tool: &dyn Tool = tool_resolution.tool.as_ref();
    render_user_tool_reject_message(Some(tool), &tool_resolution.input, theme, false, false)
}

fn render_fallback_result(_param: &ToolResultBlock, theme: &Theme) -> Vec<Line<'static>> {
    vec![ratatui::text::Line::from(ratatui::text::Span::styled(
        "Tool result received (tool metadata unavailable)",
        theme.tool_result,
    ))]
}

fn render_user_tool_reject_message(
    tool: Option<&dyn Tool>,
    input: &serde_json::Value,
    theme: &Theme,
    verbose: bool,
    is_transcript_mode: bool,
) -> Vec<Line<'static>> {
    crate::ui::messages::user_tool_result_message::user_tool_reject_message::render_user_tool_reject_message(
        tool,
        input,
        theme,
        verbose,
        is_transcript_mode,
    )
}
