//! Reusable list-item component with focus, selection, scroll indicators, and
//! disabled-state dimming.
//!
//! Mirrors the upstream TypeScript `ListItem.tsx` design-system component.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::ThemeColors;

/// A single item in a selectable list.
///
/// # Example
///
/// ```ignore
/// let item = ListItem::new("My Item")
///     .focused(true)
///     .description("A description");
/// let line = item.render(&theme, 80);
/// ```
pub struct ListItem<'a> {
    /// Primary label text.
    label: &'a str,
    /// Whether this item is focused (shows `❯` pointer).
    is_focused: bool,
    /// Whether this item is selected (shows `✓` tick).
    is_selected: bool,
    /// Whether to show a scroll-up indicator.
    show_scroll_up: bool,
    /// Whether to show a scroll-down indicator.
    show_scroll_down: bool,
    /// Whether auto styling should be applied (default: true).
    styled: bool,
    /// Whether the item is disabled (dimmed, no indicator).
    disabled: bool,
    /// Optional description shown below the label.
    description: Option<&'a str>,
    /// Optional extra style key to apply to the label.
    color: Option<&'a str>,
}

impl<'a> ListItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            is_focused: false,
            is_selected: false,
            show_scroll_up: false,
            show_scroll_down: false,
            styled: true,
            disabled: false,
            description: None,
            color: None,
        }
    }

    pub fn is_focused(mut self, v: bool) -> Self {
        self.is_focused = v;
        self
    }

    pub fn is_selected(mut self, v: bool) -> Self {
        self.is_selected = v;
        self
    }

    pub fn show_scroll_up(mut self) -> Self {
        self.show_scroll_up = true;
        self
    }

    pub fn show_scroll_down(mut self) -> Self {
        self.show_scroll_down = true;
        self
    }

    pub fn styled(mut self, v: bool) -> Self {
        self.styled = v;
        self
    }

    pub fn disabled(mut self, v: bool) -> Self {
        self.disabled = v;
        self
    }

    pub fn description(mut self, v: &'a str) -> Self {
        self.description = Some(v);
        self
    }
    pub fn color(mut self, v: &'a str) -> Self {
        self.color = Some(v);
        self
    }

    /// Render this list item into a single [`Line`].
    ///
    /// When `disabled`, no indicator is shown and text is dimmed.
    pub fn render(&self, colors: &ThemeColors) -> Line<'static> {
        if self.disabled {
            return Line::from(Span::styled(
                format!("  {}", self.label),
                Style::default().fg(colors.inactive),
            ));
        }

        let mut spans: Vec<Span<'static>> = Vec::new();

        // Scroll indicators.
        if self.show_scroll_up {
            spans.push(Span::styled(
                "\u{2191} ",
                Style::default().fg(colors.inactive),
            ));
        } else if self.show_scroll_down {
            spans.push(Span::styled(
                "\u{2193} ",
                Style::default().fg(colors.inactive),
            ));
        }

        // Focus / selection indicator.
        if self.is_focused {
            spans.push(Span::styled(
                "\u{276F} ", // ❯
                if self.styled {
                    Style::default().fg(colors.suggestion)
                } else {
                    Style::default()
                },
            ));
        } else if self.is_selected {
            spans.push(Span::styled(
                "\u{2713} ", // ✓
                if self.styled {
                    Style::default().fg(colors.success)
                } else {
                    Style::default()
                },
            ));
        } else {
            spans.push(Span::raw("  "));
        }

        // Label.
        let label_fg = if self.styled {
            if let Some(color) = self
                .color
                .and_then(|key| crate::ui::theme::color::resolve_color(key, colors))
            {
                color
            } else if self.is_focused {
                colors.suggestion
            } else if self.is_selected {
                colors.success
            } else {
                colors.surfaceText
            }
        } else {
            colors.surfaceText
        };
        let mut label_style = Style::default().fg(label_fg);
        if self.is_focused {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(self.label.to_string(), label_style));

        Line::from(spans)
    }

    /// Render the full item including an optional description line.
    ///
    /// Returns zero, one, or two lines.
    pub fn render_full(&self, colors: &ThemeColors) -> Vec<Line<'static>> {
        let mut lines = vec![self.render(colors)];

        if let Some(desc) = self.description {
            lines.push(Line::from(Span::styled(
                format!("    {}", desc),
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

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[test]
    fn plain_item_no_indicator() {
        let item = ListItem::new("hello");
        let line = item.render(dark());
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "  hello");
    }

    #[test]
    fn focused_item_shows_pointer() {
        let item = ListItem::new("hello").is_focused(true);
        let line = item.render(dark());
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('\u{276F}'));
    }

    #[test]
    fn selected_item_shows_check() {
        let item = ListItem::new("hello").is_selected(true);
        let line = item.render(dark());
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('\u{2713}'));
    }

    #[test]
    fn disabled_item_is_dimmed() {
        let item = ListItem::new("hello").disabled(true);
        let line = item.render(dark());
        assert_eq!(line.spans[0].style.fg, Some(dark().inactive));
    }

    #[test]
    fn item_with_description() {
        let item = ListItem::new("main").description("a desc");
        let lines = item.render_full(dark());
        assert_eq!(lines.len(), 2);
        let desc_text: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(desc_text.contains("a desc"));
    }

    #[test]
    fn focused_item_bold_label() {
        let item = ListItem::new("bold").is_focused(true);
        let line = item.render(dark());
        // The label span should have BOLD modifier.
        let label_span = &line.spans[line.spans.len() - 1];
        assert!(label_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn color_overrides_default_label_color() {
        let item = ListItem::new("warn").color("warning");
        let line = item.render(dark());
        let label_span = &line.spans[line.spans.len() - 1];
        assert_eq!(label_span.style.fg, Some(dark().warning));
    }
}
