//! Shared render primitives for keyboard shortcut hints and byline-style
//! multi-hint displays.
//!
//! # Dead-code note
//! Several methods and fields on [`ShortcutHint`] and [`Byline`] are currently
//! unused in the running binary.  They exist for design-system completeness and
//! will be wired up during Design System Sprint 3 (integration phase).

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::ui::theme::color::resolve_color;
use crate::ui::theme::ThemeColors;

// ---------------------------------------------------------------------------
// ShortcutHint
// ---------------------------------------------------------------------------

/// A single keyboard shortcut + action description.
///
/// # Example
///
/// ```ignore
/// let hint = ShortcutHint::new("Ctrl+O", "open")
///     .bold(true)
///     .parens(true);
/// let span = hint.render(&theme_colors);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ShortcutHint<'a> {
    pub key: &'a str,
    pub action: &'a str,
    /// Whether to wrap the hint in parentheses: `(Ctrl+O open)`.
    parens: bool,
    /// Whether to render the key in bold.
    bold_key: bool,
    /// Optional theme colour key for the hint text (default: `inactive`).
    color: Option<&'a str>,
}

impl<'a> ShortcutHint<'a> {
    pub const fn new(key: &'a str, action: &'a str) -> Self {
        Self {
            key,
            action,
            parens: false,
            bold_key: false,
            color: None,
        }
    }

    /// Wrap the hint in parentheses.
    pub fn with_parens(mut self) -> Self {
        self.parens = true;
        self
    }

    /// Render the key portion in bold.
    pub fn with_bold_key(mut self) -> Self {
        self.bold_key = true;
        self
    }

    pub fn with_color(mut self, color: &'a str) -> Self {
        self.color = Some(color);
        self
    }

    /// Render this hint as a [`Span`] with the given theme.
    pub fn render(&self, colors: &ThemeColors) -> Span<'static> {
        let fg = self
            .color
            .and_then(|k| resolve_color(k, colors))
            .unwrap_or(colors.inactive);

        let text = if self.parens {
            format!("({} {})", self.key, self.action)
        } else {
            format!("{} {}", self.key, self.action)
        };

        let mut style = Style::default().fg(fg);
        if self.bold_key {
            style = style.add_modifier(Modifier::BOLD);
        }

        Span::styled(text, style)
    }
}

// ---------------------------------------------------------------------------
// render_shortcut_hints (legacy string API)
// ---------------------------------------------------------------------------

