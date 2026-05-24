//! Loading state component with spinner animation and optional subtitle.
//!
//! Mirrors the upstream TypeScript `LoadingState.tsx` design-system component.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::ThemeColors;

/// Braille spinner animation frames (same as rendering/spinner.rs).
const SPINNER_FRAMES: &[&str] = &[
    "\u{280B}", // ⠋
    "\u{2819}", // ⠙
    "\u{2839}", // ⠹
    "\u{2838}", // ⠸
    "\u{283C}", // ⠼
    "\u{2834}", // ⠴
    "\u{2826}", // ⠦
    "\u{2827}", // ⠧
    "\u{2807}", // ⠇
    "\u{280F}", // ⠏
];

/// A spinner with loading message for async operations.
///
/// # Example
///
/// ```ignore
/// let state = LoadingState::new("Loading sessions")
///     .bold(true)
///     .subtitle("Fetching your sessions...");
/// let lines = state.render(&theme);
/// ```
pub struct LoadingState {
    /// The loading message to display next to the spinner.
    message: String,
    /// Display the message in bold.
    bold: bool,
    /// Display the message in dimmed color.
    dim_color: bool,
    /// Optional subtitle displayed below the main message.
    subtitle: Option<String>,
    /// Current animation frame index (0-9).
    frame: usize,
}

impl LoadingState {
    /// Create a new loading state with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            bold: false,
            dim_color: false,
            subtitle: None,
            frame: 0,
        }
    }

    /// Display the message in bold.
    pub fn bold(mut self, v: bool) -> Self {
        self.bold = v;
        self
    }

    /// Display the message in dimmed color.
    pub fn dim_color(mut self, v: bool) -> Self {
        self.dim_color = v;
        self
    }

    /// Optional subtitle displayed below the main message.
    pub fn subtitle(mut self, v: impl Into<String>) -> Self {
        self.subtitle = Some(v.into());
        self
    }

    /// Set the current animation frame index (clamped to frame count).
    /// Call this externally on each tick to animate.
    pub fn set_frame(&mut self, frame: usize) {
        self.frame = frame % SPINNER_FRAMES.len();
    }

    /// Current frame index for the spinner animation.
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// Number of spinner frames (useful for tick management).
    pub const fn frame_count() -> usize {
        SPINNER_FRAMES.len()
    }

    /// Render the loading state as styled lines.
    ///
    /// Returns a `Vec<Line>` containing the spinner + message on the first line
    /// and an optional subtitle on the second.
    pub fn render(&self, colors: &ThemeColors) -> Vec<Line<'static>> {
        let frame_char = SPINNER_FRAMES[self.frame % SPINNER_FRAMES.len()];

        let mut msg_style = Style::default();
        if self.bold {
            msg_style = msg_style.add_modifier(Modifier::BOLD);
        }
        msg_style = if self.dim_color {
            msg_style.fg(colors.dim)
        } else {
            msg_style.fg(colors.surfaceText)
        };

        let main_line = Line::from(vec![
            Span::styled(
                format!("{} ", frame_char),
                Style::default().fg(colors.accent),
            ),
            Span::styled(self.message.clone(), msg_style),
        ]);

        let mut lines = vec![main_line];

        if let Some(subtitle) = &self.subtitle {
            lines.push(Line::from(Span::styled(
                format!("  {}", subtitle),
                Style::default().fg(colors.dim),
            )));
        }

        lines
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};
    use insta::assert_snapshot;

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    fn render_snapshot(states: &[LoadingState], colors: &ThemeColors) -> String {
        states
            .iter()
            .map(|ls| {
                ls.render(colors)
                    .iter()
                    .map(|l| {
                        l.spans
                            .iter()
                            .map(|s| s.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n---\n")
    }

    #[test]
    fn snapshot_loading_states() {
        let states = vec![
            LoadingState::new("Loading sessions"),
            LoadingState::new("Working").bold(true),
            LoadingState::new("Loading")
                .bold(true)
                .subtitle("Please wait..."),
            LoadingState::new("Syncing").dim_color(true),
            LoadingState::new("Deep thought")
                .bold(true)
                .dim_color(true)
                .subtitle("This may take a moment"),
        ];
        assert_snapshot!("loading_state_variants", render_snapshot(&states, dark()));
    }

    #[test]
    fn renders_loading_message() {
        let ls = LoadingState::new("Loading...");
        let lines = ls.render(dark());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn renders_with_subtitle() {
        let ls = LoadingState::new("Working").subtitle("Please wait");
        let lines = ls.render(dark());
        assert_eq!(lines.len(), 2);
        let sub_text: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(sub_text.contains("Please wait"));
    }

    #[test]
    fn bold_renders_with_bold_modifier() {
        let ls = LoadingState::new("Loading").bold(true);
        let lines = ls.render(dark());
        let msg_span = &lines[0].spans[1];
        assert!(msg_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn dim_color_uses_dim_fg() {
        let ls = LoadingState::new("Loading").dim_color(true);
        let lines = ls.render(dark());
        let msg_span = &lines[0].spans[1];
        assert_eq!(msg_span.style.fg, Some(dark().dim));
    }

    #[test]
    fn frame_wraps_around() {
        let mut ls = LoadingState::new("Loading");
        assert_eq!(ls.frame(), 0);
        ls.set_frame(SPINNER_FRAMES.len());
        assert_eq!(ls.frame(), 0);
        ls.set_frame(SPINNER_FRAMES.len() + 3);
        assert_eq!(ls.frame(), 3);
    }

    #[test]
    fn frame_count_is_positive() {
        assert!(LoadingState::frame_count() > 0);
    }

    #[test]
    fn spinner_char_changes_with_frame() {
        let mut ls = LoadingState::new("Test");
        ls.set_frame(0);
        let frame0 = ls.render(dark())[0].spans[0].content.clone();
        ls.set_frame(1);
        let frame1 = ls.render(dark())[0].spans[0].content.clone();
        assert_ne!(frame0, frame1);
    }
}
