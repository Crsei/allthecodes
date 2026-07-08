//! Color resolution utilities for the design-system theme.
//!
//! Provides `resolve_color` for parsing colour specifications (theme keys,
//! `#hex`, `rgb()`, `ansi256()`, `ansi:`), and a `ColorExt` trait for
//! common operations like dimming.

use ratatui::style::Color;

use super::ThemeColors;

// ---------------------------------------------------------------------------
// resolve_color
// ---------------------------------------------------------------------------

/// Resolve a colour string to a ratatui `Color`.
///
/// Supported formats:
/// - `"success"`, `"error"`, … — theme colour key (looked up in `colors`)
/// - `"#rrggbb"` — hex RGB
/// - `"rgb(r,g,b)"` — decimal RGB (each 0-255)
/// - `"ansi256(n)"` — ANSI 256-colour index
/// - `"ansi:name"` — ANSI named colour (`black`, `red`, `green`, `yellow`,
///   `blue`, `magenta`, `cyan`, `white`, `bright_black`, …)
///
/// Returns `None` if the string cannot be parsed (caller should fall back to
/// a sensible default or skip the colour).
pub fn resolve_color(s: &str, colors: &ThemeColors) -> Option<Color> {
    if s.is_empty() {
        return None;
    }

    // Theme key lookup (case-insensitive).
    if let Some(c) = resolve_theme_key(s, colors) {
        return Some(c);
    }

    // #hex
    if s.starts_with('#') {
        return parse_hex(s);
    }

    // rgb(r,g,b)
    if s.starts_with("rgb(") {
        return parse_rgb(s);
    }

    // ansi256(n)
    if s.starts_with("ansi256(") {
        return parse_ansi256(s);
    }

    // ansi:name
    if let Some(name) = s.strip_prefix("ansi:") {
        return parse_ansi_name(name);
    }

    None
}

// ---------------------------------------------------------------------------
// Theme-key lookup
// ---------------------------------------------------------------------------

