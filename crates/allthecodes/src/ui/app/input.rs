use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::ui::clipboard_paste::{
    normalize_pasted_path, paste_image_to_temp_png, pasted_image_format, EncodedImageFormat,
};
use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::completions::{
    CombinedCompleter, CommandCompletionProvider, CompletionContext, CompletionItem,
};
use crate::ui::history_search_dialog::{HistorySearchDialog, HistorySearchDialogEvent};
use crate::ui::messages::{
    message_copy_text, message_copy_text_with_mode, message_primary_reference, CopyTextMode,
};
use crate::ui::path_completion::PathCompletionProvider;
use crate::ui::shell_history_completion::ShellHistoryCompletionProvider;
use crate::ui::slack_channel_completion::SlackChannelCompletionProvider;
use crate::ui::transcript::ViewMode;
use crate::ui::vim::VimAction;

use super::overlays::{ActiveOverlay, OverlayOutcome};
use super::{current_unix_secs, App, AppAction, MouseFocus};

/// Tracks the state of an active completion session.
#[derive(Debug)]
pub struct CompletionState {
    /// All computed completion items.
    pub items: Vec<CompletionItem>,
    /// Currently selected index.
    pub selected: usize,
    /// Whether the completion popup is visible.
    pub active: bool,
    /// The combined completer used to compute items.
    pub completer: CombinedCompleter,
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

impl CompletionState {
    pub fn new() -> Self {
        let mut completer = CombinedCompleter::new();
        completer.add_provider(Box::new(CommandCompletionProvider::new()));
        completer.add_provider(Box::new(PathCompletionProvider::new()));
        completer.add_provider(Box::new(ShellHistoryCompletionProvider::new()));
        completer.add_provider(Box::new(SlackChannelCompletionProvider::new()));

        Self {
            items: Vec::new(),
            selected: 0,
            active: false,
            completer,
        }
    }

    /// Compute completions for the given input and cursor position.
    pub fn compute(&mut self, input: &str, cursor_pos: usize) {
        let ctx = CompletionContext::new(input, cursor_pos, &[]);
        self.items = self.completer.compute(&ctx);
        self.selected = 0;
        self.active = !self.items.is_empty();
    }

    /// Get the selected completion item, if any.
    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.items.get(self.selected)
    }

    /// Move selection up (toward earlier items).
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down (toward later items).
    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    /// Close the completion popup.
    pub fn close(&mut self) {
        self.active = false;
        self.items.clear();
        self.selected = 0;
    }
}

impl Default for CompletionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod completion_state_tests {
    use super::*;
    use crate::ui::completions::CompletionKind;

    #[test]
    fn selection_moves_within_bounds() {
        let mut state = CompletionState::new();
        state.items = vec![
            CompletionItem::new(CompletionKind::Command, "one", "one", 0..0),
            CompletionItem::new(CompletionKind::Command, "two", "two", 0..0),
        ];
        state.active = true;

        state.select_next();
        assert_eq!(state.selected, 1);
        state.select_next();
        assert_eq!(state.selected, 0);
        state.select_prev();
        assert_eq!(state.selected, 0);
    }
}

impl App {
    pub fn handle_key_event(&mut self, key: KeyEvent) -> AppAction {
        if key.kind != KeyEventKind::Press {
            return AppAction::None;
        }

        // Any key press is likely to cause a visual change.
        self.dirty = true;

        if matches!(key.code, KeyCode::F(12)) {
            return AppAction::DebugSnapshot;
        }

        if self.workspace_trust_pending {
            return self.handle_workspace_trust_key(key);
        }

        if let Some(action) = self.handle_active_overlay_key(key).into_app_action() {
            return action;
        }

        if self.conversation.selection().is_some() {
            if let Some(action) = self.resolve_bound_action(&key) {
                if let Some(result) = self.dispatch_bound_action(&action) {
                    return result;
                }
            }
            return AppAction::None;
        }

        if self.command_palette.active() {
            if self.command_palette.edit_target_picker_active() {
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                        self.command_palette.close_edit_target_picker();
                        return AppAction::None;
                    }
                    (_, KeyCode::Enter) => {
                        if let Some(insert_text) = self.command_palette.selected_edit_target_input()
                        {
                            let needs_separator = self
                                .prompt
                                .input
                                .get(..self.prompt.cursor_position)
                                .and_then(|prefix| prefix.chars().last())
                                .is_some_and(|ch| !ch.is_whitespace());
                            if needs_separator {
                                self.prompt.insert_str(" ");
                            }
                            self.prompt.insert_str(&insert_text);
                            self.sync_command_palette();
                        }
                        self.command_palette.close_edit_target_picker();
                        return AppAction::None;
                    }
                    (_, KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right)
                    | (_, KeyCode::PageUp)
                    | (_, KeyCode::PageDown)
                    | (_, KeyCode::Tab)
                    | (_, KeyCode::BackTab) => {
                        self.command_palette.handle_edit_target_key(key.code);
                        return AppAction::None;
                    }
                    _ => {
                        self.command_palette.handle_edit_target_key(key.code);
                        return AppAction::None;
                    }
                }
            }

