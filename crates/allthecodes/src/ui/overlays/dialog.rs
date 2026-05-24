//! Generic modal dialog component.
//!
//! Mirrors the upstream TypeScript `Dialog.tsx` design-system component.
//! Provides a themed dialog container with title, subtitle, keyboard-based
//! cancellation (Esc / `n`), "press again to exit" safety (Ctrl+C/D),
//! configurable input guide hints, and optional border hiding.
//!
//! Dialog instances live inside an [`OverlayStack`] which manages z-ordering
//! and event routing.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::Style;
use ratatui::text::Line;

use crate::ui::keyboard_shortcut::{render_hints_styled, ShortcutHint};
use crate::ui::pane::Pane;
use crate::ui::theme::ThemeColors;

// ---------------------------------------------------------------------------
// ExitGuard
// ---------------------------------------------------------------------------

/// Tracks the "press again to exit" state for Ctrl+C/D.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitGuard {
    /// Whether the first Ctrl+C/D press has been received.
    pub armed: bool,
}

impl ExitGuard {
    pub fn new() -> Self {
        Self { armed: false }
    }

    /// Arm the guard (first press detected).
    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// Disarm (e.g. on any other key press or after timeout).
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// Returns `true` if this press should trigger exit (second press).
    pub fn confirm(&mut self) -> bool {
        if self.armed {
            true
        } else {
            self.arm();
            false
        }
    }
}

impl Default for ExitGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of routing a key through a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEvent {
    /// The dialog did not consume the key.
    None,
    /// The user requested cancellation through Esc / `n`.
    Cancel,
    /// The user confirmed the guarded exit chord.
    Exit,
}

// ---------------------------------------------------------------------------
// Dialog
// ---------------------------------------------------------------------------

/// A generic modal dialog.
///
/// # Example
///
/// ```ignore
/// let dialog = Dialog::new("Confirm")
///     .subtitle("Are you sure?")
///     .color("permission");
/// let lines = dialog.render(&theme, term_width, content_lines, &mut exit_guard);
/// ```
pub struct Dialog<'a> {
    /// Dialog title (bold, top line).
    title: Option<&'a str>,
    /// Optional subtitle (dimmed, below title).
    subtitle: Option<&'a str>,
    /// Theme colour key for the top border and accent.
    color: Option<&'a str>,
    /// Whether to hide the top border.
    hide_border: bool,
    /// Whether to hide the keyboard input guide at the bottom.
    hide_input_guide: bool,
    /// When `true`, the cancel action (Esc / `n`) is disabled (e.g. when an
    /// inner TextInput is focused).
    is_cancel_active: bool,
    /// Custom keyboard hints for the input guide (default: Enter/Esc).
    input_guide_hints: Option<Vec<ShortcutHint<'a>>>,
}

impl<'a> Dialog<'a> {
    pub fn new() -> Self {
        Self {
            title: None,
            subtitle: None,
            color: None,
            hide_border: false,
            hide_input_guide: false,
            is_cancel_active: true,
            input_guide_hints: None,
        }
    }

    /// Set the dialog title.
    pub fn title(mut self, v: &'a str) -> Self {
        self.title = Some(v);
        self
    }

    /// Set the dialog subtitle.
    pub fn subtitle(mut self, v: &'a str) -> Self {
        self.subtitle = Some(v);
        self
    }

    /// Set the accent colour key.
    pub fn color(mut self, v: &'a str) -> Self {
        self.color = Some(v);
        self
    }

    /// Hide the top border / divider.
    pub fn hide_border(mut self) -> Self {
        self.hide_border = true;
        self
    }

    /// Hide the input guide at the bottom.
    pub fn hide_input_guide(mut self) -> Self {
        self.hide_input_guide = true;
        self
    }

    /// Set whether the cancel action is active (default: true).
    pub fn cancel_active(mut self, v: bool) -> Self {
        self.is_cancel_active = v;
        self
    }

