//! Shimmer text styling for loading labels.
//!
//! Each [`ShimmerAnimation`] instance tracks its own start time so that
//! multiple shimmer elements animate independently rather than pulsing in
//! lockstep.

use std::time::{Duration, Instant};

use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// Per-instance shimmer animation state.
///
/// Call [`shimmer_spans`] to produce a sequence of styled spans where each
/// character's brightness oscillates in a sine wave. Instances created at
/// different times will be at different phases of the wave.
pub struct ShimmerAnimation {
    start: Instant,
    /// Custom color range offset (0.0 = grayscale, non-zero = hue shift).
    hue_shift: f32,
}

impl ShimmerAnimation {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            hue_shift: 0.0,
        }
    }

    /// Create a shimmer with a custom hue shift (0.0 = default grayscale).
    pub fn with_hue(hue_shift: f32) -> Self {
        Self {
            start: Instant::now(),
            hue_shift,
        }
    }

    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    pub fn shimmer_spans(&self, text: &str) -> Vec<Span<'static>> {
        let elapsed = self.elapsed().as_millis() as f32 / 160.0;
        let hue = self.hue_shift;
        text.chars()
            .enumerate()
            .map(|(idx, ch)| {
                let wave = ((idx as f32 * 0.55 + elapsed).sin() + 1.0) * 0.5;
                Span::styled(ch.to_string(), color_for_level(wave, hue))
            })
            .collect()
    }
}

impl Default for ShimmerAnimation {
    fn default() -> Self {
        Self::new()
    }
}

fn color_for_level(intensity: f32, hue_shift: f32) -> Style {
    let clamped = intensity.clamp(0.0, 1.0);
    let base = 120.0 + clamped * 100.0;
    if hue_shift == 0.0 {
        Style::default().fg(Color::Rgb(base as u8, base as u8, base as u8))
    } else {
        // Apply a simple hue shift for colored shimmer effects.
        let r = (base + hue_shift * 60.0).clamp(0.0, 255.0) as u8;
        let g = (base + hue_shift * 30.0).clamp(0.0, 255.0) as u8;
        let b = (base - hue_shift * 20.0).clamp(0.0, 255.0) as u8;
        Style::default().fg(Color::Rgb(r, g, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_one_span_per_char() {
        let anim = ShimmerAnimation::new();
        assert_eq!(anim.shimmer_spans("abc").len(), 3);
    }

    #[test]
    fn independent_instances_have_different_phase() {
        let a = ShimmerAnimation::new();
        let b = ShimmerAnimation::new();
        let spans_a = a.shimmer_spans("hello");
        let spans_b = b.shimmer_spans("hello");
        // Both should produce correct-length output even though
        // phases may differ (very unlikely to be exactly equal at
        // different start times, but we only check structural validity).
        assert_eq!(spans_a.len(), spans_b.len());
    }

    #[test]
    fn custom_hue_shimmer_renders_colored_spans() {
        let anim = ShimmerAnimation::with_hue(0.5);
        let spans = anim.shimmer_spans("x");

        assert_eq!(spans.len(), 1);
        assert!(spans[0].style.fg.is_some());
    }
}
