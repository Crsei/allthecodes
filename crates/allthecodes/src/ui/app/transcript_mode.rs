use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::transcript::{self, SearchMatch, TranscriptInputMode, ViewMode};

use super::{App, AppAction};

impl App {
    /// Dispatch a key event to the transcript handler. Returns an
    /// [`AppAction`] the TUI runner must act on (Quit / export / None).
    pub(super) fn handle_transcript_key(&mut self, key: KeyEvent) -> AppAction {
        use KeyCode::*;
        let step = self.terminal_env.scroll_speed.max(1) as usize;

        // Search input mode; keystrokes build the query.
        if matches!(
            self.transcript_state.input_mode,
            TranscriptInputMode::Search
        ) {
            match (key.modifiers, key.code) {
                (_, Esc) => {
                    self.transcript_state.clear_search();
                }
                (_, Enter) => {
                    self.commit_search();
                }
                (_, Backspace) => {
                    self.transcript_state.query.pop();
                }
                (m, Char(c)) if !m.contains(KeyModifiers::CONTROL) => {
                    self.transcript_state.query.push(c);
                }
                _ => {}
            }
            return AppAction::None;
        }

        // Normal transcript navigation.
        match (key.modifiers, key.code) {
            // Exit transcript back to prompt.
            (_, Esc) | (_, Char('q')) => {
                // Hop straight back to Prompt (not the next cycle step)
                // since Esc / q are the documented "leave" keys.
                self.transcript_state.clear_search();
                self.view_mode = ViewMode::Prompt;
                self.dirty = true;
            }
            // Start a search.
            (_, Char('/')) => {
                self.transcript_state.input_mode = TranscriptInputMode::Search;
                self.transcript_state.query.clear();
                self.transcript_state.matches.clear();
                self.transcript_state.focused = None;
                self.dirty = true;
            }
            // Navigate search hits.
            (_, Char('n')) => {
                self.transcript_state.next_match();
                self.snap_to_current_match();
            }
            (_, Char('N')) => {
                self.transcript_state.prev_match();
                self.snap_to_current_match();
            }
            // Export to editor.
            (_, Char('e')) => {
                let body = transcript::render_markdown_dump(self.conversation.messages());
                return AppAction::ExportTranscript(body);
            }
            // Less-style scroll.
            (_, Char('j')) | (_, Down) => {
                self.scroll_transcript_down(1);
            }
            (_, Char('k')) | (_, Up) => {
                self.scroll_transcript_up(1);
            }
            (_, PageDown) => {
                self.scroll_transcript_down(step.max(5));
            }
            (_, PageUp) => {
                self.scroll_transcript_up(step.max(5));
            }
            (KeyModifiers::CONTROL, Char('d')) => {
                self.scroll_transcript_down(step.max(5));
            }
            (KeyModifiers::CONTROL, Char('u')) => {
                self.scroll_transcript_up(step.max(5));
            }
            (_, Home) => {
                self.transcript_state.scroll_offset = 0;
                self.dirty = true;
            }
            (_, End) | (_, Char('G')) => {
                self.transcript_state.scroll_offset = usize::MAX;
                self.dirty = true;
            }
            (_, Char('g')) => {
                self.transcript_state.scroll_offset = 0;
                self.dirty = true;
            }
            _ => {}
        }
        AppAction::None
    }

    pub(super) fn scroll_transcript_down(&mut self, lines: usize) {
        self.transcript_state.scroll_offset =
            self.transcript_state.scroll_offset.saturating_add(lines);
        self.dirty = true;
    }

    pub(super) fn scroll_transcript_up(&mut self, lines: usize) {
        self.transcript_state.scroll_offset =
            self.transcript_state.scroll_offset.saturating_sub(lines);
        self.dirty = true;
    }

    pub(super) fn commit_search(&mut self) {
        let hits =
            transcript::search_messages(self.conversation.messages(), &self.transcript_state.query);
        self.transcript_state.set_matches(hits);
        self.transcript_state.input_mode = TranscriptInputMode::Normal;
        self.snap_to_current_match();
    }

    /// Move the viewport so the currently-focused match is visible. We
    /// snap to the match's message start line; the messages pane handles
    /// the rest of the clamp.
    pub(super) fn snap_to_current_match(&mut self) {
        if let Some(SearchMatch { message_index }) = self.transcript_state.current_match() {
            let line = self.conversation.vscroll().visual_offset_of(message_index);
            // Bias slightly upward so the line has context above it.
            self.transcript_state.scroll_offset = line.saturating_sub(1);
            self.dirty = true;
        }
    }
}
