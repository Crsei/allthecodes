use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::App;
use crate::ui::agents::agents_menu::AgentsMenuState;
use crate::ui::bottom_pane::BottomPaneHeights;
use crate::ui::command_palette::CommandPalette;
use crate::ui::command_surface::CommandSurface;
use crate::ui::history_search_dialog::HistorySearchDialog;
use crate::ui::messages::render_messages;
use crate::ui::notifications::in_app::{NotificationPriority, NotificationTone};
use crate::ui::overlays::{render_prompt_adjacent_dialog_lines, CenteredOverlayFrame};
use crate::ui::panel_layout::PanelSizePreset;
use crate::ui::prompt_input::PromptInputRenderContext;
use crate::ui::theme::{Theme, ThemeColors};
use crate::ui::transcript::{self, TranscriptInputMode, ViewMode};
use crate::ui::welcome;

/// Upper cap on the number of stdout lines the status-line runner is
/// allowed to take up. Arbitrary but small so a runaway script can't
/// eat the messages pane.
const STATUS_LINE_MAX_LINES: usize = 3;
const MESSAGE_BOTTOM_GAP_HEIGHT: u16 = 1;

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();
        if size.width < 10 || size.height < 4 {
            return;
        }
        self.session_scrollbar = None;
        self.message_area = None;
        self.prompt_area = None;

        if self.workspace_trust_pending {
            render_workspace_trust_prompt(
                size,
                frame.buffer_mut(),
                &self.cwd,
                self.workspace_trust_selection,
            );
            self.capture_render_snapshot(frame);
            return;
        }

        // Transcript / focus modes have a different chrome; dispatch
        // before we compute the prompt-mode layout.
        if self.view_mode.is_transcript_like() {
            self.render_transcript(frame, size);
            self.capture_render_snapshot(frame);
            return;
        }

        // Kick the status-line runner before computing layout so this
        // frame already has a chance to show a refreshed output. The
        // runner throttles refreshes internally.
        self.trigger_status_refresh();
        let status_output = self.status_line_runner.latest();
        let custom_lines: Vec<String> =
            if status_output.is_usable() && self.status_line_settings.is_command_mode() {
                status_output.lines(STATUS_LINE_MAX_LINES)
            } else {
                Vec::new()
            };

        let current_notification = self.current_notification();
        let immediate_notification = current_notification
            .is_some_and(|notification| notification.priority == NotificationPriority::Immediate);
        let spinner_height = if self.is_streaming && !immediate_notification {
            1u16
        } else {
            0
        };
        let suggestion_height =
            if !self.is_streaming && self.suggestions.is_some() && !immediate_notification {
                1u16
            } else {
                0
            };
        let command_palette_height = self
            .command_palette
            .preferred_height()
            .min(size.height.saturating_sub(4));
        let completion_popup_height = self.completion_popup_height();
        let cwd_path = std::path::Path::new(&self.cwd);
        let command_arg_help_height = 0;
        let paste_notice_height =
            u16::from(self.prompt.large_paste_notice().is_some() && !immediate_notification);
        let notification_height = u16::from(current_notification.is_some());
        let agent_footer_height = if self.agent_footer_visible() && !immediate_notification {
            self.agent_footer_height()
        } else {
            0
        };
        let input_height = 3u16;
        let status_height = if custom_lines.is_empty() {
            1u16
        } else {
            custom_lines.len().min(STATUS_LINE_MAX_LINES) as u16
        };
        let bottom_pane = BottomPaneHeights {
            spinner: spinner_height,
            suggestions: suggestion_height,
            paste_notice: paste_notice_height,
            input: input_height,
            completion_popup: completion_popup_height,
            command_palette: command_palette_height,
            command_arg_help: command_arg_help_height,
            notification: notification_height,
            agent_footer: agent_footer_height,
            status: status_height,
        };
        let bottom_height = bottom_pane.total();
        let message_bottom_gap_height = u16::from(bottom_height > 0) * MESSAGE_BOTTOM_GAP_HEIGHT;
        let max_content_height = size
            .height
            .saturating_sub(bottom_height.saturating_add(message_bottom_gap_height));
        let content_height = if self.show_welcome {
            welcome::welcome_height_for(size.width).min(max_content_height)
        } else {
            let message_render_context =
                super::super::messages::build_message_render_context_with_options(
                    &self.messages,
                    self.selected_message,
                    self.selected_message_expanded,
                    super::super::messages::MessageRenderOptions {
                        verbose: self.verbose,
                        is_transcript_mode: false,
                        show_all_in_transcript: false,
                        thinking_animation_frame: self.thinking_animation_frame(),
                    },
                );
            self.vscroll.ensure_up_to_date(
                &self.messages,
                size.width,
                &self.theme,
                &message_render_context,
            );
            self.vscroll
                .total_visual_lines()
                .min(max_content_height as usize) as u16
        };

        let chunks = Layout::vertical([
            Constraint::Length(content_height),
            Constraint::Length(message_bottom_gap_height),
            Constraint::Length(bottom_height),
            Constraint::Min(0),
        ])
        .split(size);

        let message_area = chunks[0];
        let bottom_area = chunks[2];
        self.message_area = Some(message_area);

        if self.show_welcome {
            // Welcome screen
            welcome::render_welcome(
                message_area,
                frame.buffer_mut(),
                env!("CARGO_PKG_VERSION"),
                &self.model_name,
                &self.session_id,
                &self.cwd,
            );
        } else {
            // Messages (virtual scroll)
            let message_render_context =
                super::super::messages::build_message_render_context_with_options(
                    &self.messages,
                    self.selected_message,
                    self.selected_message_expanded,
                    super::super::messages::MessageRenderOptions {
                        verbose: self.verbose,
                        is_transcript_mode: false,
                        show_all_in_transcript: false,
                        thinking_animation_frame: self.thinking_animation_frame(),
                    },
                );
            self.vscroll.ensure_up_to_date(
                &self.messages,
                message_area.width,
                &self.theme,
                &message_render_context,
            );
            let mut total = self.vscroll.total_visual_lines();
            let (message_body_area, scrollbar_area) =
                split_session_scrollbar_area(message_area, total);
            if message_body_area.width != message_area.width {
                self.vscroll.ensure_up_to_date(
                    &self.messages,
                    message_body_area.width,
                    &self.theme,
                    &message_render_context,
                );
                total = self.vscroll.total_visual_lines();
            }
            let max_scroll = total.saturating_sub(message_area.height as usize);
            if self.scroll_offset > max_scroll {
                self.scroll_offset = max_scroll;
            }

            render_messages(
                &self.messages,
                message_body_area,
                frame.buffer_mut(),
                &self.theme,
                self.is_streaming,
                self.scroll_offset,
                &self.vscroll,
                &message_render_context,
            );
            if let Some(scrollbar_area) = scrollbar_area {
                self.session_scrollbar = Some(super::SessionScrollbarState {
                    area: scrollbar_area,
                    total_lines: total,
                });
                render_session_scrollbar(
                    scrollbar_area,
                    frame.buffer_mut(),
                    total,
                    self.scroll_offset,
                    &self.theme,
                );
            }
        }

        // Bottom area: spinner + suggestions + paste_notice + completion_popup + palette + arg_help + input + notification + agent_footer + status
        let has_suggestions = suggestion_height > 0;
        let bottom_chunks = bottom_pane.split(bottom_area);
        self.prompt_area = Some(bottom_chunks.input);

        if self.is_streaming && bottom_chunks.spinner.height > 0 {
            self.spinner_state
                .render(bottom_chunks.spinner, frame.buffer_mut(), &self.theme);
        }

        if has_suggestions {
            self.render_suggestions(bottom_chunks.suggestions, frame.buffer_mut());
        }

        if paste_notice_height > 0 {
            self.render_paste_notice(bottom_chunks.paste_notice, frame.buffer_mut());
        }

        // Render completion popup (when active and command palette is not active)
        if self.completion_state.active && !self.command_palette.active() {
            self.render_completion_popup(bottom_chunks.completion_popup, frame.buffer_mut());
        }

        self.command_palette.render(
            bottom_chunks.command_palette,
            frame.buffer_mut(),
            &self.theme,
        );

        let argument_hint = CommandPalette::argument_hint(&self.prompt.input, cwd_path);
        let placeholder = self.prompt_placeholder();
        let mode_indicator = self.prompt_mode_indicator();
        self.prompt.render_with_context(
            bottom_chunks.input,
            frame.buffer_mut(),
            &self.theme,
            PromptInputRenderContext {
                hint: argument_hint.as_deref(),
                placeholder: Some(placeholder),
                mode_indicator: Some(mode_indicator),
            },
        );

        if notification_height > 0 {
            self.render_notification(bottom_chunks.notification, frame.buffer_mut());
        }

        if agent_footer_height > 0 {
            self.render_agent_footer(bottom_chunks.agent_footer, frame.buffer_mut());
        }

        self.render_status_bar(bottom_chunks.status, frame.buffer_mut(), &custom_lines);

        if let Some(ref surface) = self.command_surface {
            render_command_surface_overlay(
                surface,
                size,
                bottom_chunks.input,
                frame.buffer_mut(),
                self.design_theme_provider.colors(),
            );
        }

        if let Some(ref dialog) = self.history_search_dialog {
            render_history_search_overlay(
                dialog,
                size,
                bottom_chunks.input,
                frame.buffer_mut(),
                &self.theme,
                self.design_theme_provider.colors(),
            );
        }

        let current_thread_id = self.current_agent_thread_id().to_string();
        if let Some(ref mut dialog) = self.agent_tree_dialog {
            render_agent_tree_overlay(
                dialog,
                &self.agent_nav,
                &current_thread_id,
                size,
                bottom_chunks.input,
                frame.buffer_mut(),
                &self.theme,
                self.design_theme_provider.colors(),
            );
        }

        if let Some(ref dialog) = self.permission_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        if let Some(ref dialog) = self.question_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        if let Some(ref dialog) = self.bypass_permissions_mode_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        self.capture_render_snapshot(frame);
    }

    fn capture_render_snapshot(&mut self, frame: &mut Frame) {
        self.last_render_snapshot = Some(buffer_to_plain_text(frame.buffer_mut()));
    }

    fn render_suggestions(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        if let Some(suggestions) = &self.suggestions {
            let hint: String = suggestions
                .iter()
                .take(3)
                .enumerate()
                .map(|(i, s)| format!("[{}{}] {}", s.category.icon(), i + 1, s.text))
                .collect::<Vec<_>>()
                .join("  ");
            let line = Line::from(Span::styled(
                hint,
                ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
            ));
            buf.set_line(area.x, area.y, &line, area.width);
        }
    }

    fn render_paste_notice(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        if let Some(notice) = self.prompt.large_paste_notice() {
            let line = Line::from(vec![
                Span::styled(" paste ", self.theme.info),
                Span::styled(notice.to_string(), self.theme.dim),
            ]);
            buf.set_line(area.x, area.y, &line, area.width);
        }
    }

    fn render_notification(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        let Some(notification) = self.current_notification() else {
            return;
        };
        let line = if let Some(spans) = notification.rendered.as_ref() {
            Line::from(spans.clone())
        } else {
            Line::from(vec![
                Span::styled(" notice ", self.theme.info),
                Span::styled(
                    notification.text.as_str(),
                    notification_style(notification.tone, &self.theme),
                ),
            ])
        };
        buf.set_line(area.x, area.y, &line, area.width);
    }

    fn render_agent_footer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 || !self.agent_footer_visible() {
            return;
        }

        for (idx, text) in self
            .agent_footer_lines()
            .into_iter()
            .take(area.height as usize)
            .enumerate()
        {
            let line = if idx == 0 {
                Line::from(vec![
                    Span::styled(" agents ", self.theme.info),
                    Span::styled(text, self.theme.dim),
                ])
            } else {
                Line::from(Span::styled(text, self.theme.dim))
            };
            buf.set_line(area.x, area.y + idx as u16, &line, area.width);
        }
    }

    fn prompt_placeholder(&self) -> &'static str {
        if self.is_streaming {
            "Type next message; Tab queues it"
        } else if self.prompt.input.starts_with('/') || self.command_palette.active() {
            "Type a command"
        } else if self.vim.enabled {
            "Press i to insert, / for commands, Ctrl+R for history"
        } else {
            "Message allthecodes, / for commands"
        }
    }

    fn prompt_mode_indicator(&self) -> &'static str {
        if self.is_streaming {
            "BUSY"
        } else if self.prompt.input.starts_with('/') || self.command_palette.active() {
            "CMD"
        } else if self.vim.enabled {
            self.vim.mode.indicator()
        } else {
            "INS"
        }
    }

    fn render_status_bar(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        custom_lines: &[String],
    ) {
        if area.height == 0 {
            return;
        }

        // 1. Custom scriptable status-line (issue #11); when present, take
        //    full priority over the built-in footer. Padding from settings.
        if !custom_lines.is_empty() {
            let padding = self.status_line_settings.padding.unwrap_or(0) as usize;
            let pad_str: String = " ".repeat(padding);
            for (i, text) in custom_lines.iter().enumerate() {
                if (i as u16) >= area.height {
                    break;
                }
                let line = Line::from(vec![Span::styled(
                    format!("{}{}", pad_str, text),
                    self.theme.dim,
                )]);
                buf.set_line(area.x, area.y + i as u16, &line, area.width);
            }
            return;
        }

        // 2. Built-in default footer; keep the prompt-adjacent chrome quiet.
        let mut parts = Vec::new();
        if self.is_streaming && !self.prompt.input.trim().is_empty() {
            parts.push("tab to queue message".to_string());
        }
        if self.queued_count() > 0 {
            parts.push(format!("{} queued", self.queued_count()));
        }
        if let Some(goal) = &self.active_goal {
            parts.push(format!(
                "goal={} {} {} {}t",
                render_goal_status(&goal.status),
                truncate_status_text(&goal.objective, 28),
                format_status_duration(goal.time_used_seconds),
                goal.tokens_used
            ));
        }
        if !self.model_name.is_empty() {
            parts.push(self.model_name.clone());
        }
        if !self.cwd.is_empty() {
            parts.push(self.cwd.clone());
        }

        let status_text = format!(" {}", parts.join(" | "));
        let line = Line::from(vec![Span::styled(status_text, self.theme.dim)]);
        buf.set_line(area.x, area.y, &line, area.width);
    }

    // Transcript rendering (issue #12)

    fn render_transcript(&mut self, frame: &mut Frame, size: Rect) {
        if matches!(self.view_mode, ViewMode::Focus) {
            self.render_focus_view(frame, size);
            return;
        }

        // Focus mode hides all chrome and uses the full height for body;
        // Transcript mode reserves 1 line for header + 1 for footer.
        let chrome = 1u16;
        let header_height = chrome;
        let footer_height = chrome;
        let body_height = size.height.saturating_sub(header_height + footer_height);

        let rows = Layout::vertical([
            Constraint::Length(header_height),
            Constraint::Length(body_height),
            Constraint::Length(footer_height),
        ])
        .split(size);

        let body_area = rows[1];
        self.message_area = Some(body_area);
        self.prompt_area = None;

        // Ensure the virtual-scroll cache matches the body width. Sharing
        // `vscroll` with prompt mode is fine because both invalidate on
        // width change.
        let message_render_context =
            super::super::messages::build_message_render_context_with_options(
                &self.messages,
                self.selected_message,
                self.selected_message_expanded,
                super::super::messages::MessageRenderOptions {
                    verbose: self.verbose,
                    is_transcript_mode: true,
                    show_all_in_transcript: true,
                    thinking_animation_frame: None,
                },
            );
        self.vscroll.ensure_up_to_date(
            &self.messages,
            body_area.width,
            &self.theme,
            &message_render_context,
        );
        let mut total = self.vscroll.total_visual_lines();
        let (message_body_area, scrollbar_area) = split_session_scrollbar_area(body_area, total);
        if message_body_area.width != body_area.width {
            self.vscroll.ensure_up_to_date(
                &self.messages,
                message_body_area.width,
                &self.theme,
                &message_render_context,
            );
            total = self.vscroll.total_visual_lines();
        }
        let max_scroll = total.saturating_sub(body_area.height as usize);
        if self.transcript_state.scroll_offset > max_scroll {
            self.transcript_state.scroll_offset = max_scroll;
        }

        render_messages(
            &self.messages,
            message_body_area,
            frame.buffer_mut(),
            &self.theme,
            self.is_streaming,
            self.transcript_state.scroll_offset,
            &self.vscroll,
            &message_render_context,
        );
        if let Some(scrollbar_area) = scrollbar_area {
            self.session_scrollbar = Some(super::SessionScrollbarState {
                area: scrollbar_area,
                total_lines: total,
            });
            render_session_scrollbar(
                scrollbar_area,
                frame.buffer_mut(),
                total,
                self.transcript_state.scroll_offset,
                &self.theme,
            );
        }

        if header_height > 0 {
            self.render_transcript_header(rows[0], frame.buffer_mut());
        }
        if footer_height > 0 {
            self.render_transcript_footer(rows[2], frame.buffer_mut());
        }
    }

    fn render_focus_view(&mut self, frame: &mut Frame, size: Rect) {
        let focus = transcript::build_focus_view(&self.messages);
        let mut lines = Vec::new();

        if let Some(prompt) = focus.prompt {
            lines.push(Line::from(vec![Span::styled(
                "Prompt",
                self.theme.assistant_name,
            )]));
            for body_line in prompt.body.lines() {
                lines.push(Line::from(body_line.to_string()));
            }
        }

        if let Some(summary) = focus.tool_summary {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![Span::styled(
                "Tool Summary",
                self.theme.tool_name,
            )]));
            for body_line in summary.lines() {
                lines.push(Line::from(body_line.to_string()));
            }
        }

        if let Some(response) = focus.response {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![Span::styled(
                "Response",
                self.theme.user_name,
            )]));
            for body_line in response.body.lines() {
                lines.push(Line::from(body_line.to_string()));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "No focused transcript content yet.",
                self.theme.dim,
            )]));
        }

        for (row, line) in lines.iter().enumerate().take(size.height as usize) {
            frame
                .buffer_mut()
                .set_line(size.x, size.y + row as u16, line, size.width);
        }
    }

    fn render_transcript_header(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let total = self.messages.len();
        let mut parts = vec![format!(
            "\u{2500}\u{2500} {} \u{00b7} {} messages",
            self.view_mode.label(),
            total
        )];
        if matches!(
            self.transcript_state.input_mode,
            TranscriptInputMode::Search
        ) {
            parts.push(format!("search: \"{}\"", self.transcript_state.query));
        } else if !self.transcript_state.query.is_empty() {
            let total = self.transcript_state.matches.len();
            let idx = self.transcript_state.focused.map(|i| i + 1).unwrap_or(0);
            parts.push(format!(
                "search: \"{}\" ({}/{})",
                self.transcript_state.query, idx, total
            ));
        }
        let text = parts.join(" \u{00b7} ");
        let line = Line::from(vec![Span::styled(text, self.theme.dim)]);
        buf.set_line(area.x, area.y, &line, area.width);
    }

    fn render_transcript_footer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let text = match self.transcript_state.input_mode {
            TranscriptInputMode::Search => {
                " [Enter] commit \u{00b7} [Esc] cancel \u{00b7} type to extend query".to_string()
            }
            TranscriptInputMode::Normal => concat!(
                " [Esc/q] prompt \u{00b7} [Ctrl+O] cycle \u{00b7} [/] search ",
                "\u{00b7} [n]/[N] next/prev \u{00b7} [e] editor ",
                "\u{00b7} [g]/[G] top/bottom"
            )
            .to_string(),
        };
        let line = Line::from(vec![Span::styled(text, self.theme.dim)]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

fn truncate_status_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }
    if max_chars <= 3 {
        return trimmed.chars().take(max_chars).collect();
    }
    let mut out = trimmed
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn render_goal_status(status: &str) -> &str {
    match status {
        "active" => "active",
        "paused" => "paused",
        "blocked" => "blocked",
        "usage_limited" => "usage-limited",
        "budget_limited" => "budget-limited",
        "complete" | "completed" => "complete",
        other => other,
    }
}

