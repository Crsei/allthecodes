use ratatui::style::Style;
use ratatui::text::{Line, Span};

use allthecodes_types::message::{ContentBlock, MessageContent, ToolResultContent};

use super::context::MessageRenderContext;
use super::copy_text::{content_block_copy_text, image_reference};
use super::grouping::is_suppressed_tool_result;
use crate::ui::messages::assistant_text_message::{classify_assistant_text, render_api_error};
use crate::ui::messages::file_edit_tool_updated_message::{
    render_file_edit_tool_updated_message, FileEditMessageStyle, FileEditToolUpdatedView,
};
use crate::ui::messages::user_bash_output_message::{
    render_user_bash_output_message_with_options, ShellOutputRenderOptions,
};
use crate::ui::messages::user_text_message::{
    render_user_text_message, CONVERSATION_INTERRUPTED_MESSAGE,
};
use crate::ui::messages::user_tool_result_message::user_tool_result_message::render_user_tool_result_message;
use crate::ui::messages::user_tool_result_message::utils::{
    ToolResultBlock, ToolUseRecord as UserToolUseRecord, UserToolResultLookups,
};
use crate::ui::theme::Theme;

use super::USER_MESSAGE_BACKGROUND;

// ── User messages ───────────────────────────────────────────────────────

pub(super) fn render_user_message<'a>(
    msg: &allthecodes_types::message::UserMessage,
    theme: &Theme,
    msg_index: usize,
    width: usize,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let content_text = match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => {
            if let Some(tool_lines) = render_tool_result_user_message(
                msg,
                blocks,
                theme,
                msg_index,
                width,
                render_context,
            ) {
                return tool_lines;
            }
            if let Some(image_lines) = render_user_image_blocks(blocks, theme) {
                return image_lines;
            }
            blocks
                .iter()
                .filter_map(content_block_copy_text)
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    if let Some(rendered) =
        render_tagged_user_text(&content_text, msg.uuid, render_context, theme, width)
    {
        return rendered;
    }

    if content_text.trim() == "[Request interrupted by user]" {
        return vec![Line::from(Span::styled(
            "Interrupted by user",
            theme.warning,
        ))];
    }
    if content_text.trim() == CONVERSATION_INTERRUPTED_MESSAGE {
        return vec![Line::from(Span::styled(
            CONVERSATION_INTERRUPTED_MESSAGE,
            theme.warning,
        ))];
    }

    let routed = render_user_text_message(&content_text, theme);
    if routed.is_empty() {
        return Vec::new();
    }
    let content_text = routed.to_string();
    let user_style = Style::default().bg(USER_MESSAGE_BACKGROUND);

    let content_lines: Vec<&str> = content_text.lines().collect();
    if content_lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(" ", user_style)]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            format!(" {}", content_lines[0]),
            user_style,
        )]));
        for extra in &content_lines[1..] {
            lines.push(Line::from(vec![Span::styled(
                format!(" {}", extra),
                user_style,
            )]));
        }
    }

    lines
}

fn render_user_image_blocks<'a>(blocks: &[ContentBlock], theme: &Theme) -> Option<Vec<Line<'a>>> {
    let images = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image { source } => Some(image_reference(source)),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!images.is_empty()).then(|| {
        images
            .into_iter()
            .map(|image| Line::from(Span::styled(image, theme.dim)))
            .collect()
    })
}

fn render_tagged_user_text<'a>(
    text: &str,
    uuid: uuid::Uuid,
    render_context: &MessageRenderContext,
    theme: &Theme,
    width: usize,
) -> Option<Vec<Line<'a>>> {
    let trimmed = text.trim();
    if !(trimmed.starts_with("<bash-stdout") || trimmed.starts_with("<bash-stderr")) {
        return None;
    }
    let output = extract_xmlish_body(trimmed).unwrap_or(trimmed);
    let rendered = render_user_bash_output_message_with_options(
        "bash",
        output,
        ShellOutputRenderOptions {
            width: width.max(20),
            expanded: render_context.lookups.latest_bash_output_uuid == Some(uuid),
            total_lines: Some(output.lines().count()),
            total_bytes: Some(output.len()),
            ..ShellOutputRenderOptions::default()
        },
    );
    Some(styled_text_lines(&rendered, theme.tool_result))
}

fn extract_xmlish_body(text: &str) -> Option<&str> {
    let start = text.find('>')? + 1;
    let end = text.rfind("</").unwrap_or(text.len());
    Some(text[start..end].trim())
}

