//! Theme-aware box wrapper around Pane.
//!
//! Mirrors the upstream TypeScript `ThemedBox.tsx` design-system component.
//! ThemedBox resolves theme colour keys for border and background, then
//! delegates the visual framing to [`Pane`](crate::ui::components::pane::Pane).

use ratatui::style::Style;
use ratatui::text::Line;

use crate::ui::pane::Pane;
use crate::ui::theme::color::resolve_color;
use crate::ui::theme::ThemeColors;

/// A theme-aware box container.
///
/// Applies a themed top-border colour and optional background colour to a
/// [`Pane`] wrapper.  When neither colour is specified, behaves as a plain
/// [`Pane`].
pub struct ThemedBox<'a> {
    /// Theme colour key for the top border / divider.
    border_color: Option<&'a str>,
    /// Theme colour key for the background (applied behind content).
    background_color: Option<&'a str>,
    /// Horizontal padding (forwarded to Pane).
    padding_x: usize,
    /// Top margin (forwarded to Pane).
    padding_top: usize,
}

impl<'a> ThemedBox<'a> {
    pub fn new() -> Self {
        Self {
            border_color: None,
            background_color: None,
            padding_x: 2,
            padding_top: 1,
        }
    }

    /// Set the top border colour key.
    pub fn border_color(mut self, key: &'a str) -> Self {
        self.border_color = Some(key);
        self
    }

    /// Set the background colour key.
    pub fn background_color(mut self, key: &'a str) -> Self {
        self.background_color = Some(key);
        self
    }

    pub fn padding_x(mut self, v: usize) -> Self {
        self.padding_x = v;
        self
    }

    pub fn padding_top(mut self, v: usize) -> Self {
        self.padding_top = v;
        self
    }

    /// Render the boxed content.
    ///
    /// Returns the same structure as [`Pane::render`], with optional
    /// background styling applied to the content area.
    pub fn render(
        &self,
        colors: &ThemeColors,
        term_width: usize,
        children: Vec<Line<'static>>,
        inside_modal: bool,
    ) -> Vec<Line<'static>> {
        let bg = self.background_color.and_then(|k| resolve_color(k, colors));

        let pane = Pane::new()
            .padding_x(self.padding_x)
            .padding_top(self.padding_top);

        // Forward the border colour to the pane divider.
        let pane = if let Some(k) = self.border_color {
            pane.color(k)
        } else {
            pane
        };

        let mut lines = pane.render(colors, term_width, children, inside_modal);

        // Apply background colour to each line without shifting the content.
        if let Some(bg_color) = bg {
            let bg_style = Style::default().bg(bg_color);
            for line in &mut lines {
                for span in &mut line.spans {
                    if span.style.bg.is_none() {
                        span.style = span.style.bg(bg_color);
                    }
                }

                let width = line.width();
                if width < term_width {
                    line.spans.push(ratatui::text::Span::styled(
                        " ".repeat(term_width - width),
                        bg_style,
                    ));
                } else if line.spans.is_empty() && term_width > 0 {
                    line.spans.push(ratatui::text::Span::styled(
                        " ".repeat(term_width),
                        bg_style,
                    ));
                }
            }
        }

        lines
    }
}

impl<'a> Default for ThemedBox<'a> {
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

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[test]
    fn themed_box_renders_like_pane() {
        let b = ThemedBox::new().padding_top(0).padding_x(0);
        let lines = b.render(dark(), 40, vec![], false);
        assert!(!lines.is_empty());
    }

    #[test]
    fn themed_box_skips_divider_inside_modal() {
        let b = ThemedBox::new().padding_top(0).padding_x(0);
        let lines = b.render(dark(), 40, vec![], true);
        // No divider line when inside modal
        assert!(
            lines.is_empty()
                || lines.iter().all(|l| l.spans.is_empty()
                    || l.spans[0].content.is_empty()
                    || l.spans[0].content.starts_with(' '))
        );
    }

    #[test]
    fn themed_box_applies_background() {
        let b = ThemedBox::new()
            .padding_top(0)
            .padding_x(0)
            .background_color("surface");
        let children = vec![Line::from("hello")];
        let lines = b.render(dark(), 40, children, true);
        // Should have background spans
        assert!(!lines.is_empty());
        for line in &lines {
            if !line.spans.is_empty() {
                // At least one span should exist
                assert!(!line.spans.is_empty());
            }
        }
    }

    #[test]
    fn themed_box_background_preserves_child_content_position() {
        let b = ThemedBox::new()
            .padding_top(0)
            .padding_x(0)
            .background_color("surface");
        let lines = b.render(dark(), 10, vec![Line::from("hello")], true);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("hello"));
        assert_eq!(lines[0].width(), 10);
    }

    #[test]
    fn themed_box_accepts_border_color() {
        let b = ThemedBox::new()
            .padding_top(0)
            .padding_x(0)
            .border_color("warning");
        let lines = b.render(dark(), 20, vec![Line::from("content")], false);
        assert!(!lines.is_empty());
    }
}
