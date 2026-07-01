use ratatui::text::{Line, Span};

use allthecodes_types::message::{ContentBlock, InfoLevel, SystemSubtype, ToolResultContent};

use super::context::MessageRenderContext;
use super::copy_text::{compact_boundary_summary, image_reference};
use super::grouping::is_suppressed_operation_tool;
use super::render_user::{api_error_display_text, plain_text_to_lines};
use crate::ui::markdown::markdown_to_lines;
use crate::ui::messages::assistant_thinking_message::{
    render_assistant_thinking_lines, AssistantThinkingView,
};
use crate::ui::messages::assistant_tool_use_message::{
    render_assistant_tool_use_message, ToolUseState,
};
use crate::ui::messages::attachment_message::render_attachment_message as render_attachment_helper;
use crate::ui::messages::compact_boundary_message::render_compact_boundary_lines;
use crate::ui::messages::system_text_message::render_system_text_message;
use crate::ui::theme::Theme;

// ── Assistant messages ──────────────────────────────────────────────────

pub(super) fn render_assistant_message<'a>(
    msg: &allthecodes_types::message::AssistantMessage,
    theme: &Theme,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let mut first_block = true;
    let mut previous_block_was_tool_use = false;

    for (block_index, block) in msg.content.iter().enumerate() {
        match block {
            ContentBlock::Text { text } => {
                // If this is an API error message, use error classification
                // instead of standard markdown rendering.
                if msg.is_api_error_message {
                    if previous_block_was_tool_use {
                        lines.push(Line::default());
                    }
                    let error_text = api_error_display_text(text);
                    let style = theme.error;
                    lines.push(Line::from(Span::styled(error_text, style)));
                    first_block = false;
                    previous_block_was_tool_use = false;
                    continue;
                }
                let md_lines = markdown_to_lines(text, theme);
                if md_lines.is_empty() {
                    if first_block {
                        lines.push(Line::default());
                    }
                } else {
                    if previous_block_was_tool_use {
                        lines.push(Line::default());
                    }
                    for md_line in md_lines {
                        lines.push(md_line);
                    }
                }
                first_block = false;
                previous_block_was_tool_use = false;
            }

            ContentBlock::ConnectorText { connector_text, .. } => {
                let md_lines = markdown_to_lines(connector_text, theme);
                if md_lines.is_empty() {
                    if first_block {
                        lines.push(Line::default());
                    }
                } else {
                    if previous_block_was_tool_use {
                        lines.push(Line::default());
                    }
                    for md_line in md_lines {
                        lines.push(md_line);
                    }
                }
                first_block = false;
                previous_block_was_tool_use = false;
            }

            ContentBlock::ToolUse { id, name, input } => {
                // When the operation pipeline is active, suppress individual
                // tool-use rendering — it will be displayed as an operation row.
                if !render_context.options.verbose
                    && msg.content.len() == 1
                    && is_suppressed_operation_tool(name)
                {
                    previous_block_was_tool_use = true;
                    continue;
                }
                if !first_block {
                    lines.push(Line::default());
                }
                let input_json = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                let state = tool_state_for_id(id, render_context);
                let rendered =
                    render_assistant_tool_use_message(name, &input_json, state, false, theme);
                for line in rendered.lines() {
                    lines.push(Line::from(vec![
                        Span::raw(if first_block { "" } else { "        " }),
                        Span::styled(line.to_string(), theme.tool_name),
                    ]));
                }
                first_block = false;
                previous_block_was_tool_use = true;
            }

            ContentBlock::ServerToolUse { id, name, input } => {
                // When the operation pipeline is active, suppress individual
                // tool-use rendering — it will be displayed as an operation row.
                if !render_context.options.verbose
                    && msg.content.len() == 1
                    && is_suppressed_operation_tool(name)
                {
                    previous_block_was_tool_use = true;
                    continue;
                }
                if !first_block {
                    lines.push(Line::default());
                }
                let input_json = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                let state = tool_state_for_id(id, render_context);
                let rendered =
                    render_assistant_tool_use_message(name, &input_json, state, false, theme);
                for line in rendered.lines() {
                    lines.push(Line::from(vec![
                        Span::raw(if first_block { "" } else { "        " }),
                        Span::styled(format!("server: {line}"), theme.tool_name),
                    ]));
                }
                first_block = false;
                previous_block_was_tool_use = true;
            }

            ContentBlock::ToolResult {
                tool_use_id: _,
                content,
                is_error,
            } => {
                if previous_block_was_tool_use {
                    lines.push(Line::default());
                }
                let style = if *is_error {
                    theme.error
                } else {
                    theme.tool_result
                };
                let text = match content {
                    ToolResultContent::Text(t) => t.clone(),
                    ToolResultContent::Blocks(blocks) => blocks
                        .iter()
                        .map(|b| match b {
                            ContentBlock::Text { text } => text.clone(),
                            ContentBlock::ConnectorText { connector_text, .. } => {
                                connector_text.clone()
                            }
                            ContentBlock::Image { source } => image_reference(source),
                            _ => "[...]".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                let prefix_str = if *is_error { "  Error: " } else { "  Result: " };
                // Show first few lines of result.
                let result_lines: Vec<&str> = text.lines().take(5).collect();
                for (i, rl) in result_lines.iter().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::raw("        "),
                            Span::styled(prefix_str.to_string(), style),
                            Span::styled(rl.to_string(), style),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("                  "),
                            Span::styled(rl.to_string(), style),
                        ]));
                    }
                }
                let total_lines = text.lines().count();
                if total_lines > 5 {
                    lines.push(Line::from(vec![
                        Span::raw("                  "),
                        Span::styled(format!("... {} more lines", total_lines - 5), theme.dim),
                    ]));
                }
                first_block = false;
                previous_block_was_tool_use = false;
            }

            ContentBlock::Thinking {
                thinking,
                signature: _,
            } => {
                let block_id = format!("{}:{block_index}", msg.uuid);
                let animation_frame = (render_context.lookups.last_thinking_block_id.as_deref()
                    == Some(block_id.as_str()))
                .then_some(render_context.options.thinking_animation_frame)
                .flatten();
                let thinking_lines = render_assistant_thinking_lines(
                    &AssistantThinkingView {
                        thinking: thinking.clone(),
                        verbose: render_context.options.verbose,
                        is_transcript_mode: render_context.options.is_transcript_mode,
                        animation_frame,
                    },
                    theme,
                );
                if previous_block_was_tool_use && !thinking_lines.is_empty() {
                    lines.push(Line::default());
                }
                for (i, line) in thinking_lines.into_iter().enumerate() {
                    if first_block && i == 0 {
                        lines.push(line);
                    } else {
                        let mut spans = vec![Span::raw("        ")];
                        spans.extend(line.spans);
                        lines.push(Line::from(spans));
                    }
                }
                if !thinking.is_empty()
                    && (render_context.options.verbose || render_context.options.is_transcript_mode)
                {
                    first_block = false;
                    previous_block_was_tool_use = false;
                }
            }

            ContentBlock::RedactedThinking { .. } => {
                if render_context.options.verbose || render_context.options.is_transcript_mode {
                    if previous_block_was_tool_use {
                        lines.push(Line::default());
                    }
                    for (i, line) in crate::ui::messages::assistant_redacted_thinking_message::render_assistant_redacted_thinking_lines(theme).into_iter().enumerate() {
                        if first_block && i == 0 {
                            lines.push(line);
                        } else {
                            let mut spans = vec![Span::raw("        ")];
                            spans.extend(line.spans);
                            lines.push(Line::from(spans));
                        }
                    }
                    first_block = false;
                    previous_block_was_tool_use = false;
                }
            }

            ContentBlock::Image { source } => {
                if previous_block_was_tool_use {
                    lines.push(Line::default());
                }
                lines.push(Line::from(vec![
                    Span::raw(if first_block { "" } else { "        " }),
                    Span::styled(image_reference(source), theme.dim),
                ]));
                first_block = false;
                previous_block_was_tool_use = false;
            }
        }
    }

    // If there was no content at all, at least show the name.
    if lines.is_empty() {
        if msg.content.iter().all(|block| {
            matches!(
                block,
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
            )
        }) {
            return Vec::new();
        }
        lines.push(Line::default());
    }

    // Show cost if non-zero.
    if msg.cost_usd > 0.0 {
        lines.push(Line::from(vec![Span::styled(
            format!("(${:.4})", msg.cost_usd),
            theme.dim,
        )]));
    }

    lines
}

