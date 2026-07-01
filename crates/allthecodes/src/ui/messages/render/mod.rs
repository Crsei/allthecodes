mod context;
mod copy_text;
mod grouping;
mod operation_grouping;
mod preprocessing;
mod render_assistant;
mod render_user;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use allthecodes_tool_display::ToolOperation;
use allthecodes_types::message::{ContentBlock, Message};

use crate::ui::messages::tool_operation_content::{
    render_todo_operation_lines, render_tool_operation_lines, ToolOperationView,
};
use crate::ui::theme::Theme;
use crate::ui::virtual_scroll::VirtualScroll;

use self::context::RenderableMessage;
use self::copy_text::{
    attachment_copy_text, attachment_reference, content_block_copy_text, content_block_reference,
    message_content_copy_text, message_content_reference,
};
use self::render_assistant::{
    render_assistant_message, render_attachment_message, render_progress_message,
    render_system_message,
};
use self::render_user::render_user_message;
use super::wrap::wrap_line_to_width;

const USER_MESSAGE_BACKGROUND: Color = Color::Rgb(31, 35, 42);

// Re-exports to keep the external API path unchanged.
#[cfg(test)]
pub(crate) use self::context::build_message_render_context;
pub(crate) use self::context::build_message_render_context_with_options;
pub(crate) use self::context::{MessageRenderContext, MessageRenderOptions};

/// Render only the visible messages into the given buffer area using virtual
/// scrolling.
///
/// `vscroll` must have been updated via `ensure_up_to_date()` before calling.
/// `scroll` is the number of rendered lines to skip from the top.
#[expect(
    clippy::too_many_arguments,
    reason = "message renderer takes explicit ratatui render state to avoid per-frame allocations"
)]
pub fn render_messages(
    _messages: &[Message],
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    streaming: bool,
    scroll: usize,
    vscroll: &VirtualScroll,
    render_context: &MessageRenderContext,
) {
    let renderable_messages = render_context.renderable_messages();
    if area.height == 0 || area.width == 0 || renderable_messages.is_empty() {
        return;
    }

    let viewport_h = area.height as usize;
    let (start, end) = vscroll.visual_range(scroll, viewport_h);

    let mut y = 0usize; // current row in the viewport

    for idx in start..end.min(renderable_messages.len()) {
        let fill_user_background =
            renderable_message_uses_user_background(&renderable_messages[idx]);
        let mut msg_lines = render_renderable_message_wrapped(
            &renderable_messages[idx],
            idx,
            theme,
            area.width,
            render_context,
        );
        if streaming
            && idx == renderable_messages.len().saturating_sub(1)
            && renderable_messages[idx].is_assistant_message()
        {
            if let Some(last_line) = msg_lines.last_mut() {
                last_line.spans.push(Span::styled(" ▌", theme.dim));
            }
        }

        // Separator blank line (between messages, not after last)
        let has_sep = idx < renderable_messages.len() - 1;
        let total_for_msg = msg_lines.len() + if has_sep { 1 } else { 0 };

        let message_offset = vscroll.visual_offset_of(idx);
        let skip = scroll.saturating_sub(message_offset).min(total_for_msg);

        for li in skip..total_for_msg {
            if y >= viewport_h {
                return;
            }
            let line = if li < msg_lines.len() {
                if fill_user_background {
                    buf.set_style(
                        Rect::new(area.x, area.y + y as u16, area.width, 1),
                        Style::default().bg(USER_MESSAGE_BACKGROUND),
                    );
                }
                &msg_lines[li]
            } else {
                // separator blank line
                &Line::default()
            };
            buf.set_line(area.x, area.y + y as u16, line, area.width);
            y += 1;
        }
    }
}

fn renderable_message_uses_user_background(msg: &RenderableMessage) -> bool {
    match msg {
        RenderableMessage::Message {
            message: Message::User(user),
            ..
        } => user_message_uses_background(user),
        RenderableMessage::Message { .. }
        | RenderableMessage::ToolOperationBatch(_)
        | RenderableMessage::TodoList(_) => false,
    }
}

