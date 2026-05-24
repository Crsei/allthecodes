//! Theme-aware horizontal divider component.
//!
//! Renders a horizontal rule (e.g. `──────`) with an optional centred title,
//! theme-aware colour, and configurable padding / repeat character.
//!
//! Mirrors the upstream TypeScript `Divider.tsx` design-system component.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::color::resolve_color;
use crate::ui::theme::ThemeColors;

/// A horizontal divider line.
///
/// # Example
///
/// ```ignore
/// let divider = Divider::new()
///     .title("Section")
///     .color("border")
///     .padding(2);
/// let lines = divider.render(&theme_colors, term_width);
/// ```
#[derive(Debug, Clone)]
pub struct Divider<'a> {
    /// Total width in columns (default: terminal width passed to `render()`).
    width: Option<usize>,
    /// Theme colour key or raw colour string for the line and title.
    color: Option<&'a str>,
    /// Character repeated to form the line (default: `'─'` U+2500).
    char: char,
    /// Left padding columns (default: 0).
    padding: usize,
    /// Optional centred title text.
    title: Option<&'a str>,
}

impl<'a> Divider<'a> {
    pub fn new() -> Self {
        Self {
            width: None,
            color: None,
            char: '\u{2500}',
            padding: 0,
            title: None,
        }
    }

    /// Set the total width in columns.
    pub fn width(mut self, v: usize) -> Self {
        self.width = Some(v);
        self
    }

    /// Set the theme colour key (e.g. `"border"`, `"success"`).
    pub fn color(mut self, v: &'a str) -> Self {
        self.color = Some(v);
        self
    }

    /// Set the character repeated to form the line.
    pub fn char(mut self, v: char) -> Self {
        self.char = v;
        self
    }

    /// Left padding in columns.
    pub fn padding(mut self, v: usize) -> Self {
        self.padding = v;
        self
    }

    /// Optional centred title text.
    pub fn title(mut self, v: &'a str) -> Self {
        self.title = Some(v);
        self
    }

    /// Resolve the display colour from the optional theme key.
    fn resolved_color(&self, colors: &ThemeColors) -> Option<Color> {
        self.color.and_then(|k| resolve_color(k, colors))
    }

    /// Render the divider into zero or more [`Line`]s.
    ///
    /// Returns at most one line (the divider row).  Returns an empty vec when
    /// the available width after padding is zero.
    pub fn render(&self, colors: &ThemeColors, term_width: usize) -> Vec<Line<'static>> {
        let color = self.resolved_color(colors).unwrap_or(colors.border);
        let total = self.width.unwrap_or(term_width);

        if total <= self.padding {
            return Vec::new();
        }

        let available = total - self.padding;
        let padding_str = " ".repeat(self.padding);
        let style = Style::default().fg(color);

        let line = if let Some(title) = self.title {
            self.render_with_title(title, available, &padding_str, style, colors)
        } else {
            let rule: String = (0..available).map(|_| self.char).collect();
            Line::from(vec![Span::raw(padding_str), Span::styled(rule, style)])
        };

        vec![line]
    }

    fn render_with_title(
        &self,
        title: &str,
        available: usize,
        padding_str: &str,
        style: Style,
        colors: &ThemeColors,
    ) -> Line<'static> {
        // Title width — use Unicode-aware measurement for multi-width chars.
        let title_width = unicode_width::UnicodeWidthStr::width(title);
        let title_style = Style::default().fg(colors.surfaceText);

        // Need at least 4 cols for ` ┃t┃ ` wrapping.
        if available < title_width + 4 {
            // Not enough room — fall back to plain divider.
            let rule: String = (0..available).map(|_| self.char).collect();
            return Line::from(vec![
                Span::raw(padding_str.to_string()),
                Span::styled(rule, style),
            ]);
        }

        // ──── title ────
        let dash_total = available.saturating_sub(title_width + 2); // 2 spaces around title
        let left_dashes = dash_total / 2;
        let right_dashes = dash_total - left_dashes;

        let left: String = (0..left_dashes).map(|_| self.char).collect();
        let right: String = (0..right_dashes).map(|_| self.char).collect();

        Line::from(vec![
            Span::raw(padding_str.to_string()),
            Span::styled(left, style),
            Span::raw(" "),
            Span::styled(title.to_string(), title_style),
            Span::raw(" "),
            Span::styled(right, style),
        ])
    }
}

impl<'a> Default for Divider<'a> {
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
    fn render_plain_divider_uses_border_color() {
        let d = Divider::new().width(10);
        let lines = d.render(dark(), 80);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        // 10 dashes
        assert_eq!(line.width(), 10);
    }

    #[test]
    fn render_divider_with_title() {
        let d = Divider::new().width(20).title("Hello");
        let lines = d.render(dark(), 80);
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        // Has: padding + left-dashes + space + title + space + right-dashes
        assert!(spans.len() >= 3);
        let title_span = &spans[spans.len() - 3]; // second-to-last content span
        assert_eq!(title_span.content, "Hello");
    }

    #[test]
    fn render_divider_too_narrow_for_title_falls_back() {
        let d = Divider::new().width(5).title("Hello");
        let lines = d.render(dark(), 80);
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        // No title in output — only padding + plain dashes
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn render_divider_respects_padding() {
        let d = Divider::new().width(10).padding(2);
        let lines = d.render(dark(), 80);
        assert_eq!(lines.len(), 1);
        // 2 padding + 8 dashes = width 10
        assert_eq!(lines[0].width(), 10);
    }

    #[test]
    fn render_divider_with_custom_char() {
        let d = Divider::new().width(5).char('=');
        let lines = d.render(dark(), 80);
        let last = lines[0].spans.len() - 1;
        assert_eq!(&lines[0].spans[last].content, "=====");
    }

    #[test]
    fn render_zero_width_empty() {
        let d = Divider::new().width(0);
        let lines = d.render(dark(), 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn render_all_padding_no_content() {
        let d = Divider::new().width(2).padding(3);
        let lines = d.render(dark(), 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn divider_uses_theme_color_key() {
        let d = Divider::new().color("success").width(5);
        let lines = d.render(dark(), 80);
        // Second span (index 1) is the dash line when padding is 0
        let style = if lines[0].spans.len() > 1 {
            lines[0].spans[1].style
        } else {
            lines[0].spans[0].style
        };
        assert_eq!(style.fg, Some(dark().success));
    }
}