fn format_status_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h{minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m{secs}s")
    } else {
        format!("{secs}s")
    }
}

fn buffer_to_plain_text(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in area.y..area.y.saturating_add(area.height) {
        let mut line = String::new();
        for x in area.x..area.x.saturating_add(area.width) {
            line.push_str(buf.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn notification_style(tone: NotificationTone, theme: &Theme) -> Style {
    match tone {
        NotificationTone::Info => theme.info,
        NotificationTone::Warning => theme.warning,
        NotificationTone::Error => theme.error,
        NotificationTone::Dim => theme.dim,
    }
}
fn render_workspace_trust_prompt(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    cwd: &str,
    selected: usize,
) {
    let line_width = area.width.clamp(40, 120) as usize;
    let separator = "\u{2500}".repeat(line_width);
    let yes_marker = if selected == 0 { "\u{276f}" } else { " " };
    let no_marker = if selected == 1 { "\u{276f}" } else { " " };

    let lines = vec![
        Line::from(Span::styled(
            separator,
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Accessing workspace:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(" {}", cwd),
            Style::default().fg(Color::LightBlue),
        )),
        Line::from(""),
        Line::from(
            " Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source",
        ),
        Line::from(
            " project, or work from your team). If not, take a moment to review what's in this folder first.",
        ),
        Line::from(""),
        Line::from(" allthecodes can read, edit, and execute files here."),
        Line::from(""),
        Line::from(Span::styled(
            " Security guide",
            Style::default().fg(Color::LightBlue),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!(" {} ", yes_marker),
                Style::default().fg(if selected == 0 {
                    Color::Green
                } else {
                    Color::White
                }),
            ),
            Span::raw("1. Yes, I trust this folder"),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" {} ", no_marker),
                Style::default().fg(if selected == 1 {
                    Color::Red
                } else {
                    Color::White
                }),
            ),
            Span::raw("2. No, exit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Enter to confirm \u{00b7} Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

fn split_session_scrollbar_area(area: Rect, total_lines: usize) -> (Rect, Option<Rect>) {
    if area.width <= 1 || total_lines <= area.height as usize {
        return (area, None);
    }
    (
        Rect {
            width: area.width - 1,
            ..area
        },
        Some(Rect {
            x: area.x + area.width - 1,
            width: 1,
            ..area
        }),
    )
}

fn render_session_scrollbar(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    total_lines: usize,
    scroll_offset: usize,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let viewport = area.height as usize;
    let max_scroll = total_lines.saturating_sub(viewport);
    if max_scroll == 0 {
        return;
    }

    let track_style = theme.dim;
    let thumb_style = theme.selected;
    let top_active = scroll_offset > 0;
    let bottom_active = scroll_offset < max_scroll;
    buf.set_string(
        area.x,
        area.y,
        if top_active { "▲" } else { "△" },
        track_style,
    );
    if area.height == 1 {
        return;
    }
    buf.set_string(
        area.x,
        area.y + area.height - 1,
        if bottom_active { "▼" } else { "▽" },
        track_style,
    );
    if area.height <= 2 {
        return;
    }

    let track_height = area.height.saturating_sub(2) as usize;
    let thumb_height = ((track_height * viewport).max(1) / total_lines.max(1))
        .max(1)
        .min(track_height);
    let travel = track_height.saturating_sub(thumb_height);
    let thumb_offset = if max_scroll == 0 {
        0
    } else {
        scroll_offset.min(max_scroll) * travel / max_scroll
    };

    for row in 0..track_height {
        let y = area.y + 1 + row as u16;
        let in_thumb = row >= thumb_offset && row < thumb_offset + thumb_height;
        buf.set_string(
            area.x,
            y,
            if in_thumb { "█" } else { "│" },
            if in_thumb { thumb_style } else { track_style },
        );
    }
}

fn render_command_surface_overlay(
    surface: &CommandSurface,
    area: Rect,
    prompt_area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    colors: &ThemeColors,
) {
    let text = surface.render();
    let body = text
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    render_prompt_adjacent_dialog_lines(
        CenteredOverlayFrame::with_preset(surface.title(), PanelSizePreset::CommandSurface)
            .color("accent"),
        body,
        area,
        prompt_area,
        buf,
        colors,
        Style::default().fg(Color::White).bg(Color::Rgb(8, 10, 14)),
    );
}

fn render_history_search_overlay(
    dialog: &HistorySearchDialog,
    area: Rect,
    prompt_area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    theme: &Theme,
    colors: &ThemeColors,
) {
    let spec = PanelSizePreset::HistorySearch.spec();
    if area.width < spec.min_width || area.height < spec.min_height {
        return;
    }

    let overlay = spec
        .resolve_prompt_adjacent_rect(area, prompt_area, spec.max_height)
        .unwrap_or(Rect::new(area.x, area.y, area.width, area.height));
    let text = dialog.render(
        overlay.width.saturating_sub(4) as usize,
        overlay.height.saturating_sub(3) as usize,
    );
    let body = text
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    render_prompt_adjacent_dialog_lines(
        CenteredOverlayFrame::with_preset("History Search", PanelSizePreset::HistorySearch)
            .color("accent"),
        body,
        area,
        prompt_area,
        buf,
        colors,
        theme.dim,
    );
}

fn render_agent_tree_overlay(
    dialog: &mut super::agent_tree_dialog::AgentTreeDialog,
    state: &super::agent_navigation::AgentNavigationState,
    current_thread_id: &str,
    area: Rect,
    prompt_area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    theme: &Theme,
    colors: &ThemeColors,
) {
    let spec = PanelSizePreset::AgentTree.spec();
    if area.width < spec.min_width || area.height < spec.min_height {
        return;
    }

    let mut lines = dialog.render_lines(state, current_thread_id, theme);
    let total = state.thread_count();
    let menu =
        AgentsMenuState::default_with_counts(total, 1.min(total), 0, total.saturating_sub(1))
            .render();
    if let Some(summary_row) = menu.lines().nth(1) {
        lines.insert(
            1,
            Line::from(Span::styled(format!("Menu: {summary_row}"), theme.dim)),
        );
    }
    render_prompt_adjacent_dialog_lines(
        CenteredOverlayFrame::with_preset("Agent Threads", PanelSizePreset::AgentTree)
            .color("accent"),
        lines,
        area,
        prompt_area,
        buf,
        colors,
        theme.dim,
    );
}

// ---------------------------------------------------------------------------
// Completion popup rendering helpers
// ---------------------------------------------------------------------------

/// Calculate visible window start for a scrollable list.
fn visible_window_start(total: usize, selected: usize, max_rows: usize) -> usize {
    if max_rows == 0 || total <= max_rows {
        return 0;
    }

    selected.saturating_add(1).saturating_sub(max_rows)
}

/// Truncate a string to a max character width, adding "..." if truncated.
fn truncate(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        format!("{}...", chars[..max_width - 3].iter().collect::<String>())
    }
}

impl App {
    /// Preferred height for the completion popup (0 if not active).
    pub(super) fn completion_popup_height(&self) -> u16 {
        if !self.completion_state.active || self.command_palette.active() {
            return 0;
        }
        let count = self.completion_state.items.len().min(8);
        if count == 0 {
            return 0;
        }
        // Header + separator line + item rows + footer hint
        (count as u16).min(8) + 2
    }

    /// Render the active completion popup.
    fn render_completion_popup(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height < 3 || area.width < 20 {
            return;
        }

        let state = &self.completion_state;
        if state.items.is_empty() {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Completions ")
            .border_style(self.theme.dim);
        let inner = block.inner(area);
        block.render(area, buf);

        let mut lines: Vec<Line<'_>> = Vec::new();

        // Header line
        lines.push(Line::from(vec![Span::styled(
            format!(
                " {} items | Tab:accept | Shift+Tab:prev | Enter:fill ",
                state.items.len()
            ),
            self.theme.dim,
        )]));

        // Determine visible window
        let max_visible = (inner.height as usize).saturating_sub(2).max(1);
        let visible_start = visible_window_start(state.items.len(), state.selected, max_visible);

        for (idx, item) in state
            .items
            .iter()
            .enumerate()
            .skip(visible_start)
            .take(max_visible)
        {
            let selected = idx == state.selected;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let kind_label = item.kind.label();
            let source = item.source_group.unwrap_or(kind_label);

            let detail = if selected {
                item.detail.as_deref().unwrap_or("")
            } else {
                ""
            };

            lines.push(Line::from(vec![
                Span::styled(
                    " \u{276f} ".to_string(),
                    if selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(format!("{:<30}", truncate(&item.label, 28)), style),
                Span::styled(format!(" {} ", source), self.theme.dim),
                if !detail.is_empty() {
                    Span::styled(detail, self.theme.dim)
                } else {
                    Span::raw("")
                },
            ]));
        }

        Paragraph::new(lines).render(inner, buf);
    }
}