            match (key.modifiers, key.code) {
                (_, KeyCode::Esc) => {
                    self.command_palette.close();
                    return AppAction::None;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('e'))
                    if self.command_palette.selected_command_has_edit_targets() =>
                {
                    if self.command_palette.open_edit_target_picker() {
                        return AppAction::None;
                    }
                }
                (_, KeyCode::Enter) => {
                    if let Some(command) = self.command_palette.selected_command_for_execution() {
                        self.command_palette.close();
                        self.prompt.clear();
                        self.prompt.reset_vertical_navigation();
                        return AppAction::Submit(command);
                    }

                    // No suggestion matched. Submit the user's slash command
                    // unchanged instead of swallowing the first Enter.
                    self.command_palette.close();
                    return self
                        .take_prompt_submission()
                        .map_or(AppAction::None, AppAction::Submit);
                }
                (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                    if let Some(command_input) = self.command_palette.selected_command_input() {
                        self.prompt.set_input(command_input);
                        self.prompt.reset_vertical_navigation();
                        self.command_palette.close();
                        return AppAction::None;
                    }
                }
                (_, KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown) => {
                    self.command_palette.handle_key(key.code);
                    return AppAction::None;
                }
                _ => {}
            }
        }

        if let Some(action) = self.resolve_bound_action(&key) {
            if let Some(app_action) = self.dispatch_bound_action(&action) {
                return app_action;
            }
        } else if !self.pending_chord.is_empty() {
            return AppAction::None;
        }

        // Ctrl+C / Ctrl+D retain their "abort or quit" semantics even in
        // transcript / focus modes; the user always needs a way out.
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.is_streaming {
                    self.exit_guard.disarm();
                    return AppAction::Abort;
                } else if self.exit_guard.confirm() {
                    self.should_quit = true;
                    return AppAction::Quit;
                } else {
                    self.add_notification(super::notification_from_app_event(
                        "exit-guard".to_string(),
                        "Press Ctrl+C again to exit".to_string(),
                        "low".to_string(),
                        Some(2500),
                    ));
                    return AppAction::None;
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d'))
                if self.view_mode == ViewMode::Prompt
                    && (self.prompt.is_active || !self.is_streaming) =>
            {
                if self.exit_guard.confirm() {
                    self.should_quit = true;
                    return AppAction::Quit;
                } else {
                    self.add_notification(super::notification_from_app_event(
                        "exit-guard".to_string(),
                        "Press Ctrl+D again to exit".to_string(),
                        "low".to_string(),
                        Some(2500),
                    ));
                    return AppAction::None;
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('o')) => {
                self.exit_guard.disarm();
                // Cycle `Prompt -> Transcript -> Focus -> Prompt`.
                self.cycle_view_mode();
                return AppAction::None;
            }
            _ => self.exit_guard.disarm(),
        }

        // Transcript / focus modes take over all remaining keystrokes.
        if self.view_mode.is_transcript_like() {
            return self.handle_transcript_key(key);
        }

        // Prompt Up/Down is first a visual-line movement. Only when the
        // caret is already at the corresponding multiline edge should the
        // key fall through to prompt history below.
        if self.prompt.is_active && !self.is_streaming && key.modifiers.is_empty() {
            let prompt_width = self
                .render_layout
                .prompt_area
                .map(|area| area.width)
                .unwrap_or(80);
            let direction = match key.code {
                KeyCode::Up => Some(-1),
                KeyCode::Down => Some(1),
                _ => None,
            };
            if let Some(direction) = direction {
                if self.prompt.move_cursor_vertical(prompt_width, direction) {
                    self.history_index = None;
                    self.saved_input.clear();
                    self.sync_command_palette();
                    return AppAction::None;
                }
            }
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::PageUp) | (KeyModifiers::SHIFT, KeyCode::Up) => {
                let step = self.terminal_env.scroll_speed as usize;
                self.scroll_up(step);
                return AppAction::ScrollUp;
            }
            (_, KeyCode::PageDown) | (KeyModifiers::SHIFT, KeyCode::Down) => {
                let step = self.terminal_env.scroll_speed as usize;
                self.scroll_down(step);
                return AppAction::ScrollDown;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) if !self.prompt.is_active => {
                let step = (self.terminal_env.scroll_speed as usize).saturating_mul(2);
                self.scroll_up(step);
                return AppAction::ScrollUp;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d'))
                if !self.prompt.is_active && self.is_streaming =>
            {
                let step = (self.terminal_env.scroll_speed as usize).saturating_mul(2);
                self.scroll_down(step);
                return AppAction::ScrollDown;
            }

            (_, KeyCode::Up) if self.prompt.is_active && !self.is_streaming => {
                self.history_up();
                self.sync_command_palette();
                return AppAction::None;
            }
            (_, KeyCode::Down) if self.prompt.is_active && !self.is_streaming => {
                self.history_down();
                self.sync_command_palette();
                return AppAction::None;
            }

            _ => {}
        }

        if self.prompt.is_active
            && !self.is_streaming
            && matches!(
                (key.modifiers, key.code),
                (KeyModifiers::CONTROL, KeyCode::Char('v'))
            )
            && self.try_paste_clipboard_image()
        {
            return AppAction::None;
        }

        if self.vim.enabled && self.prompt.is_active {
            let vim_action =
                self.vim
                    .handle_key(key, &self.prompt.input, self.prompt.cursor_position);
            if let Some(app_action) = self.apply_vim_action(vim_action) {
                self.sync_command_palette();
                return app_action;
            }
        }

        // ── Tab completion ─────────────────────────────────────────────
        if self.prompt.is_active && !self.is_streaming {
            match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    // If completion is already active, cycle through candidates.
                    if self.completion_state.active {
                        if let Some(item) = self.completion_state.selected_item() {
                            self.prompt.set_ghost_suffix(item.ghost_suffix.clone());
                        }
                        self.completion_state.select_next();
                        if let Some(item) = self.completion_state.selected_item() {
                            self.prompt.set_ghost_suffix(item.ghost_suffix.clone());
                            self.prompt.set_show_ghost(true);
                        }
                        self.sync_command_palette();
                        self.dirty = true;
                        return AppAction::None;
                    }

                    // Check if cursor follows a word boundary (potential completion trigger)
                    let input = &self.prompt.input;
                    let cursor = self.prompt.cursor_position;
                    let should_complete = input.is_empty()
                        || cursor == 0
                        || input[..cursor].ends_with(' ')
                        || input[..cursor].ends_with('/');
                    // Also trigger when typing a /command
                    let has_slash_prefix = input.starts_with('/') || input[..cursor].contains('/');

                    if should_complete || has_slash_prefix {
                        self.completion_state.compute(input, cursor);
                        if self.completion_state.active {
                            // Show ghost suffix for first item
                            if let Some(item) = self.completion_state.selected_item() {
                                self.prompt.set_ghost_suffix(item.ghost_suffix.clone());
                                self.prompt.set_show_ghost(true);
                            }
                            self.dirty = true;
                            return AppAction::None;
                        }
                    }
                }
                (KeyModifiers::SHIFT, KeyCode::Tab) | (KeyModifiers::NONE, KeyCode::BackTab) => {
                    if self.completion_state.active {
                        self.completion_state.select_prev();
                        if let Some(item) = self.completion_state.selected_item() {
                            self.prompt.set_ghost_suffix(item.ghost_suffix.clone());
                        }
                        self.dirty = true;
                        return AppAction::None;
                    }
                }
                _ => {}
            }
        }

        if self.completion_state.active && matches!(key.code, KeyCode::Enter) {
            if let Some(item) = self.completion_state.selected_item() {
                let range = &item.range;
                let insert = &item.insert_text;
                if range.end <= self.prompt.input.len() {
                    self.prompt.replace_range(range.clone(), insert);
                    self.prompt.set_cursor_position(range.start + insert.len());
                    self.prompt.reset_vertical_navigation();
                }
            }
            self.completion_state.close();
            self.prompt.set_ghost_suffix(None);
            self.prompt.set_show_ghost(false);
            self.sync_command_palette();
            self.dirty = true;
            return AppAction::None;
        }

        if let Some(submitted) = self.prompt.handle_key(key) {
            self.command_palette.close();
            self.completion_state.close();
            self.prompt.set_ghost_suffix(None);
            self.prompt.set_show_ghost(false);
            return AppAction::Submit(submitted);
        }

        self.sync_command_palette();
        AppAction::None
    }

    fn try_paste_clipboard_image(&mut self) -> bool {
        let Ok((path, info)) = paste_image_to_temp_png() else {
            return false;
        };

        let path_text = path.to_string_lossy().into_owned();
        self.prompt.paste_text(&path_text);
        self.history_index = None;
        self.saved_input.clear();
        self.sync_command_palette();
        self.add_notification(super::notification_from_app_event(
            "pasted-clipboard-image".to_string(),
            format!(
                "Pasted {} image from clipboard: {}x{} {path_text}",
                info.encoded_format.label(),
                info.width,
                info.height
            ),
            "low".to_string(),
            Some(3500),
        ));
        self.dirty = true;
        true
    }

    fn handle_active_overlay_key(&mut self, key: KeyEvent) -> OverlayOutcome {
        let action = match self.overlays.active_overlay() {
            Some(ActiveOverlay::BypassPermissions) => self.handle_bypass_permissions_key(key),
            Some(ActiveOverlay::Question) => self.handle_question_key(key),
            Some(ActiveOverlay::Permission) => self.handle_permission_key(key),
            Some(ActiveOverlay::AgentTree) => self.handle_agent_tree_key(key),
            Some(ActiveOverlay::HistorySearch) => self.handle_history_search_key(key),
            Some(ActiveOverlay::CommandSurface) => self.handle_command_surface_key(key),
            None => return OverlayOutcome::Inactive,
        };
        OverlayOutcome::Handled(action)
    }

    fn handle_bypass_permissions_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(dialog) = self.overlays.bypass_permissions_mode_dialog.as_mut() else {
            return AppAction::None;
        };
        if let Some(choice) = dialog.handle_key(key) {
            self.overlays.bypass_permissions_mode_dialog = None;
            return AppAction::BypassPermissionsModeResponse(choice);
        }
        AppAction::None
    }

    fn handle_question_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(dialog) = self.overlays.question_dialog.as_mut() else {
            return AppAction::None;
        };
        if let Some(answer) = dialog.handle_key(key) {
            self.overlays.clear_question();
            return AppAction::QuestionResponse(answer);
        }
        AppAction::None
    }

    fn handle_permission_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(dialog) = self.overlays.permission_dialog.as_mut() else {
            return AppAction::None;
        };
        if let Some(choice) = dialog.handle_key(key) {
            self.overlays.clear_permission();
            return AppAction::PermissionResponse(choice);
        }
        AppAction::None
    }

    fn handle_command_surface_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(surface) = self.overlays.command_surface.as_mut() else {
            return AppAction::None;
        };

        match surface.handle_key(key) {
            CommandSurfaceOutcome::None => AppAction::None,
            CommandSurfaceOutcome::Close => {
                self.overlays.command_surface = None;
                AppAction::None
            }
            CommandSurfaceOutcome::FillPrompt(text) => {
                self.overlays.command_surface = None;
                self.prompt.set_input(text);
                self.prompt.is_active = true;
                self.prompt.reset_vertical_navigation();
                self.sync_command_palette();
                AppAction::None
            }
            CommandSurfaceOutcome::Submit(text) => {
                self.overlays.command_surface = None;
                AppAction::Submit(text)
            }
            CommandSurfaceOutcome::SubmitThenOpen {
                command,
                next_surface,
            } => {
                self.overlays.command_surface = None;
                self.set_pending_command_surface_after_submit(next_surface);
                AppAction::Submit(command)
            }
            CommandSurfaceOutcome::LspRecommendationResponse {
                request_id,
                plugin_name,
                decision,
                install_prompt,
            } => {
                self.overlays.command_surface = None;
                if let Some(text) = install_prompt {
                    self.prompt.set_input(text);
                    self.prompt.is_active = true;
                    self.prompt.reset_vertical_navigation();
                    self.sync_command_palette();
                }
                AppAction::LspRecommendationResponse {
                    request_id,
                    plugin_name,
                    decision,
                }
            }
        }
    }

    fn handle_history_search_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(dialog) = self.overlays.history_search_dialog.as_mut() else {
            return AppAction::None;
        };

        match dialog.handle_key(key) {
            HistorySearchDialogEvent::None => AppAction::None,
            HistorySearchDialogEvent::Cancelled => {
                self.overlays.history_search_dialog = None;
                self.sync_command_palette();
                AppAction::None
            }
            HistorySearchDialogEvent::Selected(prompt) => {
                self.overlays.history_search_dialog = None;
                self.prompt.set_input(prompt);
                self.prompt.is_active = true;
                self.prompt.reset_vertical_navigation();
                self.history_index = None;
                self.saved_input.clear();
                self.sync_command_palette();
                AppAction::None
            }
        }
    }

    fn handle_agent_tree_key(&mut self, key: KeyEvent) -> AppAction {
        let Some(mut dialog) = self.overlays.agent_tree_dialog.take() else {
            return AppAction::None;
        };
        let current_thread_id = self.current_agent_thread_id().to_string();
        let agent_nav = self.runtime_state().agent_nav().clone();

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.overlays.agent_tree_dialog = None;
                self.dirty = true;
            }
            (_, KeyCode::Up) => {
                dialog.move_prev(&agent_nav, &current_thread_id);
                self.overlays.agent_tree_dialog = Some(dialog);
                self.dirty = true;
            }
            (_, KeyCode::Down) | (_, KeyCode::Tab) => {
                dialog.move_next(&agent_nav, &current_thread_id);
                self.overlays.agent_tree_dialog = Some(dialog);
                self.dirty = true;
            }
            (_, KeyCode::Enter) => {
                if let Some(selected) = dialog.selected_thread_id(&agent_nav, &current_thread_id) {
                    self.runtime_view.set_current_agent_thread(Some(selected));
                    let selected = self.current_agent_thread_id().to_string();
                    self.overlays.agent_tree_dialog = None;
                    self.dirty = true;
                    return AppAction::AgentThreadSelected(selected);
                }
                self.overlays.agent_tree_dialog = None;
                self.dirty = true;
            }
            _ => {
                self.overlays.agent_tree_dialog = Some(dialog);
            }
        }

        AppAction::None
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) -> AppAction {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && (self.command_palette.active() || self.overlays.command_surface.is_some())
        {
            // Command panels are modal with respect to the session behind
            // them. Do not let wheel input move conversation text underneath
            // an open palette or command surface.
            self.dirty = true;
            return AppAction::None;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.update_mouse_focus(mouse);
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_up(1);
                    AppAction::ScrollUp
                } else {
                    self.scroll_up(1);
                    AppAction::ScrollUp
                }
            }
            MouseEventKind::ScrollDown => {
                self.update_mouse_focus(mouse);
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_down(1);
                    AppAction::ScrollDown
                } else {
                    self.scroll_down(1);
                    AppAction::ScrollDown
                }
            }
            MouseEventKind::Down(_) => {
                if self.mouse_on_session_scrollbar(mouse) {
                    self.render_layout.session_scrollbar_dragging = true;
                    self.seek_session_scrollbar(mouse.row)
                } else {
                    self.update_mouse_focus(mouse);
                    AppAction::None
                }
            }
            MouseEventKind::Drag(_) => {
                if self.render_layout.session_scrollbar_dragging {
                    self.seek_session_scrollbar(mouse.row)
                } else {
                    AppAction::None
                }
            }
            MouseEventKind::Up(_) => {
                self.render_layout.session_scrollbar_dragging = false;
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn update_mouse_focus(&mut self, mouse: MouseEvent) -> MouseFocus {
        if self
            .render_layout
            .prompt_area
            .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
        {
            self.mouse_focus = MouseFocus::Prompt;
            return self.mouse_focus;
        }
        if self
            .render_layout
            .message_area
            .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
        {
            self.mouse_focus = MouseFocus::Messages;
        }
        self.mouse_focus
    }

    fn mouse_on_session_scrollbar(&self, mouse: MouseEvent) -> bool {
        let Some(scrollbar) = self.render_layout.session_scrollbar else {
            return false;
        };
        mouse.column == scrollbar.area.x
            && mouse.row >= scrollbar.area.y
            && mouse.row < scrollbar.area.y.saturating_add(scrollbar.area.height)
    }

    fn seek_session_scrollbar(&mut self, row: u16) -> AppAction {
        let Some(scrollbar) = self.render_layout.session_scrollbar else {
            return AppAction::None;
        };
        let area = scrollbar.area;
        if area.height == 0 {
            return AppAction::None;
        }
        let viewport = area.height as usize;
        let max_scroll = scrollbar.total_lines.saturating_sub(viewport);
        if max_scroll == 0 {
            return AppAction::None;
        }

        if row <= area.y {
            if self.view_mode.is_transcript_like() {
                self.scroll_transcript_up(1);
            } else {
                self.scroll_up(1);
            }
            return AppAction::ScrollUp;
        }
        if row >= area.y.saturating_add(area.height).saturating_sub(1) {
            if self.view_mode.is_transcript_like() {
                self.scroll_transcript_down(1);
            } else {
                self.scroll_down(1);
            }
            return AppAction::ScrollDown;
        }

        let track_height = area.height.saturating_sub(2) as usize;
        if track_height == 0 {
            return AppAction::None;
        }
        let relative = row.saturating_sub(area.y.saturating_add(1)) as usize;
        let scroll = if track_height <= 1 {
            0
        } else {
            relative.min(track_height - 1) * max_scroll / (track_height - 1)
        };
        let previous = if self.view_mode.is_transcript_like() {
            let previous = self.transcript_state.scroll_offset;
            self.transcript_state.scroll_offset = scroll;
            previous
        } else {
            let previous = self.conversation.scroll_offset();
            self.conversation.set_scroll_offset(scroll);
            previous
        };
        self.dirty = true;

        if scroll < previous {
            AppAction::ScrollUp
        } else if scroll > previous {
            AppAction::ScrollDown
        } else {
            AppAction::None
        }
    }

    pub fn handle_paste_event(&mut self, text: String) -> AppAction {
        if self.view_mode != ViewMode::Prompt || !self.prompt.is_active {
            return AppAction::None;
        }
        let paste_text = if let Some(path) = normalize_pasted_path(&text) {
            let format = pasted_image_format(&path);
            let path_text = path.to_string_lossy().into_owned();
            if format != EncodedImageFormat::Other {
                self.add_notification(super::notification_from_app_event(
                    "pasted-image-path".to_string(),
                    format!("Pasted {} image path: {path_text}", format.label()),
                    "low".to_string(),
                    Some(3500),
                ));
            }
            path_text
        } else {
            text
        };
        self.prompt.paste_text(&paste_text);
        self.history_index = None;
        self.saved_input.clear();
        self.sync_command_palette();
        self.dirty = true;
        AppAction::None
    }

    pub(super) fn scroll_up(&mut self, lines: usize) {
        self.conversation
            .set_scroll_offset(self.conversation.scroll_offset().saturating_sub(lines));
        self.dirty = true;
    }

    pub(super) fn scroll_down(&mut self, lines: usize) {
        self.conversation
            .set_scroll_offset(self.conversation.scroll_offset().saturating_add(lines));
        self.dirty = true;
    }

    pub(super) fn scroll_to_bottom_deferred(&mut self) {
        self.conversation.set_scroll_offset(usize::MAX);
    }

    pub(super) fn history_up(&mut self) {
        if self.session_ui.history.is_empty() {
            return;
        }
        if self.history_index.is_none() {
            self.saved_input = self.prompt.expanded_text();
            self.history_index = Some(self.session_ui.history.len() - 1);
        } else if let Some(idx) = self.history_index {
            if idx > 0 {
                self.history_index = Some(idx - 1);
            } else {
                return;
            }
        }
        if let Some(idx) = self.history_index {
            self.prompt
                .set_input(self.session_ui.history[idx].display.clone());
            self.prompt.reset_vertical_navigation();
        }
    }

    pub(super) fn history_down(&mut self) {
        if let Some(idx) = self.history_index {
            if idx < self.session_ui.history.len() - 1 {
                self.history_index = Some(idx + 1);
                self.prompt
                    .set_input(self.session_ui.history[idx + 1].display.clone());
                self.prompt.reset_vertical_navigation();
            } else {
                self.history_index = None;
                self.prompt.set_input(self.saved_input.clone());
                self.prompt.reset_vertical_navigation();
            }
        }
    }

    pub(super) fn take_prompt_submission(&mut self) -> Option<String> {
        if self.prompt.input.trim().is_empty() {
            return None;
        }
        let text = self.prompt.expanded_text();
        self.prompt.clear();
        self.prompt.reset_vertical_navigation();
        Some(text)
    }

    pub fn restore_prompt_text(&mut self, text: String) {
        self.prompt.set_input(text);
        self.prompt.is_active = true;
        self.prompt.reset_vertical_navigation();
        self.sync_command_palette();
        self.dirty = true;
    }

    pub(super) fn sync_command_palette(&mut self) {
        if self.prompt.is_active && !self.is_streaming {
            self.command_palette.sync_from_input(
                &self.prompt.input,
                std::path::Path::new(&self.session_ui.cwd),
            );
        } else {
            self.command_palette.close();
        }
    }

    pub(super) fn active_keybinding_contexts(
        &self,
    ) -> Vec<allthecodes_keybindings::context::Context> {
        if self.conversation.selection().is_some() {
            return vec![
                allthecodes_keybindings::context::Context::MessageActions,
                allthecodes_keybindings::context::Context::Global,
            ];
        }
        if self.view_mode.is_transcript_like() {
            vec![
                allthecodes_keybindings::context::Context::Transcript,
                allthecodes_keybindings::context::Context::Scroll,
            ]
        } else {
            let mut contexts = Vec::with_capacity(3);
            if self.is_streaming {
                contexts.push(allthecodes_keybindings::context::Context::Busy);
            }
            contexts.extend([
                allthecodes_keybindings::context::Context::Chat,
                allthecodes_keybindings::context::Context::Scroll,
            ]);
            contexts
        }
    }

    pub(super) fn resolve_bound_action(
        &mut self,
        key: &KeyEvent,
    ) -> Option<allthecodes_keybindings::action::Action> {
        use allthecodes_keybindings::keystroke::{Chord, Keystroke};
        use allthecodes_keybindings::registry::Resolution;

        let stroke = Keystroke::from_event(key)?;

        if !self.pending_chord.is_empty() {
            self.pending_chord.push(stroke.clone());
            let chord = Chord(self.pending_chord.clone());
            for context in self.active_keybinding_contexts() {
                match self.keybindings.resolve_chord(context, &chord) {
                    Resolution::Action(action) => {
                        self.pending_chord.clear();
                        return Some(action);
                    }
                    Resolution::Pending => return None,
                    Resolution::None => {}
                }
            }
            self.pending_chord.clear();
        }

        for context in self.active_keybinding_contexts() {
            match self.keybindings.resolve_single(context, &stroke) {
                Resolution::Action(action) => return Some(action),
                Resolution::Pending => {
                    self.pending_chord = vec![stroke.clone()];
                    return None;
                }
                Resolution::None => {}
            }
        }

        None
    }

    pub(super) fn dispatch_bound_action(
        &mut self,
        action: &allthecodes_keybindings::action::Action,
    ) -> Option<AppAction> {
        match action.as_str() {
            "app:interrupt" => {
                if self.is_streaming {
                    self.exit_guard.disarm();
                    return Some(AppAction::Abort);
                }
                if self.exit_guard.confirm() {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }
                self.add_notification(super::notification_from_app_event(
                    "exit-guard".to_string(),
                    "Press Ctrl+C again to exit".to_string(),
                    "low".to_string(),
                    Some(2500),
                ));
                return Some(AppAction::None);
            }
            "app:exit" => {
                if self.view_mode == ViewMode::Prompt
                    && (self.prompt.is_active || !self.is_streaming)
                {
                    if self.exit_guard.confirm() {
                        self.should_quit = true;
                        return Some(AppAction::Quit);
                    }
                    self.add_notification(super::notification_from_app_event(
                        "exit-guard".to_string(),
                        "Press Ctrl+D again to exit".to_string(),
                        "low".to_string(),
                        Some(2500),
                    ));
                    return Some(AppAction::None);
                }
                return Some(AppAction::None);
            }
            "app:toggleTranscript" => {
                self.cycle_view_mode();
                return Some(AppAction::None);
            }
            "app:toggleVim" => {
                self.vim.toggle();
                self.dirty = true;
                return Some(AppAction::None);
            }
            "app:redraw" => {
                self.mark_dirty();
                return Some(AppAction::None);
            }
            "history:previous" => {
                if self.view_mode == ViewMode::Prompt && self.prompt.is_active && !self.is_streaming
                {
                    let prompt_width = self
                        .render_layout
                        .prompt_area
                        .map(|area| area.width)
                        .unwrap_or(80);
                    if self.prompt.move_cursor_vertical(prompt_width, -1) {
                        self.history_index = None;
                        self.saved_input.clear();
                        return Some(AppAction::None);
                    }
                    self.history_up();
                    return Some(AppAction::None);
                }
                return None;
            }
            "history:next" => {
                if self.view_mode == ViewMode::Prompt && self.prompt.is_active && !self.is_streaming
                {
                    let prompt_width = self
                        .render_layout
                        .prompt_area
                        .map(|area| area.width)
                        .unwrap_or(80);
                    if self.prompt.move_cursor_vertical(prompt_width, 1) {
                        self.history_index = None;
                        self.saved_input.clear();
                        return Some(AppAction::None);
                    }
                    self.history_down();
                    return Some(AppAction::None);
                }
                return None;
            }
            "history:search" => {
                if self.view_mode == ViewMode::Prompt && !self.is_streaming {
                    self.open_history_search();
                }
                return Some(AppAction::None);
            }
            "chat:clearInput" => {
                self.prompt.clear();
                self.prompt.reset_vertical_navigation();
                self.dirty = true;
                return Some(AppAction::None);
            }
            "chat:submit" => {
                return Some(
                    self.take_prompt_submission()
                        .map_or(AppAction::None, AppAction::Submit),
                );
            }
            "chat:steer" => {
                return Some(
                    self.take_prompt_submission()
                        .map_or(AppAction::None, AppAction::Steer),
                );
            }
            "chat:queue" => {
                let Some(text) = self.take_prompt_submission() else {
                    return Some(AppAction::None);
                };
                if self.is_streaming {
                    return Some(AppAction::Queue(text));
                }
                return Some(AppAction::Submit(text));
            }
            "chat:killAgents" => {
                return Some(AppAction::KillAgentThreads(self.active_agent_thread_ids()));
            }
            "agents:tree" => {
                self.toggle_agent_tree_dialog();
                return Some(AppAction::None);
            }
            "chat:messageActions" => {
                self.enter_message_actions();
                return Some(AppAction::None);
            }
            "messageActions:prev" => {
                self.select_previous_message(false);
                return Some(AppAction::None);
            }
            "messageActions:next" => {
                self.select_next_message(false);
                return Some(AppAction::None);
            }
            "messageActions:prevUser" => {
                self.select_previous_message(true);
                return Some(AppAction::None);
            }
            "messageActions:nextUser" => {
                self.select_next_message(true);
                return Some(AppAction::None);
            }
            "messageActions:top" => {
                self.conversation
                    .set_selection(self.first_selectable_message());
                self.conversation.set_selected_expanded(false);
                self.scroll_selected_message_into_view();
                return Some(AppAction::None);
            }
            "messageActions:bottom" => {
                self.conversation
                    .set_selection(self.last_selectable_message());
                self.conversation.set_selected_expanded(false);
                self.scroll_selected_message_into_view();
                return Some(AppAction::None);
            }
            "messageActions:escape" => {
                if self.conversation.selected_expanded() {
                    self.conversation.set_selected_expanded(false);
                } else {
                    self.conversation.set_selection(None);
                }
                self.dirty = true;
                return Some(AppAction::None);
            }
            "messageActions:ctrlc" => {
                self.conversation.set_selection(None);
                self.conversation.set_selected_expanded(false);
                self.dirty = true;
                return Some(AppAction::None);
            }
            "messageActions:enter" => {
                self.conversation
                    .set_selected_expanded(!self.conversation.selected_expanded());
                self.dirty = true;
                return Some(AppAction::None);
            }
            "messageActions:o" => {
                if let Some(text) = self
                    .conversation
                    .selection()
                    .and_then(|idx| self.conversation.messages().get(idx))
                    .and_then(message_primary_reference)
                {
                    return Some(AppAction::OpenPath(text));
                }
                return Some(AppAction::None);
            }
            "messageActions:c" => {
                if let Some(message) = self
                    .conversation
                    .selection()
                    .and_then(|idx| self.conversation.messages().get(idx))
                {
                    return Some(AppAction::CopyMessage(message_copy_text(message)));
                }
                return Some(AppAction::None);
            }
            "messageActions:rawCopy" => {
                if let Some(message) = self
                    .conversation
                    .selection()
                    .and_then(|idx| self.conversation.messages().get(idx))
                {
                    return Some(AppAction::CopyMessage(message_copy_text_with_mode(
                        message,
                        CopyTextMode::Raw,
                    )));
                }
                return Some(AppAction::None);
            }
            "messageActions:p" => {
                if let Some(text) = self
                    .conversation
                    .selection()
                    .and_then(|idx| self.conversation.messages().get(idx))
                    .and_then(message_primary_reference)
                {
                    return Some(AppAction::CopyMessage(text));
                }
                return Some(AppAction::None);
            }
            "voice:pushToTalk" => {
                if self.is_voice_ready() {
                    match self
                        .voice
                        .as_ref()
                        .map(|voice| voice.state())
                        .unwrap_or(allthecodes_voice::VoiceState::Idle)
                    {
                        allthecodes_voice::VoiceState::Idle
                        | allthecodes_voice::VoiceState::Error(_) => {
                            self.begin_push_to_talk();
                        }
                        allthecodes_voice::VoiceState::Recording => self.end_push_to_talk(),
                        allthecodes_voice::VoiceState::Transcribing => {}
                    }
                }
                return Some(AppAction::None);
            }
            "transcript:exit" => {
                self.transcript_state.clear_search();
                self.view_mode = ViewMode::Prompt;
                self.dirty = true;
                return Some(AppAction::None);
            }
            "scroll:pageUp" => {
                let step = self.terminal_env.scroll_speed.max(5) as usize;
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_up(step);
                } else {
                    self.scroll_up(step);
                }
                return Some(AppAction::ScrollUp);
            }
            "scroll:pageDown" => {
                let step = self.terminal_env.scroll_speed.max(5) as usize;
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_down(step);
                } else {
                    self.scroll_down(step);
                }
                return Some(AppAction::ScrollDown);
            }
            "scroll:lineUp" => {
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_up(1);
                } else {
                    self.scroll_up(1);
                }
                return Some(AppAction::ScrollUp);
            }
            "scroll:lineDown" => {
                if self.view_mode.is_transcript_like() {
                    self.scroll_transcript_down(1);
                } else {
                    self.scroll_down(1);
                }
                return Some(AppAction::ScrollDown);
            }
            "scroll:top" => {
                if self.view_mode.is_transcript_like() {
                    self.transcript_state.scroll_offset = 0;
                } else {
                    self.conversation.set_scroll_offset(0);
                }
                self.dirty = true;
                return Some(AppAction::None);
            }
            "scroll:bottom" => {
                if self.view_mode.is_transcript_like() {
                    self.transcript_state.scroll_offset = usize::MAX;
                } else {
                    self.conversation.set_scroll_offset(usize::MAX);
                }
                self.dirty = true;
                return Some(AppAction::None);
            }
            _ => {}
        }

        None
    }

    pub(super) fn open_history_search(&mut self) {
        let entries = self
            .session_ui
            .history
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        self.overlays.history_search_dialog = Some(HistorySearchDialog::from_entries(
            entries,
            self.prompt.expanded_text(),
            current_unix_secs(),
        ));
        self.command_palette.close();
        self.overlays.command_surface = None;
        self.dirty = true;
    }

    pub(super) fn apply_vim_action(&mut self, action: VimAction) -> Option<AppAction> {
        match action {
            VimAction::None => Some(AppAction::None),
            VimAction::InsertChar(c) => {
                self.prompt.insert_str(&c.to_string());
                Some(AppAction::None)
            }
            VimAction::Delete { start, end } => {
                if start <= end && end <= self.prompt.input.len() {
                    self.prompt.delete_range(start..end);
                    self.prompt
                        .set_cursor_position(start.min(self.prompt.input.len()));
                    self.prompt.reset_vertical_navigation();
                }
                Some(AppAction::None)
            }
            VimAction::MoveCursor(pos) => {
                self.prompt
                    .set_cursor_position(pos.min(self.prompt.input.len()));
                self.prompt.reset_vertical_navigation();
                Some(AppAction::None)
            }
            VimAction::Yank { .. } | VimAction::YankLine | VimAction::Undo => Some(AppAction::None),
            VimAction::Paste(text) => {
                self.prompt.insert_str(&text);
                Some(AppAction::None)
            }
            VimAction::DeleteLine => {
                self.prompt.clear();
                self.prompt.reset_vertical_navigation();
                Some(AppAction::None)
            }
            VimAction::Submit => Some(
                self.take_prompt_submission()
                    .map_or(AppAction::None, AppAction::Submit),
            ),
            VimAction::SwitchMode(_) => Some(AppAction::None),
            VimAction::Passthrough(key) => Some(
                self.prompt
                    .handle_key(key)
                    .map_or(AppAction::None, AppAction::Submit),
            ),
        }
    }

    fn enter_message_actions(&mut self) {
        self.conversation
            .set_selection(self.last_selectable_message());
        self.conversation.set_selected_expanded(false);
        self.scroll_selected_message_into_view();
        self.dirty = true;
    }

    fn first_selectable_message(&self) -> Option<usize> {
        self.conversation
            .messages()
            .iter()
            .enumerate()
            .find_map(|(idx, message)| is_selectable_message(message).then_some(idx))
    }

    fn last_selectable_message(&self) -> Option<usize> {
        self.conversation
            .messages()
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, message)| is_selectable_message(message).then_some(idx))
    }

    fn select_previous_message(&mut self, user_only: bool) {
        let messages = self.conversation.messages();
        let start = self
            .conversation
            .selection()
            .unwrap_or_else(|| messages.len().saturating_sub(1));
        let selected = (0..start)
            .rev()
            .find(|idx| selectable_by_mode(&messages[*idx], user_only))
            .or(self.conversation.selection())
            .or_else(|| self.last_selectable_message());
        self.conversation.set_selection(selected);
        self.conversation.set_selected_expanded(false);
        self.scroll_selected_message_into_view();
        self.dirty = true;
    }

    fn select_next_message(&mut self, user_only: bool) {
        let messages = self.conversation.messages();
        let start = self
            .conversation
            .selection()
            .map_or(0, |idx| idx.saturating_add(1));
        let selected = (start..messages.len())
            .find(|idx| selectable_by_mode(&messages[*idx], user_only))
            .or(self.conversation.selection())
            .or_else(|| self.first_selectable_message());
        self.conversation.set_selection(selected);
        self.conversation.set_selected_expanded(false);
        self.scroll_selected_message_into_view();
        self.dirty = true;
    }

    fn scroll_selected_message_into_view(&mut self) {
        if let Some(idx) = self.conversation.selection() {
            let line = self.conversation.vscroll().visual_offset_of(idx);
            if self.view_mode.is_transcript_like() {
                self.transcript_state.scroll_offset = line.saturating_sub(1);
            } else {
                self.conversation.set_scroll_offset(line.saturating_sub(1));
            }
        }
    }
}

fn selectable_by_mode(message: &allthecodes_types::message::Message, user_only: bool) -> bool {
    if user_only {
        matches!(message, allthecodes_types::message::Message::User(_))
            && is_selectable_message(message)
    } else {
        is_selectable_message(message)
    }
}

fn is_selectable_message(message: &allthecodes_types::message::Message) -> bool {
    match message {
        allthecodes_types::message::Message::User(user) => {
            !user.is_meta
                && !message_copy_text(message).trim().is_empty()
                && message_copy_text(message).trim() != "[Request interrupted by user]"
        }
        allthecodes_types::message::Message::Assistant(assistant) => !assistant.content.is_empty(),
        allthecodes_types::message::Message::System(_)
        | allthecodes_types::message::Message::Attachment(_) => true,
        allthecodes_types::message::Message::Progress(_) => false,
    }
}