pub(super) fn tool_state_for_id(
    tool_use_id: &str,
    render_context: &MessageRenderContext,
) -> ToolUseState {
    if render_context
        .lookups
        .errored_tool_use_ids
        .contains(tool_use_id)
    {
        ToolUseState::Error
    } else if render_context
        .lookups
        .resolved_tool_use_ids
        .contains(tool_use_id)
    {
        ToolUseState::Resolved
    } else {
        ToolUseState::InProgress
    }
}

// ── System messages ─────────────────────────────────────────────────────

pub(super) fn render_system_message<'a>(
    msg: &allthecodes_types::message::SystemMessage,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let (prefix, style) = match &msg.subtype {
        SystemSubtype::CompactBoundary { .. } => ("context compacted", theme.dim),
        SystemSubtype::MicrocompactBoundary { .. } => ("context microcompacted", theme.dim),
        SystemSubtype::ApiError { .. } => ("", theme.error),
        SystemSubtype::Informational { level } => match level {
            InfoLevel::Info => ("Info: ", theme.unselected),
            InfoLevel::Warning => ("Warning: ", theme.warning),
            InfoLevel::Error => ("Error: ", theme.error),
        },
        SystemSubtype::LocalCommand { .. } => ("$ ", theme.system_name),
        SystemSubtype::Warning => ("Warning: ", theme.warning),
    };

    if let SystemSubtype::ApiError {
        retry_attempt,
        max_retries: _,
        retry_in_ms,
        error,
    } = &msg.subtype
    {
        let detail = if msg.content.trim().is_empty() {
            error.message.as_str()
        } else {
            msg.content.trim()
        };
        let rendered = if *retry_in_ms > 0 {
            render_system_text_message(
                "api_error",
                &format!("retry_attempt={} {}", retry_attempt, detail),
                theme,
            )
        } else {
            render_system_text_message("api_error", detail, theme)
        };
        return plain_text_to_lines(&rendered, theme.error);
    }

    if matches!(&msg.subtype, SystemSubtype::MicrocompactBoundary { .. }) {
        return Vec::new();
    }

    if matches!(&msg.subtype, SystemSubtype::CompactBoundary { .. }) {
        return render_compact_boundary_lines(theme);
    }

    if matches!(
        &msg.subtype,
        SystemSubtype::CompactBoundary { .. } | SystemSubtype::MicrocompactBoundary { .. }
    ) {
        lines.push(Line::from(vec![Span::styled(
            compact_boundary_summary(&msg.subtype, prefix),
            style,
        )]));
    } else {
        let content_lines: Vec<&str> = msg.content.lines().collect();
        if content_lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(prefix.to_string(), style)]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), style),
                Span::styled(content_lines[0].to_string(), style),
            ]));
            for extra in &content_lines[1..] {
                lines.push(Line::from(Span::styled(format!("  {}", extra), style)));
            }
        }
    }

    lines
}

