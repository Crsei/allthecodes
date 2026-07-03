//! Theme-aware text wrapper that resolves colour keys to renderable styles.
//!
//! Mirrors the upstream TypeScript `ThemedText.tsx` design-system component.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::ui::theme::color::resolve_color;
use crate::ui::theme::ThemeColors;

/// A theme-aware text span.
///
/// Resolves theme colour keys (e.g. `"success"`, `"error"`) or raw colour
/// strings (`#hex`, `rgb()`, etc.) into ratatui `Style`, then produces a
/// [`Span`] suitable for composition into a [`Line`](ratatui::text::Line).
///
/// # Example
///
/// ```ignore
/// let span = ThemedText::new("Hello")
///     .color("accent")
///     .bold()
///     .render(&theme);
/// ```
#[derive(Debug, Clone)]
pub struct ThemedText<'a> {
    content: &'a str,
    color: Option<&'a str>,
    background_color: Option<&'a str>,
    dim_color: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

impl<'a> ThemedText<'a> {
    /// Create a new `ThemedText` with the given string content.
    pub fn new(content: &'a str) -> Self {
        Self {
            content,
            color: None,
            background_color: None,
            dim_color: false,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    /// Set the foreground colour key or raw colour string.
    pub fn color(mut self, key: &'a str) -> Self {
        self.color = Some(key);
        self
    }

    /// Set the background colour key or raw colour string.
    pub fn background_color(mut self, key: &'a str) -> Self {
        self.background_color = Some(key);
        self
    }

    /// Use `inactive` colour for dimmed appearance (compatible with bold).
    pub fn dim(mut self) -> Self {
        self.dim_color = true;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Resolve the foreground colour from the theme.
    fn resolved_color(&self, colors: &ThemeColors) -> Option<Color> {
        self.color.and_then(|k| resolve_color(k, colors))
    }

    /// Resolve the background colour from the theme.
    fn resolved_bg(&self, colors: &ThemeColors) -> Option<Color> {
        self.background_color.and_then(|k| resolve_color(k, colors))
    }

    /// Build the ratatui `Style` and produce a `Span`.
    pub fn render(&self, colors: &ThemeColors) -> Span<'static> {
        let fg = if self.dim_color {
            Some(colors.inactive)
        } else {
            self.resolved_color(colors)
        };

        let mut style = Style::default();
        if let Some(c) = fg {
            style = style.fg(c);
        }
        if let Some(c) = self.resolved_bg(colors) {
            style = style.bg(c);
        }

        let mut modifiers = Modifier::empty();
        if self.bold {
            modifiers |= Modifier::BOLD;
        }
        if self.italic {
            modifiers |= Modifier::ITALIC;
        }
        if self.underline {
            modifiers |= Modifier::UNDERLINED;
        }
        if self.strikethrough {
            modifiers |= Modifier::CROSSED_OUT;
        }
        style = style.add_modifier(modifiers);

        Span::styled(self.content.to_string(), style)
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
    fn plain_text_no_style() {
        let span = ThemedText::new("hello").render(dark());
        assert_eq!(span.content, "hello");
        // default style has no fg/bg
    }

    #[test]
    fn themed_text_resolves_theme_key() {
        let span = ThemedText::new("ok").color("success").render(dark());
        assert_eq!(span.style.fg, Some(dark().success));
    }

    #[test]
    fn themed_text_bold_sets_modifier() {
        let span = ThemedText::new("bold").bold().render(dark());
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn themed_text_dim_uses_inactive() {
        let span = ThemedText::new("dim").dim().render(dark());
        assert_eq!(span.style.fg, Some(dark().inactive));
    }

    #[test]
    fn themed_text_italic_underline() {
        let span = ThemedText::new("stylish")
            .italic()
            .underline()
            .render(dark());
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn themed_text_with_background() {
        let span = ThemedText::new("bg")
            .background_color("surface")
            .render(dark());
        assert_eq!(span.style.bg, Some(dark().surface));
    }

    #[test]
    fn themed_text_color_overrides_dim() {
        let span = ThemedText::new("test")
            .color("success")
            .dim()
            .render(dark());
        // dim takes priority
        assert_eq!(span.style.fg, Some(dark().inactive));
    }

    #[test]
    fn themed_text_strikethrough_sets_modifier() {
        let span = ThemedText::new("gone").strikethrough().render(dark());
        assert!(span.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }
}
