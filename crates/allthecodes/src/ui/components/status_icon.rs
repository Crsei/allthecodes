//! Semantic status icon with Unicode glyphs and theme-aware colours.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::ui::theme::ThemeColors;

/// Semantic status with corresponding Unicode icon and colour key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIcon {
    Success,
    Error,
    Warning,
    Info,
    Pending,
    Loading,
}

impl StatusIcon {
    /// Unicode glyph for this status.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Success => "\u{2713}", // ✓
            Self::Error => "\u{2717}",   // ✗
            Self::Warning => "\u{26A0}", // ⚠
            Self::Info => "\u{2139}",    // ℹ
            Self::Pending => "\u{25CB}", // ○
            Self::Loading => "\u{2026}", // …
        }
    }

    /// Theme colour key, or `None` for dim fallback (pending/loading).
    pub fn color_key(self) -> Option<&'static str> {
        match self {
            Self::Success => Some("icon_success"),
            Self::Error => Some("icon_error"),
            Self::Warning => Some("icon_warning"),
            Self::Info => Some("icon_info"),
            Self::Pending => None,
            Self::Loading => None,
        }
    }

    /// Resolve the colour from the theme.
    fn resolved_color(self, colors: &ThemeColors) -> ratatui::style::Color {
        match self {
            Self::Success => colors.iconSuccess,
            Self::Error => colors.iconError,
            Self::Warning => colors.iconWarning,
            Self::Info => colors.iconInfo,
            Self::Pending | Self::Loading => colors.iconPending,
        }
    }

    /// Render as a `Span` with the theme's status colour.
    ///
    /// If `with_space` is true a trailing space is appended after the icon.
    pub fn render(self, colors: &ThemeColors, with_space: bool) -> Span<'static> {
        let text = if with_space {
            format!("{} ", self.icon())
        } else {
            self.icon().to_string()
        };
        Span::styled(text, Style::default().fg(self.resolved_color(colors)))
    }

    /// Legacy label for backward compatibility.
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::Error => "error",
            Self::Warning => "warn",
            Self::Info => "info",
            Self::Pending => "pending",
            Self::Loading => "loading",
        }
    }

    /// Map from legacy `StatusSeverity` (if used elsewhere).
    pub fn from_legacy(legacy: LegacyStatus) -> Self {
        match legacy {
            LegacyStatus::Ok => Self::Success,
            LegacyStatus::Warning => Self::Warning,
            LegacyStatus::Error => Self::Error,
            LegacyStatus::Attention => Self::Warning,
        }
    }
}

/// Legacy 4-variant status for migration compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyStatus {
    Ok,
    Warning,
    Error,
    Attention,
}

impl LegacyStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warn",
            Self::Error => "error",
            Self::Attention => "attention",
        }
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
    fn success_icon() {
        assert_eq!(StatusIcon::Success.icon(), "\u{2713}");
    }

    #[test]
    fn error_icon() {
        assert_eq!(StatusIcon::Error.icon(), "\u{2717}");
    }

    #[test]
    fn warning_icon() {
        assert_eq!(StatusIcon::Warning.icon(), "\u{26A0}");
    }

    #[test]
    fn success_color() {
        let span = StatusIcon::Success.render(dark(), false);
        assert_eq!(span.style.fg, Some(dark().iconSuccess));
    }

    #[test]
    fn error_color() {
        let span = StatusIcon::Error.render(dark(), false);
        assert_eq!(span.style.fg, Some(dark().iconError));
    }

    #[test]
    fn pending_color_is_dim() {
        let span = StatusIcon::Pending.render(dark(), false);
        assert_eq!(span.style.fg, Some(dark().iconPending));
    }

    #[test]
    fn render_with_space() {
        let span = StatusIcon::Warning.render(dark(), true);
        assert!(span.content.ends_with(' '));
    }

    #[test]
    fn legacy_label_matches() {
        assert_eq!(StatusIcon::Success.label(), "ok");
        assert_eq!(StatusIcon::Error.label(), "error");
        assert_eq!(StatusIcon::Info.label(), "info");
        assert_eq!(StatusIcon::Loading.label(), "loading");
    }

    #[test]
    fn legacy_mapping() {
        let l = LegacyStatus::Ok;
        assert_eq!(StatusIcon::from_legacy(l), StatusIcon::Success);
        assert_eq!(
            StatusIcon::from_legacy(LegacyStatus::Warning),
            StatusIcon::Warning
        );
        assert_eq!(
            StatusIcon::from_legacy(LegacyStatus::Error),
            StatusIcon::Error
        );
        assert_eq!(LegacyStatus::Warning.label(), "warn");
        assert_eq!(LegacyStatus::Error.label(), "error");
    }

    #[test]
    fn status_icons_expose_color_keys() {
        assert_eq!(StatusIcon::Info.color_key(), Some("icon_info"));
        assert_eq!(StatusIcon::Loading.color_key(), None);
    }
}