fn render_tool_result_user_message<'a>(
    msg: &allthecodes_types::message::UserMessage,
    blocks: &[ContentBlock],
    theme: &Theme,
    msg_index: usize,
    width: usize,
    render_context: &MessageRenderContext,
) -> Option<Vec<Line<'a>>> {
    let tool_result = blocks.iter().find_map(|block| match block {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Some((tool_use_id, content, *is_error)),
        _ => None,
    })?;

    // When operation pipeline is active, suppress tool results for tools
    // that are consumed by the operation batch (Read/Search/Edit/Write, not shell).
    if !render_context.options.verbose {
        if let Some(tool_use) = render_context.tool_use(tool_result.0) {
            if is_suppressed_tool_result(&tool_use.tool_name) {
                return Some(Vec::new());
            }
        }
    }

    if let Some(tool_use) = render_context.tool_use(tool_result.0) {
        if tool_use.is_shell() {
            let output = msg
                .tool_use_result
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| tool_result_content_text(tool_result.1));
            let expanded = render_context.shell_expanded(msg_index, tool_result.0);
            let rendered = render_user_bash_output_message_with_options(
                &tool_use.command(),
                &output,
                ShellOutputRenderOptions {
                    width: width.max(20),
                    expanded,
                    total_lines: Some(output.lines().count()),
                    total_bytes: Some(output.len()),
                    ..ShellOutputRenderOptions::default()
                },
            );
            let style = if tool_result.2 {
                theme.error
            } else {
                theme.tool_result
            };
            return Some(styled_text_lines(&rendered, style));
        }
    }

    if let Some(preview) = msg.tool_use_result.as_deref() {
        if !tool_result.2 {
            if let Some(lines) = render_file_edit_preview(preview, theme) {
                return Some(lines);
            }
        }
    }

    let text = tool_result_content_text(tool_result.1);
    let mut block = ToolResultBlock::new(tool_result.0.clone(), tool_result.2, text.clone());
    block.tool_use_result = msg
        .tool_use_result
        .clone()
        .or_else(|| (!text.is_empty()).then_some(text));
    let mut lookups = UserToolResultLookups::new();
    for (id, record) in &render_context.lookups.tool_uses {
        lookups.tool_use_by_tool_use_id.insert(
            id.clone(),
            UserToolUseRecord {
                tool_name: record.tool_name.clone(),
                input: record.input.clone(),
            },
        );
    }
    let tools: allthecodes_engine::types::tool::Tools = Vec::new();
    Some(render_user_tool_result_message(
        &block,
        &lookups,
        &tools,
        theme,
        render_context.options.verbose,
        width,
        render_context.options.is_transcript_mode,
    ))
}

fn render_file_edit_preview<'a>(preview: &str, theme: &Theme) -> Option<Vec<Line<'a>>> {
    let value = serde_json::from_str::<serde_json::Value>(preview).ok()?;
    if value.get("kind").and_then(|v| v.as_str()) != Some("file_edit") {
        return None;
    }
    let file_path = value.get("path").and_then(|v| v.as_str())?.to_string();
    let hunk_lines = value
        .get("hunk_lines")?
        .as_array()?
        .iter()
        .filter_map(|line| line.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    if hunk_lines.is_empty() {
        return None;
    }

    let rendered = render_file_edit_tool_updated_message(&FileEditToolUpdatedView {
        file_path,
        hunk_lines,
        style: FileEditMessageStyle::Regular,
        verbose: true,
        preview_hint: None,
        width: 100,
        max_lines: 48,
    });
    Some(
        rendered
            .lines()
            .enumerate()
            .map(|(idx, line)| {
                let style = if idx == 0 {
                    theme.tool_name
                } else {
                    theme.tool_result
                };
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect(),
    )
}

pub(super) fn tool_result_content_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                ContentBlock::Image { source } => Some(image_reference(source)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub(super) fn styled_text_lines<'a>(text: &str, style: Style) -> Vec<Line<'a>> {
    if text.is_empty() {
        return vec![Line::from(Span::styled("<empty tool output>", style))];
    }
    text.lines()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

pub(super) fn plain_text_to_lines<'a>(text: &str, style: Style) -> Vec<Line<'a>> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines()
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect()
}

pub(super) fn api_error_display_text(text: &str) -> String {
    if let Some(error) = classify_assistant_text(text) {
        render_api_error(&error)
    } else {
        format!("Error occurred: {text}")
    }
}