/// Render a slice of [`ShortcutHint`] into a pipe-separated string.
///
/// Legacy API kept for callers that need a `String` rather than styled spans.
/// Prefer [`render_hints_styled`] for new code.
pub fn render_shortcut_hints(hints: &[ShortcutHint<'_>]) -> String {
    hints
        .iter()
        .copied()
        .map(|h| {
            if h.parens {
                format!("({} {})", h.key, h.action)
            } else {
                format!("{} {}", h.key, h.action)
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

// ---------------------------------------------------------------------------
// render_hints_styled (theme-aware, Span-based)
// ---------------------------------------------------------------------------

/// Render a slice of [`ShortcutHint`] into a `Vec<Span>` joined by ` · `.
///
/// Each hint is rendered through its own `render()` method so bold/parens
/// settings are preserved.  Returns an empty vec when `hints` is empty.
pub fn render_hints_styled(hints: &[ShortcutHint<'_>], colors: &ThemeColors) -> Vec<Span<'static>> {
    if hints.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(hints.len() * 2 - 1);
    for (i, hint) in hints.iter().enumerate() {
        if i > 0 {
            result.push(Span::styled(" · ", Style::default().fg(colors.inactive)));
        }
        result.push(hint.render(colors));
    }
    result
}

/// Render a full byline (wrapper around [`render_hints_styled`]).
pub fn render_byline(hints: &[ShortcutHint<'_>], colors: &ThemeColors) -> Vec<Span<'static>> {
    render_hints_styled(hints, colors)
}

// ---------------------------------------------------------------------------
// Byline
// ---------------------------------------------------------------------------

/// A byline composed of keyboard hints separated by middle dots.
///
/// Mirrors the upstream TypeScript `Byline.tsx` design-system component.
pub struct Byline<'a> {
    hints: Vec<ShortcutHint<'a>>,
}

impl<'a> Byline<'a> {
    pub fn new() -> Self {
        Self { hints: Vec::new() }
    }

    /// Add a shortcut hint.
    pub fn hint(mut self, key: &'a str, action: &'a str) -> Self {
        self.hints.push(ShortcutHint::new(key, action));
        self
    }

    /// Add a pre-configured `ShortcutHint`.
    pub fn push(mut self, hint: ShortcutHint<'a>) -> Self {
        self.hints.push(hint);
        self
    }

    /// Number of hints in the byline.
    pub fn len(&self) -> usize {
        self.hints.len()
    }
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    /// Render the full byline as a `Vec<Span>`.
    ///
    /// Returns an empty vec when there are no hints.
    pub fn render(&self, colors: &ThemeColors) -> Vec<Span<'static>> {
        render_hints_styled(&self.hints, colors)
    }
}

impl<'a> Default for Byline<'a> {
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

    // -- ShortcutHint --

    #[test]
    fn shortcut_hint_renders_key_action() {
        let hint = ShortcutHint::new("Enter", "select");
        let span = hint.render(dark());
        assert_eq!(span.content, "Enter select");
    }

    #[test]
    fn shortcut_hint_with_parens() {
        let hint = ShortcutHint::new("Esc", "close").with_parens();
        let span = hint.render(dark());
        assert_eq!(span.content, "(Esc close)");
    }

    #[test]
    fn shortcut_hint_bold_key() {
        let hint = ShortcutHint::new("y", "yes").with_bold_key();
        let span = hint.render(dark());
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn shortcut_hint_color_overrides_default() {
        let hint = ShortcutHint::new("!", "warn").with_color("warning");
        let span = hint.render(dark());
        assert_eq!(span.style.fg, Some(dark().warning));
    }

    // -- render_shortcut_hints (legacy) --

    #[test]
    fn legacy_render_shortcut_hints_empty() {
        assert_eq!(render_shortcut_hints(&[]), "");
    }

    #[test]
    fn legacy_render_shortcut_hints_single() {
        assert_eq!(
            render_shortcut_hints(&[ShortcutHint::new("q", "quit")]),
            "q quit"
        );
    }

    #[test]
    fn legacy_render_shortcut_hints_multiple() {
        assert_eq!(
            render_shortcut_hints(&[
                ShortcutHint::new("Enter", "select"),
                ShortcutHint::new("Esc", "close"),
            ]),
            "Enter select | Esc close"
        );
    }

    // -- render_hints_styled --

    #[test]
    fn render_hints_styled_empty() {
        let spans = render_hints_styled(&[], dark());
        assert!(spans.is_empty());
    }

    #[test]
    fn render_hints_styled_single() {
        let hints = [ShortcutHint::new("q", "quit")];
        let spans = render_hints_styled(&hints, dark());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "q quit");
    }

    #[test]
    fn render_hints_styled_multiple() {
        let hints = [
            ShortcutHint::new("Enter", "select"),
            ShortcutHint::new("Esc", "close"),
        ];
        let spans = render_hints_styled(&hints, dark());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "Enter select");
        assert_eq!(spans[2].content, "Esc close");
    }

    // -- render_byline --

    #[test]
    fn render_byline_works() {
        let hints = [ShortcutHint::new("y", "yes"), ShortcutHint::new("n", "no")];
        let spans = render_byline(&hints, dark());
        assert_eq!(spans.len(), 3);
    }

    // -- Byline --

    #[test]
    fn byline_empty() {
        let b = Byline::new();
        assert!(b.render(dark()).is_empty());
    }

    #[test]
    fn byline_with_hints() {
        let b = Byline::new().hint("Enter", "send").hint("Esc", "cancel");
        let spans = b.render(dark());
        assert_eq!(spans.len(), 3);
        assert!(spans[1].content.contains('·'));
    }

    #[test]
    fn byline_push_and_len_track_hints() {
        let b = Byline::new().push(ShortcutHint::new("Tab", "next"));
        assert_eq!(b.len(), 1);
        assert!(!b.is_empty());
        assert_eq!(b.render(dark())[0].content, "Tab next");
    }
}