    /// Replace the default input guide hints.
    pub fn input_guide(mut self, hints: Vec<ShortcutHint<'a>>) -> Self {
        self.input_guide_hints = Some(hints);
        self
    }

    /// Render the dialog into lines.
    ///
    /// * `colors` – active theme.
    /// * `term_width` – terminal width in columns.
    /// * `children` – content lines inside the dialog.
    /// * `exit_guard` – mutable guard for "press again to exit".
    /// * `inside_modal` – whether this dialog is already inside a modal stack.
    pub fn render(
        &self,
        colors: &ThemeColors,
        term_width: usize,
        children: Vec<Line<'static>>,
        exit_guard: &ExitGuard,
        inside_modal: bool,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Title bar.
        if let Some(title) = self.title {
            let title_style = Style::default()
                .fg(colors.accent)
                .add_modifier(ratatui::style::Modifier::BOLD);
            lines.push(Line::from(ratatui::text::Span::styled(
                format!(" {} ", title),
                title_style,
            )));
        }

        // Subtitle.
        if let Some(subtitle) = self.subtitle {
            lines.push(Line::from(ratatui::text::Span::styled(
                format!(" {}", subtitle),
                Style::default().fg(colors.dim),
            )));
            // Blank line after subtitle.
            lines.push(Line::from(""));
        }

        // Content.
        lines.extend(children);

        // Input guide.
        if !self.hide_input_guide {
            // Blank line before guide.
            lines.push(Line::from(""));

            let hints = self
                .input_guide_hints
                .clone()
                .unwrap_or_else(|| self.default_hints(exit_guard));

            let hint_spans = render_hints_styled(&hints, colors);
            if !hint_spans.is_empty() {
                let mut guide_line = Line::from(ratatui::text::Span::raw(" "));
                guide_line.spans.extend(hint_spans);
                lines.push(guide_line);
            }
        }

        // Wrap in Pane.
        if !self.hide_border && !inside_modal {
            let mut pane = Pane::new().padding_x(2).padding_top(0);
            if let Some(k) = self.color {
                pane = pane.color(k);
            }
            pane.render(colors, term_width, lines, false)
        } else {
            // Just add horizontal padding.
            let pad = "  ";
            let mut padded: Vec<Line<'static>> = Vec::with_capacity(lines.len());
            for line in lines {
                let mut p = Line::from(pad);
                p.spans.extend(line.spans);
                padded.push(p);
            }
            padded
        }
    }

    /// Route a key event through the dialog's built-in controls.
    ///
    /// Returns [`DialogEvent::Cancel`] for Esc / `n` when cancellation is
    /// active. Ctrl+C and Ctrl+D use [`ExitGuard`]: the first press arms the
    /// guard and is consumed, the second press returns [`DialogEvent::Exit`].
    /// Any other key disarms the guard so accidental exits require consecutive
    /// exit key presses.
    pub fn handle_key(&self, key: KeyEvent, exit_guard: &mut ExitGuard) -> DialogEvent {
        if key.kind != KeyEventKind::Press {
            return DialogEvent::None;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c' | 'd')) => {
                if exit_guard.confirm() {
                    DialogEvent::Exit
                } else {
                    DialogEvent::None
                }
            }
            (_, KeyCode::Esc) | (_, KeyCode::Char('n')) if self.is_cancel_active => {
                exit_guard.disarm();
                DialogEvent::Cancel
            }
            _ => {
                exit_guard.disarm();
                DialogEvent::None
            }
        }
    }

    /// Build the default input-guide hints based on state.
    fn default_hints(&self, exit_guard: &ExitGuard) -> Vec<ShortcutHint<'static>> {
        if exit_guard.armed {
            return vec![ShortcutHint::new("Ctrl+C", "press again to exit")];
        }

        let mut hints = Vec::new();

        if self.is_cancel_active {
            hints.push(ShortcutHint::new("Esc", "cancel").with_parens());
            hints.push(ShortcutHint::new("n", "no"));
        }

        hints
    }
}