// ── Progress messages ───────────────────────────────────────────────────

pub(super) fn render_progress_message<'a>(
    msg: &allthecodes_types::message::ProgressMessage,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let data_summary = if msg.data.is_object() {
        msg.data
            .as_object()
            .and_then(|o| o.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        msg.data.to_string()
    };

    vec![Line::from(vec![
        Span::styled("  ... ", theme.dim),
        Span::styled(data_summary, theme.dim),
    ])]
}

// ── Attachment messages ─────────────────────────────────────────────────

pub(super) fn render_attachment_message<'a>(
    msg: &allthecodes_types::message::AttachmentMessage,
    theme: &Theme,
) -> Vec<Line<'a>> {
    use allthecodes_types::message::Attachment;
    let text = match &msg.attachment {
        Attachment::EditedTextFile { path } => format!("[edited: {}]", path),
        Attachment::QueuedCommand { prompt, .. } => render_attachment_helper(
            "queued_command",
            &serde_json::json!({ "prompt": prompt }).to_string(),
            theme,
        ),
        Attachment::MaxTurnsReached {
            max_turns,
            turn_count,
        } => format!("[max turns reached: {}/{}]", turn_count, max_turns),
        Attachment::StructuredOutput { .. } => "[structured output]".to_string(),
        Attachment::HookStoppedContinuation => "[hook stopped continuation]".to_string(),
        Attachment::NestedMemory { path, .. } => {
            render_attachment_helper("nested_memory", path, theme)
        }
        Attachment::SkillDiscovery { skills } => render_attachment_helper(
            "skill_discovery",
            &serde_json::to_string(skills).unwrap_or_else(|_| "[]".to_string()),
            theme,
        ),
    };
    vec![Line::from(Span::styled(text, theme.dim))]
}
