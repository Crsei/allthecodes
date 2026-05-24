use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::theme::Theme;

/// Braille-dot spinner frames, matching the original TypeScript implementation.
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

/// Animated spinner state.
///
/// Call [`tick`] on a regular interval (e.g. every 80ms) to advance the
/// animation frame. The spinner renders a single line: the animation character
/// followed by an optional message.
pub struct SpinnerState {
    /// Current animation frame index (wraps around `SPINNER_FRAMES`).
    pub frame: usize,
    /// Text to display next to the spinner character.
    pub message: String,
    /// Whether the spinner is currently active/visible.
    pub active: bool,
}

impl SpinnerState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            message: String::new(),
            active: false,
        }
    }

    /// Advance the spinner animation by one frame.
    pub fn tick(&mut self) {
        if self.active {
            self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
        }
    }

    /// Set the message displayed next to the spinner.
    pub fn set_message(&mut self, msg: String) {
        self.message = msg;
    }

    /// Start the spinner with an optional message.
    pub fn start(&mut self, message: Option<String>) {
        self.active = true;
        self.frame = 0;
        if let Some(msg) = message {
            self.message = msg;
        }
    }

    /// Stop the spinner.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Render the spinner into the given buffer area.
    ///
    /// If the spinner is inactive nothing is rendered.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        if !self.active || area.height == 0 || area.width == 0 {
            return;
        }

        let frame_char = SPINNER_FRAMES[self.frame % SPINNER_FRAMES.len()];

        let spans = vec![
            Span::styled(format!("{} ", frame_char), theme.info),
            Span::styled(self.message.clone(), theme.dim),
        ];

        let line = Line::from(spans);
        // Render on the first row of the provided area.
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self::new()
    }
}
