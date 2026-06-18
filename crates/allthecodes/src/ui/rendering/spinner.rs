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
    base_message: String,
    /// Whether the spinner is currently active/visible.
    pub active: bool,
    tips_enabled: bool,
    tip_interval_ms: u64,
    custom_tips: Vec<String>,
    current_tip: Option<usize>,
    elapsed_since_tip_ms: u64,
}

impl SpinnerState {
    const DEFAULT_TIP_INTERVAL_MS: u64 = 10_000;

    pub fn new() -> Self {
        Self {
            frame: 0,
            message: String::new(),
            base_message: String::new(),
            active: false,
            tips_enabled: true,
            tip_interval_ms: Self::DEFAULT_TIP_INTERVAL_MS,
            custom_tips: Vec::new(),
            current_tip: None,
            elapsed_since_tip_ms: 0,
        }
    }

    /// Advance the spinner animation by one frame.
    pub fn tick(&mut self) {
        if self.active {
            self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
        }
    }

    /// Advance the tip timer by `elapsed_ms`.
    pub fn tick_tip(&mut self, elapsed_ms: u64) {
        if !self.active || !self.tips_enabled || self.custom_tips.is_empty() {
            self.current_tip = None;
            self.sync_message();
            return;
        }

        self.elapsed_since_tip_ms = self.elapsed_since_tip_ms.saturating_add(elapsed_ms);
        if self.current_tip.is_none() {
            self.current_tip = Some(0);
            self.elapsed_since_tip_ms = 0;
            self.sync_message();
            return;
        }

        if self.elapsed_since_tip_ms >= self.tip_interval_ms {
            let next = (self.current_tip.unwrap_or(0) + 1) % self.custom_tips.len();
            self.current_tip = Some(next);
            self.elapsed_since_tip_ms = 0;
            self.sync_message();
        }
    }

    /// Set the message displayed next to the spinner.
    pub fn set_message(&mut self, msg: String) {
        self.base_message = msg;
        self.sync_message();
    }

    /// Apply runtime tip settings from `settings.json::spinnerTips`.
    pub fn configure_tips(
        &mut self,
        enabled: Option<bool>,
        interval_ms: Option<u64>,
        custom_tips: Vec<String>,
    ) {
        self.tips_enabled = enabled.unwrap_or(true);
        self.tip_interval_ms = interval_ms
            .filter(|value| *value > 0)
            .unwrap_or(Self::DEFAULT_TIP_INTERVAL_MS);
        self.custom_tips = custom_tips
            .into_iter()
            .map(|tip| tip.trim().to_string())
            .filter(|tip| !tip.is_empty())
            .collect();
        self.current_tip = None;
        self.elapsed_since_tip_ms = 0;
        self.sync_message();
    }

    /// Start the spinner with an optional message.
    pub fn start(&mut self, message: Option<String>) {
        self.active = true;
        self.frame = 0;
        self.current_tip = None;
        self.elapsed_since_tip_ms = 0;
        if let Some(msg) = message {
            self.base_message = msg;
        }
        self.sync_message();
    }

    /// Stop the spinner.
    pub fn stop(&mut self) {
        self.active = false;
        self.current_tip = None;
        self.elapsed_since_tip_ms = 0;
        self.sync_message();
    }

    fn sync_message(&mut self) {
        let base = self.base_message.clone();
        if !self.tips_enabled || self.custom_tips.is_empty() {
            self.message = base;
            return;
        }

        if let Some(index) = self.current_tip {
            if let Some(tip) = self.custom_tips.get(index) {
                self.message = if base.trim().is_empty() {
                    format!("Tip: {tip}")
                } else {
                    format!("{base}  Tip: {tip}")
                };
                return;
            }
        }

        self.message = base;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_tips_preserve_base_message_and_rotate() {
        let mut spinner = SpinnerState::new();
        spinner.configure_tips(Some(true), Some(50), vec!["one".into(), "two".into()]);
        spinner.start(Some("Thinking...".into()));

        spinner.tick_tip(16);
        assert_eq!(spinner.message, "Thinking...  Tip: one");
        spinner.tick_tip(50);
        assert_eq!(spinner.message, "Thinking...  Tip: two");
    }

    #[test]
    fn spinner_tips_can_be_disabled() {
        let mut spinner = SpinnerState::new();
        spinner.configure_tips(Some(false), Some(50), vec!["one".into()]);
        spinner.start(Some("Thinking...".into()));
        spinner.tick_tip(50);

        assert_eq!(spinner.message, "Thinking...");
    }
}