impl<'a> Default for Dialog<'a> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};
    use crossterm::event::{KeyEventState, KeyModifiers};

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ctrl_key(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn dialog_renders_title() {
        let d = Dialog::new().title("Hello");
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![], &guard, true);
        // Title should be present
        let has_title = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("Hello")));
        assert!(has_title);
    }

    #[test]
    fn dialog_renders_subtitle() {
        let d = Dialog::new().title("T").subtitle("desc");
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![], &guard, true);
        let has_sub = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("desc")));
        assert!(has_sub);
    }

    #[test]
    fn dialog_hides_input_guide() {
        let d = Dialog::new().title("T").hide_input_guide();
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![], &guard, true);
        let has_cancel = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("cancel")));
        assert!(!has_cancel);
    }

    #[test]
    fn exit_guard_arms_and_confirms() {
        let mut guard = ExitGuard::new();
        assert!(!guard.armed);
        // First press: arms but doesn't confirm.
        assert!(!guard.confirm());
        assert!(guard.armed);
        // Second press: confirms.
        assert!(guard.confirm());
    }

    #[test]
    fn exit_guard_disarms() {
        let mut guard = ExitGuard::new();
        guard.arm();
        guard.disarm();
        assert!(!guard.armed);
    }

    #[test]
    fn dialog_render_without_modal_uses_pane() {
        let d = Dialog::new().title("Test");
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![], &guard, false);
        // Should have more structure (pane + divider)
        assert!(!lines.is_empty());
    }

    #[test]
    fn dialog_color_and_hidden_border_use_padded_rendering() {
        let d = Dialog::new()
            .title("Test")
            .color("permission")
            .hide_border();
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![Line::from("body")], &guard, false);
        let joined = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(joined.contains("Test"));
        assert!(joined.contains("body"));
    }

    #[test]
    fn dialog_custom_input_guide() {
        let hints = vec![ShortcutHint::new("y", "yes")];
        let d = Dialog::new().title("T").input_guide(hints);
        let guard = ExitGuard::new();
        let lines = d.render(dark(), 40, vec![], &guard, true);
        let has_yes = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("yes")));
        assert!(has_yes);
    }

    #[test]
    fn dialog_exit_guard_shows_press_again() {
        let mut guard = ExitGuard::new();
        guard.arm();
        let d = Dialog::new().title("T");
        let lines = d.render(dark(), 40, vec![], &guard, true);
        let has_press_again = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("press again")));
        assert!(has_press_again);
    }

    #[test]
    fn dialog_key_handling_cancels_when_active() {
        let d = Dialog::new();
        let mut guard = ExitGuard::new();
        assert_eq!(
            d.handle_key(key(KeyCode::Esc), &mut guard),
            DialogEvent::Cancel
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Char('n')), &mut guard),
            DialogEvent::Cancel
        );
    }

    #[test]
    fn dialog_key_handling_respects_cancel_inactive() {
        let d = Dialog::new().cancel_active(false);
        let mut guard = ExitGuard::new();
        assert_eq!(
            d.handle_key(key(KeyCode::Esc), &mut guard),
            DialogEvent::None
        );
        assert_eq!(
            d.handle_key(key(KeyCode::Char('n')), &mut guard),
            DialogEvent::None
        );
    }

    #[test]
    fn dialog_key_handling_requires_consecutive_exit_chords() {
        let d = Dialog::new();
        let mut guard = ExitGuard::new();
        assert_eq!(d.handle_key(ctrl_key('c'), &mut guard), DialogEvent::None);
        assert!(guard.armed);
        assert_eq!(
            d.handle_key(key(KeyCode::Char('x')), &mut guard),
            DialogEvent::None
        );
        assert!(!guard.armed);
        assert_eq!(d.handle_key(ctrl_key('d'), &mut guard), DialogEvent::None);
        assert_eq!(d.handle_key(ctrl_key('d'), &mut guard), DialogEvent::Exit);
    }
}
