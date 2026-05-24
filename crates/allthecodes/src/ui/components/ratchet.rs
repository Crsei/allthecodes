//! Ratchet — progressive height-locking component.
//!
//! Ensures that a content area never shrinks below its maximum observed height.
//! This prevents layout "collapse" when content transitions from tall to short.
//!
//! Mirrors the upstream TypeScript `Ratchet.tsx` design-system component.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Lock mode for the ratchet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatchetLock {
    /// Always engage the height lock (default).
    Always,
    /// Only engage when the content is scrolled offscreen.
    Offscreen,
}

/// A ratchet that prevents content height from shrinking.
///
/// Tracks the maximum height observed across renders. When new content is
/// shorter, the remaining space is filled with blank lines so the total height
/// stays at the maximum.
///
/// # Example
///
/// ```ignore
/// let mut ratchet = Ratchet::new(RatchetLock::Always);
/// let lines = ratchet.render(content_lines, &theme);
/// ```
#[derive(Debug, Clone)]
pub struct Ratchet {
    /// Lock mode.
    lock: RatchetLock,
    /// Maximum content height seen so far (in lines).
    max_height: usize,
    /// Whether the component is currently visible on screen.
    /// When `false` and lock is `Offscreen`, the ratchet is disengaged.
    is_visible: bool,
}

impl Ratchet {
    /// Create a new ratchet with the given lock mode.
    pub fn new(lock: RatchetLock) -> Self {
        Self {
            lock,
            max_height: 0,
            is_visible: true,
        }
    }

    /// Reset the max height tracking (e.g., on content identity change).
    pub fn reset(&mut self) {
        self.max_height = 0;
    }

    /// Set whether the component is currently visible on screen.
    /// Only relevant when `lock` is `Offscreen`.
    pub fn set_visible(&mut self, visible: bool) {
        self.is_visible = visible;
    }

    /// Current maximum observed height.
    pub fn max_height(&self) -> usize {
        self.max_height
    }

    /// Render content with ratchet height locking.
    ///
    /// Records the height of `lines`. On subsequent calls, if new content is
    /// shorter than the recorded maximum, blank padding lines are appended.
    pub fn render(&mut self, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let content_height = lines.len();
        let engaged = self.lock == RatchetLock::Always
            || (self.lock == RatchetLock::Offscreen && !self.is_visible);

        if engaged && content_height > self.max_height {
            self.max_height = content_height;
        }

        if !engaged || content_height >= self.max_height {
            return lines;
        }

        // Pad with blank lines to maintain max height.
        let padding = self.max_height.saturating_sub(content_height);
        let mut result = lines;
        for _ in 0..padding {
            result.push(Line::from(Span::styled(" ", Style::default())));
        }
        result
    }
}

impl Default for Ratchet {
    fn default() -> Self {
        Self::new(RatchetLock::Always)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::style::Style;
    use ratatui::text::Span;

    fn line(text: &str) -> Line<'static> {
        Line::from(Span::styled(text.to_string(), Style::default()))
    }

    fn fmt_lines(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                let text = l
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>();
                if !text.is_empty() && text.trim().is_empty() {
                    format!("<blank:{}>", text.len())
                } else {
                    text
                }
            })
            .collect()
    }

    #[test]
    fn snapshot_ratchet_scenarios() {
        let mut r = Ratchet::new(RatchetLock::Always);
        let r1 = r.render(vec![line("short")]);
        let r2 = r.render(vec![line("line 1"), line("line 2"), line("line 3")]);
        let r3 = r.render(vec![line("only one")]);

        let mut off = Ratchet::new(RatchetLock::Offscreen);
        off.set_visible(true);
        let o1 = off.render(vec![line("a"), line("b"), line("c")]);
        let o2 = off.render(vec![line("x")]);
        off.set_visible(false);
        let o3 = off.render(vec![line("y")]);

        assert_snapshot!(
            "ratchet_scenarios",
            format!(
                "## grow then shrink\n{}\n---\n{}\n---\n{}\n\n## offscreen (visible)\n{}\n---\n{}\n\n## offscreen (hidden)\n{}",
                fmt_lines(&r1).join("\n"),
                fmt_lines(&r2).join("\n"),
                fmt_lines(&r3).join("\n"),
                fmt_lines(&o1).join("\n"),
                fmt_lines(&o2).join("\n"),
                fmt_lines(&o3).join("\n"),
            )
        );
    }

    #[test]
    fn initial_render_passes_through() {
        let mut r = Ratchet::new(RatchetLock::Always);
        let content = vec![line("a"), line("b")];
        let result = r.render(content);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn taller_content_increases_max() {
        let mut r = Ratchet::new(RatchetLock::Always);
        r.render(vec![line("a")]);
        let result = r.render(vec![line("a"), line("b"), line("c")]);
        assert_eq!(result.len(), 3);
        assert_eq!(r.max_height(), 3);
    }

    #[test]
    fn shorter_content_is_padded() {
        let mut r = Ratchet::new(RatchetLock::Always);
        r.render(vec![line("a"), line("b"), line("c")]);
        assert_eq!(r.max_height(), 3);
        let result = r.render(vec![line("x")]);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn offscreen_lock_disengages_when_visible() {
        let mut r = Ratchet::new(RatchetLock::Offscreen);
        r.set_visible(true);
        r.render(vec![line("a"), line("b"), line("c")]);
        // Visible + Offscreen => disengaged, so no padding.
        let result = r.render(vec![line("x")]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn offscreen_lock_engages_when_not_visible() {
        let mut r = Ratchet::new(RatchetLock::Offscreen);
        r.set_visible(false);
        r.render(vec![line("a"), line("b"), line("c")]);
        let result = r.render(vec![line("x")]);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn reset_clears_max_height() {
        let mut r = Ratchet::new(RatchetLock::Always);
        r.render(vec![line("a"), line("b")]);
        assert_eq!(r.max_height(), 2);
        r.reset();
        assert_eq!(r.max_height(), 0);
        let result = r.render(vec![line("x")]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn max_height_tracks_growth_only() {
        let mut r = Ratchet::new(RatchetLock::Always);
        r.render(vec![line("a")]);
        r.render(vec![line("b"), line("c")]);
        r.render(vec![line("d")]); // shorter
        assert_eq!(r.max_height(), 2);
    }
}