/// Map a colour key string to the corresponding `ThemeColors` field.
fn resolve_theme_key(key: &str, colors: &ThemeColors) -> Option<Color> {
    // Normalise: lowercase, strip hyphens/underscores so `border-focus` and
    // `borderFocus` both work.
    match key.to_lowercase().replace(['-', '_'], "").as_str() {
        // Core
        "accent" => Some(colors.accent),
        "accentdim" => Some(colors.accentDim),
        "accenttext" => Some(colors.accentText),
        "inverted" => Some(colors.inverted),
        "invertedtext" => Some(colors.invertedText),

        // Semantic
        "success" => Some(colors.success),
        "error" => Some(colors.error),
        "warning" => Some(colors.warning),
        "suggestion" => Some(colors.suggestion),
        "info" => Some(colors.info),
        "contextinfoborder" => Some(colors.info),
        "contextinfolabel" => Some(colors.info),
        "contextinfotext" => Some(colors.surfaceText),
        "contextwarning" => Some(colors.warning),
        "contexterror" => Some(colors.error),

        // Surface
        "surface" => Some(colors.surface),
        "surfacetext" => Some(colors.surfaceText),
        "muted" => Some(colors.muted),
        "mutedtext" => Some(colors.mutedText),
        "inactive" => Some(colors.inactive),
        "inactivetext" => Some(colors.inactiveText),
        "border" => Some(colors.border),
        "borderfocus" => Some(colors.borderFocus),
        "bordererror" => Some(colors.borderError),
        "permission" => Some(colors.permission),
        "permissiontext" => Some(colors.permissionText),

        // Typography
        "dim" => Some(colors.dim),
        "bold" => Some(colors.bold),
        "link" => Some(colors.link),
        "code" => Some(colors.code),
        "codebg" => Some(colors.codeBg),
        "heading" => Some(colors.heading),
        "blockquote" => Some(colors.blockquote),
        "blockquoteborder" => Some(colors.blockquoteBorder),
        "hr" => Some(colors.hr),

        // Selection
        "selection" => Some(colors.selection),
        "selectiontext" => Some(colors.selectionText),
        "cursor" => Some(colors.cursor),
        "cursortext" => Some(colors.cursorText),
        "searchhighlight" => Some(colors.searchHighlight),
        "searchhighlighttext" => Some(colors.searchHighlightText),

        // Diff
        "diffadd" => Some(colors.diffAdd),
        "diffaddbg" => Some(colors.diffAddBg),
        "diffremove" => Some(colors.diffRemove),
        "diffremovebg" => Some(colors.diffRemoveBg),
        "diffheader" => Some(colors.diffHeader),
        "diffcontext" => Some(colors.diffContext),

        // Syntax
        "syntaxkeyword" => Some(colors.syntaxKeyword),
        "syntaxstring" => Some(colors.syntaxString),
        "syntaxnumber" => Some(colors.syntaxNumber),
        "syntaxtype" => Some(colors.syntaxType),
        "syntaxfunction" => Some(colors.syntaxFunction),
        "syntaxcomment" => Some(colors.syntaxComment),
        "syntaxoperator" => Some(colors.syntaxOperator),
        "syntaxpunctuation" => Some(colors.syntaxPunctuation),
        "syntaxbuiltin" => Some(colors.syntaxBuiltin),
        "syntaxconstant" => Some(colors.syntaxConstant),
        "syntaxvariable" => Some(colors.syntaxVariable),
        "syntaxparameter" => Some(colors.syntaxParameter),
        "syntaxlabel" => Some(colors.syntaxLabel),

        // Icons
        "iconsuccess" => Some(colors.iconSuccess),
        "iconerror" => Some(colors.iconError),
        "iconwarning" => Some(colors.iconWarning),
        "iconinfo" => Some(colors.iconInfo),
        "iconpending" => Some(colors.iconPending),
        "iconloading" => Some(colors.iconLoading),

        // Agents
        "agentred" => Some(colors.agentRed),
        "agentorange" => Some(colors.agentOrange),
        "agentyellow" => Some(colors.agentYellow),
        "agentgreen" => Some(colors.agentGreen),
        "agentcyan" => Some(colors.agentCyan),
        "agentblue" => Some(colors.agentBlue),
        "agentpurple" => Some(colors.agentPurple),
        "agentpink" => Some(colors.agentPink),
        "agentplanner" => Some(colors.agentPlanner),
        "agentexecutor" => Some(colors.agentExecutor),
        "agentreviewer" => Some(colors.agentReviewer),
        "agenttester" => Some(colors.agentTester),
        "agentresearcher" => Some(colors.agentResearcher),
        "agentbackground" => Some(colors.agentBackground),

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

fn parse_hex(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    // Accept 6-char hex only.
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn parse_rgb(s: &str) -> Option<Color> {
    let inner = s.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some(Color::Rgb(r, g, b))
}

fn parse_ansi256(s: &str) -> Option<Color> {
    let inner = s.strip_prefix("ansi256(")?.strip_suffix(')')?;
    let idx = inner.trim().parse::<u8>().ok()?;
    Some(Color::Indexed(idx))
}

fn parse_ansi_name(name: &str) -> Option<Color> {
    let c = match name.to_lowercase().replace('-', "_").as_str() {
        "black" => Color::Indexed(0),
        "red" => Color::Indexed(1),
        "green" => Color::Indexed(2),
        "yellow" => Color::Indexed(3),
        "blue" => Color::Indexed(4),
        "magenta" => Color::Indexed(5),
        "cyan" => Color::Indexed(6),
        "white" => Color::Indexed(7),
        "bright_black" | "gray" | "grey" => Color::Indexed(8),
        "bright_red" => Color::Indexed(9),
        "bright_green" => Color::Indexed(10),
        "bright_yellow" => Color::Indexed(11),
        "bright_blue" => Color::Indexed(12),
        "bright_magenta" => Color::Indexed(13),
        "bright_cyan" => Color::Indexed(14),
        "bright_white" => Color::Indexed(15),
        _ => return None,
    };
    Some(c)
}

// ---------------------------------------------------------------------------
// ColorExt
// ---------------------------------------------------------------------------

/// Extension trait for `ratatui::style::Color` that adds theme-aware utilities.
#[cfg(test)]
pub trait ColorExt {
    /// Dim a colour using the theme's `inactive` colour as a dim target.
    ///
    /// Unlike raw ANSI dim (which only works on foreground and is terminal-
    /// dependent), this returns a colour that visually approximates dimming
    /// while remaining compatible with bold and other modifiers.
    fn dimmed(self, colors: &ThemeColors) -> Color;
}

#[cfg(test)]
impl ColorExt for Color {
    fn dimmed(self, colors: &ThemeColors) -> Color {
        // For simplicity, return the theme's inactive colour as the "dimmed"
        // version.  A more sophisticated implementation could blend the
        // original colour toward `inactive` by an alpha factor.
        colors.inactive
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};

    fn dark_colors() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    // -- resolve_theme_key --

    #[test]
    fn resolve_named_theme_key() {
        let c = dark_colors();
        assert_eq!(resolve_color("accent", c), Some(c.accent));
        assert_eq!(resolve_color("success", c), Some(c.success));
        assert_eq!(resolve_color("error", c), Some(c.error));
    }

    #[test]
    fn resolve_key_case_insensitive() {
        let c = dark_colors();
        assert_eq!(resolve_color("ACCENT", c), Some(c.accent));
        assert_eq!(resolve_color("BorderFocus", c), Some(c.borderFocus));
    }

    #[test]
    fn resolve_key_with_hyphen() {
        let c = dark_colors();
        assert_eq!(resolve_color("border-focus", c), Some(c.borderFocus));
    }

    #[test]
    fn resolve_context_info_keys() {
        let c = dark_colors();
        assert_eq!(resolve_color("context.info.label", c), None);
        assert_eq!(resolve_color("context-info-label", c), Some(c.info));
        assert_eq!(resolve_color("context_info_text", c), Some(c.surfaceText));
    }

    #[test]
    fn resolve_agent_identity_role_keys() {
        let c = dark_colors();
        assert_eq!(resolve_color("agent-planner", c), Some(c.agentPlanner));
        assert_eq!(resolve_color("agent_executor", c), Some(c.agentExecutor));
    }

    #[test]
    fn resolve_unknown_key_returns_none() {
        let c = dark_colors();
        assert_eq!(resolve_color("nonexistent", c), None);
    }

    #[test]
    fn resolve_empty_string_returns_none() {
        let c = dark_colors();
        assert_eq!(resolve_color("", c), None);
    }

    // -- hex parsing --

    #[test]
    fn resolve_hex_color() {
        let c = dark_colors();
        assert_eq!(resolve_color("#FF0000", c), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(resolve_color("#00ff00", c), Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn resolve_hex_too_short_returns_none() {
        let c = dark_colors();
        assert_eq!(resolve_color("#FFF", c), None);
        assert_eq!(resolve_color("#ff", c), None);
    }

    // -- rgb() parsing --

    #[test]
    fn resolve_rgb_function() {
        let c = dark_colors();
        assert_eq!(
            resolve_color("rgb(100,200,30)", c),
            Some(Color::Rgb(100, 200, 30))
        );
    }

    #[test]
    fn resolve_rgb_with_spaces() {
        let c = dark_colors();
        assert_eq!(
            resolve_color("rgb( 50 , 100 , 150 )", c),
            Some(Color::Rgb(50, 100, 150))
        );
    }

    // -- ansi parsing --

    #[test]
    fn resolve_ansi256() {
        let c = dark_colors();
        assert_eq!(resolve_color("ansi256(196)", c), Some(Color::Indexed(196)));
    }

    #[test]
    fn resolve_ansi_named() {
        let c = dark_colors();
        assert_eq!(resolve_color("ansi:red", c), Some(Color::Indexed(1)));
        assert_eq!(
            resolve_color("ansi:bright_green", c),
            Some(Color::Indexed(10))
        );
    }

    // -- dimmed --

    #[test]
    fn dimmed_returns_inactive() {
        let c = dark_colors();
        let white = Color::Rgb(255, 255, 255);
        assert_eq!(white.dimmed(c), c.inactive);
    }
}