fn user_message_uses_background(user: &allthecodes_types::message::UserMessage) -> bool {
    let content_text = match &user.content {
        allthecodes_types::message::MessageContent::Text(text) => text.as_str().to_string(),
        allthecodes_types::message::MessageContent::Blocks(blocks) => {
            if blocks.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolResult { .. } | ContentBlock::Image { .. }
                )
            }) {
                return false;
            }
            blocks
                .iter()
                .filter_map(content_block_copy_text)
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    let trimmed = content_text.trim();
    if trimmed == "[Request interrupted by user]"
        || trimmed == crate::ui::messages::user_text_message::CONVERSATION_INTERRUPTED_MESSAGE
        || trimmed.starts_with("<bash-stdout")
        || trimmed.starts_with("<bash-stderr")
    {
        return false;
    }
    !matches!(
        crate::ui::messages::user_text_message::route_user_text(&content_text),
        crate::ui::messages::user_text_message::UserTextRendered::Hidden
    )
}

pub(crate) fn render_renderable_message_wrapped<'a>(
    msg: &RenderableMessage,
    index: usize,
    theme: &Theme,
    width: u16,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    render_renderable_message_for_layout(msg, index, theme, width as usize, render_context)
        .into_iter()
        .flat_map(|line| wrap_line_to_width(&line, width))
        .collect()
}

pub(crate) fn render_renderable_message_for_layout<'a>(
    msg: &RenderableMessage,
    index: usize,
    theme: &Theme,
    width: usize,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    let mut lines =
        render_renderable_message_with_context(msg, index, theme, width, render_context);
    if renderable_message_uses_user_background(msg) && !lines.is_empty() {
        lines.insert(0, Line::default());
        lines.push(Line::default());
    }
    if msg.has_source_index(render_context.selected_message) {
        if let RenderableMessage::Message { message, .. } = msg {
            decorate_selected_message(
                &mut lines,
                message,
                theme,
                render_context.selected_expanded,
                width,
            );
        }
    }
    lines
}

/// Render a single message into one or more `Line`s.
///
/// `pub(super)` so that `virtual_scroll` can call it for height measurement.
#[cfg(test)]
pub(in crate::ui) fn render_single_message<'a>(msg: &Message, theme: &Theme) -> Vec<Line<'a>> {
    render_single_message_with_context(msg, 0, theme, 80, &MessageRenderContext::default())
}

pub(in crate::ui) fn render_single_message_with_context<'a>(
    msg: &Message,
    index: usize,
    theme: &Theme,
    width: usize,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    match msg {
        Message::User(user_msg) => {
            render_user_message(user_msg, theme, index, width, render_context)
        }
        Message::Assistant(assistant_msg) => {
            render_assistant_message(assistant_msg, theme, render_context)
        }
        Message::System(system_msg) => render_system_message(system_msg, theme),
        Message::Progress(progress_msg) => render_progress_message(progress_msg, theme),
        Message::Attachment(attachment_msg) => render_attachment_message(attachment_msg, theme),
    }
}

pub(in crate::ui) fn render_renderable_message_with_context<'a>(
    msg: &RenderableMessage,
    index: usize,
    theme: &Theme,
    width: usize,
    render_context: &MessageRenderContext,
) -> Vec<Line<'a>> {
    match msg {
        RenderableMessage::Message { message, .. } => {
            render_single_message_with_context(message, index, theme, width, render_context)
        }
        RenderableMessage::ToolOperationBatch(batch) => {
            if batch.is_batch {
                let op_refs: Vec<&ToolOperation> = batch.operations.iter().collect();
                let view = ToolOperationView::from_batch(&op_refs);
                render_tool_operation_lines(&view, theme)
            } else if let Some(op) = batch.operations.first() {
                let view = ToolOperationView::from_operation(op);
                render_tool_operation_lines(&view, theme)
            } else {
                Vec::new()
            }
        }
        RenderableMessage::TodoList(todo) => render_todo_operation_lines(&todo.operations, theme),
    }
}

pub(in crate::ui) fn message_copy_text(msg: &Message) -> String {
    match msg {
        Message::User(user) => message_content_copy_text(&user.content),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .filter_map(content_block_copy_text)
            .collect::<Vec<_>>()
            .join("\n"),
        Message::System(system) => system.content.clone(),
        Message::Progress(progress) => progress
            .data
            .get("message")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| progress.data.to_string()),
        Message::Attachment(attachment) => attachment_copy_text(&attachment.attachment),
    }
}

pub(in crate::ui) fn message_primary_reference(msg: &Message) -> Option<String> {
    match msg {
        Message::User(user) => message_content_reference(&user.content),
        Message::Assistant(assistant) => assistant.content.iter().find_map(content_block_reference),
        Message::Attachment(attachment) => attachment_reference(&attachment.attachment),
        Message::System(_) | Message::Progress(_) => None,
    }
}

