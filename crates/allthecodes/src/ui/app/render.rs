use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::App;
use crate::ui::agents::agents_menu::AgentsMenuState;
use crate::ui::app::ProactiveUiStatus;
use crate::ui::bottom_pane::BottomPaneHeights;
use crate::ui::command_palette::CommandPalette;
use crate::ui::command_surface::{CommandSurface, CommandSurfaceCursorAnchor};
use crate::ui::history_search_dialog::HistorySearchDialog;
use crate::ui::messages::{
    render_messages, task_list_content::render_task_list_lines, MessageListViewModel,
    MessageRenderOptions,
};
use crate::ui::notifications::in_app::{NotificationPriority, NotificationTone};
use crate::ui::overlays::{
    render_prompt_adjacent_dialog_lines, render_prompt_adjacent_lines, CenteredOverlayFrame,
};
use crate::ui::panel_layout::PanelSizePreset;
use crate::ui::prompt_input::{
    slash_command_highlight_ranges_for_metadata, PromptInputRenderContext,
};
use crate::ui::theme::identity::{agent_identity_style, AgentIdentity};
use crate::ui::theme::{Theme, ThemeColors};
use crate::ui::transcript::{self, TranscriptInputMode, ViewMode};
use crate::ui::welcome;

/// Upper cap on the number of stdout lines the status-line runner is
/// allowed to take up. Arbitrary but small so a runaway script can't
/// eat the messages pane.
const STATUS_LINE_MAX_LINES: usize = 3;
const MESSAGE_BOTTOM_GAP_HEIGHT: u16 = 1;
const COMMAND_HIGHLIGHT_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(1);

impl App {
    pub fn set_kairos_status(&mut self, status: Option<super::domain::KairosUiStatus>) {
        if self.session_ui.kairos_status != status {
            self.session_ui.kairos_status = status;
            self.mark_dirty();
        }
    }

