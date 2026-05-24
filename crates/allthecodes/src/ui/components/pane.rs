//! Generic pane container with theme-aware styling.
//!
//! Mirrors the upstream TypeScript `Pane.tsx` design-system component.
//!
//! A Pane renders an optional coloured top divider line (when not inside a
//! modal), configurable horizontal padding, and top margin.  Callers provide
//! the content rows; Pane surrounds them with the appropriate visual framing.

use ratatui::text::Line;

use crate::ui::divider::Divider;
use crate::ui::theme::ThemeColors;

/// A generic pane container.
///
/// # Example
///
/// ```ignore
/// let pane = Pane::new()
///     .color("border")
///     .padding_x(2)
///     .padding_top(1);
/// let output = pane.render(&theme, term_width, content_lines, false);
/// ```
pub struct Pane<'a> {
    /// Theme colour key for the top divider line.
    color: Option<&'a str>,
    /// Horizontal padding in columns (default: 2).
    padding_x: usize,
    /// Top margin rows (default: 1).
    padding_top: usize,
}

impl<'a> Pane<'a> {
    pub fn new() -> Self {
        Self {
            color: None,
            padding_x: 2,
            padding_top: 1,
        }
    }

    /// Set the divider colour key.
    pub fn color(mut self, key: &'a str) -> Self {
        self.color = Some(key);
        self
    }

    /// Set horizontal padding.
    pub fn padding_x(mut self, v: usize) -> Self {
        self.padding_x = v;
        self
    }

    /// Set top margin rows.
    pub fn padding_top(mut self, v: usize) -> Self {
        self.padding_top = v;
        self
    }

    /// Render the pane content.
    ///
    /// * `colors`  – active theme colours.
    /// * `term_width` – terminal width in columns.
    /// * `children` – content lines to render inside the pane.
    /// * `inside_modal` – when `true`, the top divider is skipped to avoid
    ///   double borders.
    ///
    /// Returns the full set of `Line`s including divider, padding, and
    /// content.
    pub fn render(
        &self,
        colors: &ThemeColors,
        term_width: usize,
        children: Vec<Line<'static>>,
        inside_modal: bool,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Top divider (skipped inside modals).
        if !inside_modal {
            let mut divider = Divider::new().width(term_width);
            if let Some(color_key) = self.color {
                divider = divider.color(color_key);
            }
            lines.extend(divider.render(colors, term_width));
        }

        // Top padding.
        for _ in 0..self.padding_top {
            lines.push(Line::from(""));
        }

        // Content with horizontal padding.
        let pad = " ".repeat(self.padding_x);
        for child in children {
            let mut padded = Line::from(pad.clone());
            padded.spans.extend(child.spans);
            lines.push(padded);
        }

        lines
    }
}

impl<'a> Default for Pane<'a> {
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
    fn pane_adds_top_divider_outside_modal() {
        let pane = Pane::new().padding_top(0).padding_x(0);
        let lines = pane.render(dark(), 40, vec![], false);
        // First line is the divider
        assert!(!lines.is_empty());
        assert_eq!(lines[0].spans.len(), 2); // padding + dash span
    }

    #[test]
    fn pane_skips_divider_inside_modal() {
        let pane = Pane::new().padding_top(0).padding_x(0);
        let lines = pane.render(dark(), 40, vec![], true);
        // No divider — empty or padding-only
        assert!(lines.is_empty() || lines[0].spans[0].content.is_empty());
    }

    #[test]
    fn pane_adds_top_padding() {
        let pane = Pane::new().padding_top(2).padding_x(0);
        let lines = pane.render(dark(), 40, vec![], true);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.is_empty() || lines[0].spans[0].content.is_empty());
    }

    #[test]
    fn pane_horizontal_padding() {
        let pane = Pane::new().padding_top(0).padding_x(3);
        let child = Line::from("hello");
        let lines = pane.render(dark(), 40, vec![child], true);
        // First content line should start with 3 spaces
        assert!(!lines.is_empty());
        let first_content = &lines[0];
        assert!(first_content.spans[0].content.starts_with("   "));
    }
}