fn decorate_selected_message<'a>(
    lines: &mut Vec<Line<'a>>,
    msg: &Message,
    theme: &Theme,
    expanded: bool,
    width: usize,
) {
    let mut header = vec![
        Span::styled(
            if expanded {
                "▼ selected"
            } else {
                "▶ selected"
            },
            theme.selected,
        ),
        Span::styled(" · c copy", theme.dim),
        Span::styled(" · o open", theme.dim),
        Span::styled(" · enter detail", theme.dim),
    ];
    if let Some(meta) = selected_message_meta(msg) {
        header.push(Span::styled(format!(" · {meta}"), theme.dim));
    }
    lines.insert(0, Line::from(header));

    if expanded {
        lines.extend(
            message_detail_lines(msg, width)
                .into_iter()
                .map(|line| Line::from(Span::styled(format!("  {line}"), theme.dim))),
        );
    }
}

fn selected_message_meta(msg: &Message) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(ts) = concise_timestamp(msg.timestamp()) {
        parts.push(ts);
    }
    if let Some(reference) = message_primary_reference(msg) {
        parts.push(reference);
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn concise_timestamp(timestamp: i64) -> Option<String> {
    if timestamp <= 0 {
        return None;
    }
    let millis = if timestamp > 10_000_000_000 {
        timestamp
    } else {
        timestamp.saturating_mul(1000)
    };
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
}

fn message_detail_lines(msg: &Message, width: usize) -> Vec<String> {
    let mut lines = vec![format!("uuid: {}", msg.uuid())];
    if let Some(ts) = concise_timestamp(msg.timestamp()) {
        lines.push(format!("time: {ts}"));
    }
    if let Some(reference) = message_primary_reference(msg) {
        lines.push(format!("ref: {reference}"));
    }
    let preview = allthecodes_utils::messages::truncate_text(
        &message_copy_text(msg).replace('\n', " ⏎ "),
        width.saturating_sub(4).max(20),
    );
    if !preview.is_empty() {
        lines.push(format!("copy: {preview}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{message_copy_text, message_primary_reference, render_single_message};
    use crate::ui::diff::file_edit_diff::unified_hunk_lines_from_edit;
    use crate::ui::messages::user_text_message::CONVERSATION_INTERRUPTED_MESSAGE;
    use crate::ui::theme::Theme;
    use crate::ui::virtual_scroll::VirtualScroll;
    use allthecodes_types::message::{
        ApiErrorInfo, AssistantMessage, CompactMetadata, ContentBlock, ImageSource, Message,
        MessageContent, MicrocompactMetadata, SystemMessage, SystemSubtype, ToolResultContent,
        UserMessage,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use serde_json::json;

    #[test]
    fn renders_file_edit_tool_preview_from_tool_use_result() {
        let hunk_lines = unified_hunk_lines_from_edit(
            "src/lib.rs",
            "fn main() {\n    old_call();\n}\n",
            "fn main() {\n    new_call();\n}\n",
        );
        let preview = json!({
            "kind": "file_edit",
            "path": "src/lib.rs",
            "output": "Successfully replaced 1 occurrence(s) in src/lib.rs",
            "replacements": 1,
            "hunk_lines": hunk_lines,
        })
        .to_string();
        let message = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_edit".to_string(),
                content: ToolResultContent::Text("The file was updated.".to_string()),
                is_error: false,
            }]),
            is_meta: true,
            tool_use_result: Some(preview),
            source_tool_assistant_uuid: None,
        });

        let rendered = render_single_message(&message, &Theme::default())
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Added 1 line, removed 1 line"));
        assert!(rendered.contains("file: src/lib.rs"));
        assert!(rendered.contains("old_call();"));
        assert!(rendered.contains("new_call();"));
    }

    #[test]
    fn messages_render_path_image_compact_and_interrupt_summaries() {
        let theme = Theme::default();
        let image = ImageSource {
            source_type: "base64".to_string(),
            media_type: "image/png".to_string(),
            data: "abcdef".to_string(),
        };
        let assistant = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_000,
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::ToolUse {
                    id: "toolu_read".to_string(),
                    name: "Read".to_string(),
                    input: json!({ "file_path": "src/main.rs" }),
                },
                ContentBlock::Image {
                    source: image.clone(),
                },
            ],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let rendered = lines_to_text(render_single_message(&assistant, &theme));

        assert!(rendered.contains("path=src/main.rs"));
        assert!(rendered.contains("[image: image/png, 6 chars]"));
        assert_eq!(
            message_primary_reference(&assistant).as_deref(),
            Some("path=src/main.rs")
        );
        assert!(message_copy_text(&assistant).contains("[image: image/png, 6 chars]"));

        let compact = Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_000,
            subtype: SystemSubtype::CompactBoundary {
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 12_000,
                    post_compact_token_count: 4_000,
                    preserved_segment: None,
                    pre_compact_discovered_tools: None,
                }),
            },
            content: String::new(),
        });
        assert!(lines_to_text(render_single_message(&compact, &theme))
            .contains("✻ Conversation compacted (ctrl+o for history)"));

        let interrupted = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("[Request interrupted by user]".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
        assert_eq!(
            lines_to_text(render_single_message(&interrupted, &theme)),
            "Interrupted by user"
        );

        let interrupted = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text(CONVERSATION_INTERRUPTED_MESSAGE.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
        assert_eq!(
            lines_to_text(render_single_message(&interrupted, &theme)),
            CONVERSATION_INTERRUPTED_MESSAGE
        );
    }

    #[test]
    fn assistant_bash_tool_use_renders_ran_block_with_description_and_command() {
        let tool_use_id = "toolu_bash".to_string();
        let assistant = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_000,
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: tool_use_id.clone(),
                name: "Bash".to_string(),
                input: json!({
                    "description": "Run Rust tests",
                    "command": "cargo test",
                }),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let result = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_001,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id,
                content: ToolResultContent::Text("ok".to_string()),
                is_error: false,
            }]),
            is_meta: true,
            tool_use_result: Some("ok".to_string()),
            source_tool_assistant_uuid: None,
        });
        let context =
            super::build_message_render_context(&[assistant.clone(), result], None, false);

        let rendered = lines_to_text(super::render_single_message_with_context(
            &assistant,
            0,
            &Theme::default(),
            80,
            &context,
        ));

        assert_eq!(rendered, "  ● Ran\n   ⎿  Bash(Run Rust tests, cargo test)");
    }

    #[test]
    fn assistant_tool_tasks_are_spaced_from_each_other_and_dialogue() {
        let first_tool_id = "toolu_bash".to_string();
        let second_tool_id = "toolu_read".to_string();
        let assistant = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_000,
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "before".to_string(),
                },
                ContentBlock::ToolUse {
                    id: first_tool_id.clone(),
                    name: "Bash".to_string(),
                    input: json!({"command": "cargo test"}),
                },
                ContentBlock::ToolUse {
                    id: second_tool_id.clone(),
                    name: "Read".to_string(),
                    input: json!({"file_path": "src/main.rs"}),
                },
                ContentBlock::Text {
                    text: "after".to_string(),
                },
            ],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let result = Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1_700_000_001,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolResult {
                    tool_use_id: first_tool_id,
                    content: ToolResultContent::Text("ok".to_string()),
                    is_error: false,
                },
                ContentBlock::ToolResult {
                    tool_use_id: second_tool_id,
                    content: ToolResultContent::Text("ok".to_string()),
                    is_error: false,
                },
            ]),
            is_meta: true,
            tool_use_result: Some("ok".to_string()),
            source_tool_assistant_uuid: None,
        });
        let context =
            super::build_message_render_context(&[assistant.clone(), result], None, false);

        let rendered = super::render_single_message_with_context(
            &assistant,
            0,
            &Theme::default(),
            80,
            &context,
        );
        let plain = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(plain[0], "before");
        assert!(plain[1].is_empty(), "dialogue and first task need a gap");
        assert!(plain[2].contains("● Ran"));
        assert!(plain[3].contains("⎿  Bash(cargo test)"));
        assert!(plain[4].is_empty(), "two task bullets need a gap");
        assert!(plain[5].contains("● Read"));
        assert!(plain[6].is_empty(), "last task and dialogue need a gap");
        assert_eq!(plain[7], "after");
    }

    #[test]
    fn user_prompt_background_fills_terminal_row() {
        let theme = Theme::default();
        let messages = vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })];
        let context = super::build_message_render_context(&messages, None, false);
        let mut vscroll = VirtualScroll::new();
        vscroll.ensure_up_to_date(&messages, 16, &theme, &context);
        let area = Rect::new(0, 0, 16, 3);
        let mut buffer = Buffer::empty(area);

        super::render_messages(
            &messages,
            area,
            &mut buffer,
            &theme,
            false,
            0,
            &vscroll,
            &context,
        );

        for y in 0..area.height {
            for x in 0..area.width {
                assert_eq!(buffer[(x, y)].bg, Color::Rgb(31, 35, 42));
            }
        }
    }

    #[test]
    fn assistant_api_error_runtime_path_uses_error_occurred_prefix() {
        let message = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "API error: Provider openrouter error (HTTP 429): rate limit exceeded"
                    .to_string(),
            }],
            usage: None,
            stop_reason: Some("error".to_string()),
            is_api_error_message: true,
            api_error: Some(
                "Provider openrouter error (HTTP 429): rate limit exceeded".to_string(),
            ),
            cost_usd: 0.0,
        });

        let rendered = lines_to_text(render_single_message(&message, &Theme::default()));

        assert!(rendered.contains(
            "Error occurred: API error: Provider openrouter error (HTTP 429): rate limit exceeded"
        ));
        assert!(!rendered.contains("Claude:"));
    }

    #[test]
    fn system_api_error_runtime_path_uses_error_occurred_prefix() {
        let message = Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            subtype: SystemSubtype::ApiError {
                retry_attempt: 1,
                max_retries: 3,
                retry_in_ms: 0,
                error: ApiErrorInfo {
                    status: Some(403),
                    message: "Provider proxy error (HTTP 403): forbidden".to_string(),
                },
            },
            content: String::new(),
        });

        let rendered = lines_to_text(render_single_message(&message, &Theme::default()));

        assert_eq!(
            rendered,
            "Error occurred: Provider proxy error (HTTP 403): forbidden"
        );
    }

    #[test]
    fn thinking_visibility_matches_prompt_and_transcript_modes() {
        let message = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "inspect files".to_string(),
                signature: None,
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });

        let prompt = render_single_message(&message, &Theme::default());
        assert!(prompt.is_empty());

        let transcript_context = super::build_message_render_context_with_options(
            std::slice::from_ref(&message),
            None,
            false,
            super::MessageRenderOptions {
                verbose: false,
                is_transcript_mode: true,
                show_all_in_transcript: true,
                thinking_animation_frame: None,
            },
        );
        let transcript = super::render_single_message_with_context(
            &message,
            0,
            &Theme::default(),
            80,
            &transcript_context,
        );
        let rendered = lines_to_text(transcript);
        assert!(rendered.contains("∴ Thinking"));
        assert!(rendered.contains("inspect files"));
    }

    #[test]
    fn thinking_animation_frame_reaches_prompt_rendering() {
        let message = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "inspect files".to_string(),
                signature: None,
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let context = super::build_message_render_context_with_options(
            std::slice::from_ref(&message),
            None,
            false,
            super::MessageRenderOptions {
                verbose: true,
                thinking_animation_frame: Some(1),
                ..super::MessageRenderOptions::default()
            },
        );

        let rendered =
            super::render_single_message_with_context(&message, 0, &Theme::default(), 80, &context);

        assert_eq!(
            rendered[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "∴ Thinking…"
        );
        assert!(
            rendered[0].spans.len() > 1,
            "animated label should be segmented into styled spans"
        );
    }

    #[test]
    fn thinking_animation_frame_only_applies_to_latest_thinking_block() {
        let theme = Theme::default();
        let first = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "first".to_string(),
                signature: None,
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let second = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "second".to_string(),
                signature: None,
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let messages = vec![first.clone(), second.clone()];
        let context = super::build_message_render_context_with_options(
            &messages,
            None,
            false,
            super::MessageRenderOptions {
                verbose: true,
                thinking_animation_frame: Some(1),
                ..super::MessageRenderOptions::default()
            },
        );

        let first_rendered =
            super::render_single_message_with_context(&first, 0, &theme, 80, &context);
        let second_rendered =
            super::render_single_message_with_context(&second, 1, &theme, 80, &context);

        assert_eq!(first_rendered[0].spans.len(), 1);
        assert_eq!(first_rendered[0].spans[0].style, theme.thinking);
        assert!(
            second_rendered[0].spans.len() > 1,
            "latest thinking label should animate"
        );
    }

    #[test]
    fn transcript_mode_does_not_animate_thinking_label() {
        let theme = Theme::default();
        let message = Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "inspect files".to_string(),
                signature: None,
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let context = super::build_message_render_context_with_options(
            std::slice::from_ref(&message),
            None,
            false,
            super::MessageRenderOptions {
                is_transcript_mode: true,
                show_all_in_transcript: true,
                thinking_animation_frame: Some(1),
                ..super::MessageRenderOptions::default()
            },
        );

        let rendered = super::render_single_message_with_context(&message, 0, &theme, 80, &context);

        assert_eq!(
            rendered[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "∴ Thinking…"
        );
        assert_eq!(rendered[0].spans.len(), 1);
        assert_eq!(rendered[0].spans[0].style, theme.thinking);
    }

    #[test]
    fn render_pipeline_renders_read_search_operations_and_hides_microcompact() {
        let read_id = "toolu_read".to_string();
        let grep_id = "toolu_grep".to_string();
        let messages = vec![
            Message::System(SystemMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                subtype: SystemSubtype::MicrocompactBoundary {
                    microcompact_metadata: Some(MicrocompactMetadata {
                        trigger: "test".to_string(),
                        pre_tokens: 100,
                        tokens_saved: 50,
                        compacted_tool_ids: vec![read_id.clone()],
                        cleared_attachment_uuids: Vec::new(),
                    }),
                },
                content: String::new(),
            }),
            Message::Assistant(AssistantMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 1,
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: read_id.clone(),
                        name: "Read".to_string(),
                        input: json!({ "file_path": "src/lib.rs" }),
                    },
                    ContentBlock::ToolUse {
                        id: grep_id.clone(),
                        name: "Grep".to_string(),
                        input: json!({ "pattern": "fn main" }),
                    },
                ],
                usage: None,
                stop_reason: None,
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.0,
            }),
            Message::User(UserMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 2,
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: read_id,
                    content: ToolResultContent::Text("ok".to_string()),
                    is_error: false,
                }]),
                is_meta: true,
                tool_use_result: Some("ok".to_string()),
                source_tool_assistant_uuid: None,
            }),
            Message::User(UserMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 3,
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: grep_id,
                    content: ToolResultContent::Text("match".to_string()),
                    is_error: false,
                }]),
                is_meta: true,
                tool_use_result: Some("match".to_string()),
                source_tool_assistant_uuid: None,
            }),
        ];

        let context = super::build_message_render_context_with_options(
            &messages,
            None,
            false,
            super::MessageRenderOptions::default(),
        );
        let rendered = context
            .renderable_messages()
            .iter()
            .flat_map(|message| {
                super::render_renderable_message_with_context(
                    message,
                    0,
                    &Theme::default(),
                    80,
                    &context,
                )
            })
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!rendered.contains("microcompact"));
        assert!(rendered.contains("Read src/lib.rs"));
        assert!(rendered.contains("Search \"fn main\""));
    }

    #[test]
    fn common_shell_and_edit_tool_uses_are_grouped() {
        let bash_first_id = "toolu_bash_1".to_string();
        let bash_second_id = "toolu_bash_2".to_string();
        let messages = vec![
            Message::Assistant(AssistantMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 1,
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: bash_first_id.clone(),
                        name: "Bash".to_string(),
                        input: json!({ "command": "cargo test" }),
                    },
                    ContentBlock::ToolUse {
                        id: bash_second_id.clone(),
                        name: "Bash".to_string(),
                        input: json!({ "command": "cargo build" }),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_pwsh_1".to_string(),
                        name: "PowerShell".to_string(),
                        input: json!({ "command": "Get-ChildItem" }),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_pwsh_2".to_string(),
                        name: "PowerShell".to_string(),
                        input: json!({ "command": "Get-Location" }),
                    },
                ],
                usage: None,
                stop_reason: None,
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.0,
            }),
            Message::User(UserMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 2,
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: bash_first_id,
                    content: ToolResultContent::Text("first bash output".to_string()),
                    is_error: false,
                }]),
                is_meta: true,
                tool_use_result: Some("first bash output".to_string()),
                source_tool_assistant_uuid: None,
            }),
            Message::User(UserMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 3,
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: bash_second_id,
                    content: ToolResultContent::Text("second bash output".to_string()),
                    is_error: false,
                }]),
                is_meta: true,
                tool_use_result: Some("second bash output".to_string()),
                source_tool_assistant_uuid: None,
            }),
        ];

        let rendered = render_pipeline_text(&messages, super::MessageRenderOptions::default());

        assert!(rendered.contains("cargo test"));
        assert!(rendered.contains("cargo build"));
        assert!(rendered.contains("2 commands"));
        assert!(rendered.contains("first bash output"));
        assert!(rendered.contains("second bash output"));
    }

    #[test]
    fn common_edit_tool_uses_are_grouped() {
        for tool_name in [
            "Edit",
            "Write",
            "FileEdit",
            "FileWrite",
            "MultiEdit",
            "NotebookEdit",
        ] {
            let path_field = if tool_name == "NotebookEdit" {
                "notebook_path"
            } else {
                "file_path"
            };
            let messages = vec![Message::Assistant(AssistantMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 1,
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: format!("toolu_{tool_name}_1"),
                        name: tool_name.to_string(),
                        input: json!({ path_field: "src/lib.rs" }),
                    },
                    ContentBlock::ToolUse {
                        id: format!("toolu_{tool_name}_2"),
                        name: tool_name.to_string(),
                        input: json!({ path_field: "src/main.rs" }),
                    },
                ],
                usage: None,
                stop_reason: None,
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.0,
            })];

            let rendered = render_pipeline_text(&messages, super::MessageRenderOptions::default());

            let expected = if matches!(tool_name, "Write" | "FileWrite") {
                "2 creates"
            } else {
                "2 edits"
            };
            assert!(
                rendered.contains(expected),
                "{tool_name} should render as {expected}, got:\n{rendered}"
            );
        }
    }

    #[test]
    fn verbose_mode_keeps_common_tool_uses_ungrouped() {
        let messages = vec![Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::ToolUse {
                    id: "toolu_bash_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "cargo test" }),
                },
                ContentBlock::ToolUse {
                    id: "toolu_bash_2".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "cargo build" }),
                },
            ],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })];

        let rendered = render_pipeline_text(
            &messages,
            super::MessageRenderOptions {
                verbose: true,
                thinking_animation_frame: None,
                ..super::MessageRenderOptions::default()
            },
        );

        assert!(!rendered.contains("2 Bash calls"));
        assert!(rendered.contains("Bash(cargo test)"));
        assert!(rendered.contains("Bash(cargo build)"));
    }

    #[test]
    fn todo_write_tool_uses_are_not_grouped_away() {
        let messages = vec![Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::ToolUse {
                    id: "toolu_todo_1".to_string(),
                    name: "TodoWrite".to_string(),
                    input: json!({
                        "todos": [
                            { "content": "Inspect app state", "status": "completed" },
                            { "content": "Patch todo renderer", "status": "in_progress" }
                        ]
                    }),
                },
                ContentBlock::ToolUse {
                    id: "toolu_todo_2".to_string(),
                    name: "TodoWrite".to_string(),
                    input: json!({
                        "todos": [
                            { "content": "Run focused tests", "status": "pending" }
                        ]
                    }),
                },
            ],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })];

        let context = super::build_message_render_context_with_options(
            &messages,
            None,
            false,
            super::MessageRenderOptions::default(),
        );
        let rendered = context
            .renderable_messages()
            .iter()
            .flat_map(|message| {
                super::render_renderable_message_with_context(
                    message,
                    0,
                    &Theme::default(),
                    80,
                    &context,
                )
            })
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!rendered.contains("TodoWrite calls"));
        assert!(rendered.contains("[x] Inspect app state"));
        assert!(rendered.contains("[*] Patch todo renderer"));
        assert!(rendered.contains("[ ] Run focused tests"));
    }

    #[test]
    fn tool_input_summary_prefers_primary_input_and_abbreviates_json() {
        let input = json!({
            "file_path": "src/main.rs",
            "content": "abcdefghijklmnopqrstuvwxyz"
        });

        let summary = super::copy_text::tool_input_summary("Write", &input, 28);

        assert!(summary.starts_with("path=src/main.rs "));
        assert!(summary.ends_with("..."));
    }

    fn render_pipeline_text(messages: &[Message], options: super::MessageRenderOptions) -> String {
        let context =
            super::build_message_render_context_with_options(messages, None, false, options);
        context
            .renderable_messages()
            .iter()
            .flat_map(|message| {
                super::render_renderable_message_with_context(
                    message,
                    0,
                    &Theme::default(),
                    80,
                    &context,
                )
            })
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn lines_to_text(lines: Vec<ratatui::text::Line<'_>>) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