    pub fn set_proactive_status(&mut self, status: Option<ProactiveUiStatus>) {
        if self.session_ui.proactive_status != status {
            self.session_ui.proactive_status = status;
            self.mark_dirty();
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();
        self.welcome_logo_visible = false;
        if size.width < 10 || size.height < 4 {
            return;
        }
        self.render_layout.session_scrollbar = None;
        self.render_layout.message_area = None;
        self.render_layout.prompt_area = None;

        if self.workspace_trust_pending {
            render_workspace_trust_prompt(
                size,
                frame.buffer_mut(),
                &self.session_ui.cwd,
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

        self.refresh_context_layer();
        let expanded_view_lines = self.expanded_view_lines();

        let command_highlights = self.command_highlight_ranges();
        let current_notification = self.current_notification();
        let immediate_notification = current_notification
            .is_some_and(|notification| notification.priority == NotificationPriority::Immediate);
        let spinner_height = if self.is_streaming && !immediate_notification {
            1u16
        } else {
            0
        };
        let expanded_view_height = if immediate_notification {
            0
        } else {
            expanded_view_lines
                .len()
                .min(size.height.saturating_sub(5) as usize) as u16
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
        let cwd_path = std::path::Path::new(&self.session_ui.cwd);
        let command_arg_help_height = 0;
        let notification_height = u16::from(current_notification.is_some());
        let context_height = u16::from(!self.runtime_view.context_layer().items().is_empty());
        let agent_footer_height = if self.agent_footer_visible() && !immediate_notification {
            self.agent_footer_height()
        } else {
            0
        };
        let input_height = self
            .prompt
            .preferred_height(size.width, PromptInputRenderContext::default());
        let status_height = if custom_lines.is_empty() {
            1u16
        } else {
            custom_lines.len().min(STATUS_LINE_MAX_LINES) as u16
        };
        let bottom_pane = BottomPaneHeights {
            expanded_view: expanded_view_height,
            spinner: spinner_height,
            suggestions: suggestion_height,
            input: input_height,
            completion_popup: completion_popup_height,
            command_palette: command_palette_height,
            command_arg_help: command_arg_help_height,
            notification: notification_height,
            context: context_height,
            agent_footer: agent_footer_height,
            status: status_height,
        };
        let bottom_height = bottom_pane.total();
        let message_bottom_gap_height = u16::from(bottom_height > 0) * MESSAGE_BOTTOM_GAP_HEIGHT;
        let max_content_height = size
            .height
            .saturating_sub(bottom_height.saturating_add(message_bottom_gap_height));
        let content_height = if self.overlays.command_surface.is_some() {
            // A command surface is a modal panel above the prompt. Give it
            // the full message area so the normal welcome height does not
            // leave the prompt covering the panel's lower rows.
            max_content_height
        } else if self.show_welcome {
            welcome::welcome_height_for(size.width).min(max_content_height)
        } else {
            let (selected, selected_expanded) = self.conversation.render_context_inputs();
            let message_vm = MessageListViewModel::build(
                self.conversation.messages(),
                selected,
                selected_expanded,
                MessageRenderOptions {
                    verbose: self.verbose,
                    brief_only: self.brief_only,
                    is_transcript_mode: false,
                    thinking_animation_frame: self.thinking_animation_frame(),
                    ..MessageRenderOptions::default()
                },
            );
            self.conversation.ensure_vscroll_up_to_date(
                size.width,
                &self.theme,
                message_vm.render_context(),
            );
            self.conversation
                .vscroll()
                .total_visual_lines()
                .min(max_content_height as usize) as u16
        };

        let chunks = Layout::vertical([
            Constraint::Length(content_height),
            Constraint::Min(0),
            Constraint::Length(message_bottom_gap_height),
            Constraint::Length(bottom_height),
        ])
        .split(size);

        let message_area = chunks[0];
        let bottom_area = chunks[3];
        self.render_layout.message_area = Some(message_area);

        if self.show_welcome {
            // Welcome screen
            let logo_visible = welcome::render_welcome(
                message_area,
                frame.buffer_mut(),
                welcome::WelcomeInfo {
                    version: env!("CARGO_PKG_VERSION"),
                    model_name: &self.session_ui.model_name,
                    session_id: &self.session_ui.session_id,
                    cwd: &self.session_ui.cwd,
                },
                self.welcome_logo.frame(),
                self.design_theme_provider.colors(),
            );
            self.welcome_logo_visible = logo_visible;
        } else {
            // Messages (virtual scroll)
            let (selected, selected_expanded) = self.conversation.render_context_inputs();
            let message_vm = MessageListViewModel::build(
                self.conversation.messages(),
                selected,
                selected_expanded,
                MessageRenderOptions {
                    verbose: self.verbose,
                    brief_only: self.brief_only,
                    is_transcript_mode: false,
                    thinking_animation_frame: self.thinking_animation_frame(),
                    ..MessageRenderOptions::default()
                },
            );
            self.conversation.ensure_vscroll_up_to_date(
                message_area.width,
                &self.theme,
                message_vm.render_context(),
            );
            let mut total = self.conversation.vscroll().total_visual_lines();
            let (message_body_area, scrollbar_area) =
                split_session_scrollbar_area(message_area, total);
            if message_body_area.width != message_area.width {
                self.conversation.ensure_vscroll_up_to_date(
                    message_body_area.width,
                    &self.theme,
                    message_vm.render_context(),
                );
                total = self.conversation.vscroll().total_visual_lines();
            }
            let max_scroll = total.saturating_sub(message_area.height as usize);
            if self.conversation.scroll_offset() > max_scroll {
                self.conversation.set_scroll_offset(max_scroll);
            }

            render_messages(
                self.conversation.messages(),
                message_body_area,
                frame.buffer_mut(),
                &self.theme,
                self.is_streaming,
                self.conversation.scroll_offset(),
                self.conversation.vscroll(),
                message_vm.render_context(),
            );
            if let Some(scrollbar_area) = scrollbar_area {
                self.render_layout.session_scrollbar = Some(super::SessionScrollbarState {
                    area: scrollbar_area,
                    total_lines: total,
                });
                render_session_scrollbar(
                    scrollbar_area,
                    frame.buffer_mut(),
                    total,
                    self.conversation.scroll_offset(),
                    &self.theme,
                );
            }
        }

        // Bottom area: expanded task/teammate view + spinner + suggestions + completion popup + palette + arg help + input + notification + context + agent footer + status.
        let has_suggestions = suggestion_height > 0;
        let bottom_chunks = bottom_pane.split(bottom_area);
        self.render_layout.prompt_area = Some(bottom_chunks.input);

        for (index, line) in expanded_view_lines
            .iter()
            .take(bottom_chunks.expanded_view.height as usize)
            .enumerate()
        {
            frame.buffer_mut().set_line(
                bottom_chunks.expanded_view.x,
                bottom_chunks.expanded_view.y + index as u16,
                line,
                bottom_chunks.expanded_view.width,
            );
        }

        if self.is_streaming && bottom_chunks.spinner.height > 0 {
            self.spinner_state
                .render(bottom_chunks.spinner, frame.buffer_mut(), &self.theme);
        }

        if has_suggestions {
            self.render_suggestions(bottom_chunks.suggestions, frame.buffer_mut());
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
        let prompt_hint = if self.command_palette.active() {
            Some(CommandPalette::input_hint())
        } else {
            argument_hint.as_deref()
        };
        let placeholder = self.prompt_placeholder();
        let mode_indicator = self.prompt_mode_indicator();
        let prompt_layout = self.prompt.render_with_context(
            bottom_chunks.input,
            frame.buffer_mut(),
            &self.theme,
            PromptInputRenderContext {
                hint: prompt_hint,
                placeholder: Some(placeholder),
                mode_indicator: Some(mode_indicator),
                command_highlights: &command_highlights,
            },
        );

        if notification_height > 0 {
            self.render_notification(bottom_chunks.notification, frame.buffer_mut());
        }

        if context_height > 0 {
            self.render_context_layer(bottom_chunks.context, frame.buffer_mut());
        }

        if agent_footer_height > 0 {
            self.render_agent_footer(bottom_chunks.agent_footer, frame.buffer_mut());
        }

        self.render_status_bar(bottom_chunks.status, frame.buffer_mut(), &custom_lines);

        if let Some(ref surface) = self.overlays.command_surface {
            render_command_surface_overlay(surface, size, bottom_chunks.input, frame.buffer_mut());
        }

        if let Some(ref dialog) = self.overlays.history_search_dialog {
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
        let agent_nav = self.runtime_state().agent_nav().clone();
        if let Some(ref mut dialog) = self.overlays.agent_tree_dialog {
            render_agent_tree_overlay(
                dialog,
                &agent_nav,
                &current_thread_id,
                size,
                bottom_chunks.input,
                frame.buffer_mut(),
                &self.theme,
                self.design_theme_provider.colors(),
            );
        }

        if let Some(ref dialog) = self.overlays.permission_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        if let Some(ref dialog) = self.overlays.question_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        if let Some(ref dialog) = self.overlays.bypass_permissions_mode_dialog {
            dialog.render(
                size,
                Some(bottom_chunks.input),
                frame.buffer_mut(),
                &self.theme,
            );
        }

        // The prompt and every editable overlay derive their physical cursor
        // from the same rendered frame. Read-only surfaces intentionally
        // return no position, so Ratatui hides the cursor for that frame.
        let history_cursor = self
            .overlays
            .history_search_dialog
            .as_ref()
            .and_then(|dialog| {
                find_history_search_cursor(frame.buffer_mut(), size, dialog.query())
            });
        let picker_cursor = self
            .overlays
            .command_surface
            .as_ref()
            .and_then(|surface| surface.cursor_anchor())
            .and_then(|anchor| find_command_surface_cursor(frame.buffer_mut(), size, &anchor));
        let permission_cursor = self.overlays.permission_dialog.as_ref().and_then(|dialog| {
            dialog.cursor_position(size, Some(bottom_chunks.input), &self.theme)
        });
        let question_cursor = self
            .overlays
            .question_dialog
            .as_ref()
            .and_then(|dialog| dialog.cursor_position(size, Some(bottom_chunks.input)));
        let placement = self.terminal_cursor_placement(
            prompt_layout.cursor_position(),
            history_cursor,
            picker_cursor,
            permission_cursor,
            question_cursor,
        );
        if let Some(position) = placement.position {
            frame.set_cursor_position(position);
        }

        self.capture_render_snapshot(frame);
    }

    fn expanded_view_lines(&self) -> Vec<Line<'static>> {
        match self.expanded_view {
            super::ExpandedView::None => Vec::new(),
            super::ExpandedView::Tasks => render_task_list_lines(
                self.runtime_view.task_list_items(),
                &self.theme,
                chrono::Utc::now().timestamp(),
            ),
            super::ExpandedView::Teammates => {
                let count = self.runtime_view.agent_nav().active_non_primary_count();
                (count > 0)
                    .then(|| {
                        Line::from(Span::styled(
                            format!(
                                "  {count} teammate{} active",
                                if count == 1 { "" } else { "s" }
                            ),
                            self.theme.dim,
                        ))
                    })
                    .into_iter()
                    .collect()
            }
        }
    }

    pub(super) fn command_highlight_ranges(&mut self) -> Vec<std::ops::Range<usize>> {
        let now = std::time::Instant::now();
        let cwd = std::path::PathBuf::from(&self.session_ui.cwd);
        let registry_revision = allthecodes_commands::dynamic_registry_revision();
        let stale = self
            .command_highlight_cache
            .refreshed_at
            .is_none_or(|refreshed_at| {
                now.saturating_duration_since(refreshed_at) >= COMMAND_HIGHLIGHT_CACHE_TTL
            });
        if self.command_highlight_cache.cwd != cwd
            || self.command_highlight_cache.registry_revision != registry_revision
            || stale
        {
            self.command_highlight_cache.metadata =
                allthecodes_commands::get_dynamic_metadata_for_cwd(&cwd);
            self.command_highlight_cache.cwd = cwd;
            self.command_highlight_cache.registry_revision = registry_revision;
            self.command_highlight_cache.refreshed_at = Some(now);
        }

        slash_command_highlight_ranges_for_metadata(
            &self.prompt.input,
            &self.command_highlight_cache.metadata,
        )
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

    fn render_context_layer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        let items = self.runtime_view.context_layer().items();
        if items.is_empty() {
            return;
        }
        let lines = crate::ui::context_layer::render_context_layer(
            &items,
            area.width as usize,
            &self.theme,
        );
        if let Some(line) = lines.first() {
            buf.set_line(area.x, area.y, line, area.width);
        }
    }

    fn render_agent_footer(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 || !self.agent_footer_visible() {
            return;
        }

        let entries = self.agent_footer_entries();
        if entries.is_empty() {
            return;
        }

        let agent_label = if entries.len() == 1 {
            "agent"
        } else {
            "agents"
        };
        let summary = format!(
            "Running {} {agent_label}... Ctrl+X Ctrl+A open tree",
            entries.len()
        );
        let summary_line = Line::from(vec![
            Span::styled(" agents ", self.theme.info),
            Span::styled(summary, self.theme.dim),
        ]);
        buf.set_line(area.x, area.y, &summary_line, area.width);

        let visible_count = entries.len().min(3);
        let colors = self.design_theme_provider.colors();
        for (index, entry) in entries.iter().take(visible_count).enumerate() {
            let y_offset = 1 + index as u16;
            if y_offset >= area.height {
                break;
            }
            let branch = if index + 1 == visible_count {
                "`-"
            } else {
                "|-"
            };
            let line = Line::from(vec![
                Span::styled(format!("   {branch} "), self.theme.dim),
                Span::styled(
                    truncate_display_width(&entry.label, 32),
                    agent_identity_style(
                        colors,
                        AgentIdentity {
                            agent_id: &entry.thread_id,
                            role: entry.role.as_deref(),
                            is_primary: entry.is_primary,
                        },
                    ),
                ),
                Span::styled(format!(" | {}", entry.status), self.theme.dim),
            ]);
            buf.set_line(area.x, area.y + y_offset, &line, area.width);
        }

        if entries.len() > visible_count {
            let y_offset = 1 + visible_count as u16;
            if y_offset < area.height {
                let line = Line::from(Span::styled(
                    format!("   ... {} more", entries.len() - visible_count),
                    self.theme.dim,
                ));
                buf.set_line(area.x, area.y + y_offset, &line, area.width);
            }
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
        let kairos_includes_proactive = self
            .session_ui
            .kairos_status
            .as_ref()
            .is_some_and(|status| status.includes_proactive);
        if let Some(status) = &self.session_ui.kairos_status {
            parts.push(status.render_inline().to_string());
        }
        if !kairos_includes_proactive {
            if let Some(status) = &self.session_ui.proactive_status {
                parts.push(status.render_inline());
            }
        }
        if let Some(verification) = &self.session_ui.verification {
            parts.push(verification.render_inline());
        }
        if !self.session_ui.cwd.is_empty() {
            parts.push(self.session_ui.cwd.clone());
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
        self.render_layout.message_area = Some(body_area);
        self.render_layout.prompt_area = None;

        // Ensure the virtual-scroll cache matches the body width. Sharing
        // `vscroll` with prompt mode is fine because both invalidate on
        // width change.
        let (selected, selected_expanded) = self.conversation.render_context_inputs();
        let message_vm = MessageListViewModel::build(
            self.conversation.messages(),
            selected,
            selected_expanded,
            MessageRenderOptions {
                verbose: self.verbose,
                brief_only: false,
                is_transcript_mode: true,
                show_all_in_transcript: true,
                thinking_animation_frame: None,
            },
        );
        self.conversation.ensure_vscroll_up_to_date(
            body_area.width,
            &self.theme,
            message_vm.render_context(),
        );
        let mut total = self.conversation.vscroll().total_visual_lines();
        let (message_body_area, scrollbar_area) = split_session_scrollbar_area(body_area, total);
        if message_body_area.width != body_area.width {
            self.conversation.ensure_vscroll_up_to_date(
                message_body_area.width,
                &self.theme,
                message_vm.render_context(),
            );
            total = self.conversation.vscroll().total_visual_lines();
        }
        let max_scroll = total.saturating_sub(body_area.height as usize);
        if self.transcript_state.scroll_offset > max_scroll {
            self.transcript_state.scroll_offset = max_scroll;
        }

        render_messages(
            self.conversation.messages(),
            message_body_area,
            frame.buffer_mut(),
            &self.theme,
            self.is_streaming,
            self.transcript_state.scroll_offset,
            self.conversation.vscroll(),
            message_vm.render_context(),
        );
        if let Some(scrollbar_area) = scrollbar_area {
            self.render_layout.session_scrollbar = Some(super::SessionScrollbarState {
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
        let focus = transcript::build_focus_view(self.conversation.messages());
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
        let total = self.conversation.messages().len();
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
        NotificationTone::Info => theme.context_info_text,
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

fn find_history_search_cursor(buf: &Buffer, area: Rect, query: &str) -> Option<Position> {
    find_cursor_after_marker(buf, area, "filter=", query, &[" matches="])
}

fn find_command_surface_cursor(
    buf: &Buffer,
    area: Rect,
    anchor: &CommandSurfaceCursorAnchor,
) -> Option<Position> {
    match anchor {
        CommandSurfaceCursorAnchor::Search { marker, value } => find_cursor_after_marker(
            buf,
            area,
            marker,
            value,
            &[" matches=", " visible=", " total="],
        ),
        CommandSurfaceCursorAnchor::Field { marker, value } => {
            find_cursor_after_marker(buf, area, marker, value, &[])
        }
    }
}

fn find_cursor_after_marker(
    buf: &Buffer,
    area: Rect,
    marker: &str,
    value: &str,
    stops: &[&str],
) -> Option<Position> {
    // Overlays are painted on top of the conversation. Search from the
    // prompt upward so an identical marker in an older message cannot steal
    // the physical cursor from the active surface.
    for y in (area.y..area.y.saturating_add(area.height)).rev() {
        let line = buffer_row_text(buf, area, y);
        let Some(marker_start) = line.find(marker) else {
            continue;
        };
        let value_start = marker_start + marker.len();
        let visible_value = visible_value_after_marker(&line, value_start, value, stops);
        return position_after_text(area, y, &line[..value_start], &visible_value);
    }
    None
}

fn visible_value_after_marker(
    line: &str,
    value_start: usize,
    value: &str,
    stops: &[&str],
) -> String {
    // An empty value means the caret is immediately after the marker; do not
    // mistake a placeholder or panel padding for user input.
    if value.is_empty() {
        return String::new();
    }

    let remainder = &line[value_start..];
    let stop_end = stops
        .iter()
        .filter_map(|stop| remainder.find(stop))
        .min()
        .unwrap_or(remainder.len());
    let mut visible = remainder[..stop_end].trim_end_matches(' ');
    // BetterViewPanel rows usually end with a right border after the value.
    // Strip only that terminal border and its padding; preserve spaces that
    // are part of a value in the middle of the row.
    if let Some(without_border) = visible.strip_suffix('│') {
        visible = without_border.trim_end_matches(' ');
    }

    if visible.starts_with(value) {
        value.to_string()
    } else {
        visible.to_string()
    }
}

fn position_after_text(area: Rect, y: u16, prefix: &str, value: &str) -> Option<Position> {
    let right = area.x.saturating_add(area.width);
    let x = area
        .x
        .saturating_add(UnicodeWidthStr::width(prefix) as u16)
        .saturating_add(UnicodeWidthStr::width(value) as u16);
    if y >= area.y.saturating_add(area.height) || right <= area.x || x >= right {
        return None;
    }
    Some(Position { x, y })
}

fn buffer_row_text(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x.saturating_add(area.width))
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    #[test]
    fn cursor_scan_prefers_the_overlay_row() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        buf.set_string(1, 0, "old message filter=old", Style::default());
        buf.set_string(1, 4, "overlay filter=new matches=1", Style::default());

        let position = find_cursor_after_marker(&buf, area, "filter=", "new", &[" matches="])
            .expect("overlay cursor");
        assert_eq!(position.y, 4);
        assert_eq!(
            position.x,
            1 + UnicodeWidthStr::width("overlay filter=new") as u16
        );
    }

    #[test]
    fn field_cursor_uses_visible_value_without_panel_padding() {
        let area = Rect::new(0, 0, 32, 2);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "│ input: abc              │", Style::default());

        let position =
            find_cursor_after_marker(&buf, area, "input: ", "abc", &[]).expect("field cursor");
        assert_eq!(position, Position { x: 12, y: 0 });
    }
}

fn render_command_surface_overlay(
    surface: &CommandSurface,
    area: Rect,
    prompt_area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let text = surface.render();
    let body = text
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    render_prompt_adjacent_lines(
        body,
        area,
        prompt_area,
        PanelSizePreset::BetterViewPanel.spec(),
        buf,
        Style::default(),
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

#[allow(clippy::too_many_arguments)]
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

    let mut lines = dialog.render_lines(state, current_thread_id, theme, colors);
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

fn truncate_display_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut width = 0usize;
    let mut out = String::new();
    let mut truncated = false;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            truncated = true;
            break;
        }
        width += ch_width;
        out.push(ch);
    }

    if truncated && max_width > 3 {
        while display_width(&out) + 3 > max_width {
            out.pop();
        }
        out.push_str("...");
    }

    out
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
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
                self.theme.selected
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_info_label_is_blue_but_body_is_not() {
        let theme = Theme::default();

        assert_eq!(theme.info.fg, Some(Color::Rgb(130, 200, 255)));
        assert_eq!(
            notification_style(NotificationTone::Info, &theme).fg,
            theme.context_info_text.fg
        );
        assert_ne!(
            notification_style(NotificationTone::Info, &theme).fg,
            theme.info.fg
        );
        assert_eq!(
            notification_style(NotificationTone::Dim, &theme).fg,
            theme.dim.fg
        );
    }
}
