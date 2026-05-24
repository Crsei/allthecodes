//! Design-system theme provider: runtime theme switching, color tables, and
//! resolution helpers.
//!
//! This module implements the Rust-side equivalent of the TypeScript
//! `ThemeProvider` + `theme-types.ts` system.  Six built-in themes are
//! available (dark / light / daltonized / ANSI variants), selected at runtime
//! via `ThemeName` and persisted to user config.

use ratatui::style::{Color, Modifier, Style};
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

pub mod color;

// ---------------------------------------------------------------------------
// Theme name
// ---------------------------------------------------------------------------

/// Built-in theme variants, matching the upstream TypeScript `ThemeName` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeName {
    Dark,
    Light,
    LightDaltonized,
    DarkDaltonized,
    LightAnsi,
    DarkAnsi,
}

impl ThemeName {
    /// All known variants in display order.
    #[cfg(test)]
    pub const ALL: &[Self] = &[
        Self::Dark,
        Self::Light,
        Self::LightDaltonized,
        Self::DarkDaltonized,
        Self::LightAnsi,
        Self::DarkAnsi,
    ];

    #[cfg(test)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::LightDaltonized => "Light (Daltonized)",
            Self::DarkDaltonized => "Dark (Daltonized)",
            Self::LightAnsi => "Light (ANSI)",
            Self::DarkAnsi => "Dark (ANSI)",
        }
    }

    #[cfg(test)]
    pub fn is_dark(&self) -> bool {
        matches!(self, Self::Dark | Self::DarkDaltonized | Self::DarkAnsi)
    }

    #[cfg(test)]
    pub fn as_settings_value(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::LightDaltonized => "light-daltonized",
            Self::DarkDaltonized => "dark-daltonized",
            Self::LightAnsi => "light-ansi",
            Self::DarkAnsi => "dark-ansi",
        }
    }
}

impl FromStr for ThemeName {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value
            .trim()
            .to_ascii_lowercase()
            .replace(['_', ' '], "-")
            .as_str()
        {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            "light-daltonized" | "lightdaltonized" => Ok(Self::LightDaltonized),
            "dark-daltonized" | "darkdaltonized" => Ok(Self::DarkDaltonized),
            "light-ansi" | "lightansi" => Ok(Self::LightAnsi),
            "dark-ansi" | "darkansi" => Ok(Self::DarkAnsi),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSetting {
    Auto,
    Named(ThemeName),
}

impl ThemeSetting {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            Some(value) if value.eq_ignore_ascii_case("auto") => Self::Auto,
            Some(value) => ThemeName::from_str(value)
                .map(Self::Named)
                .unwrap_or(Self::Named(ThemeName::Dark)),
            None => Self::Named(ThemeName::Dark),
        }
    }

    pub fn resolved_name(&self) -> ThemeName {
        match self {
            Self::Auto => {
                if terminal_prefers_dark() {
                    ThemeName::Dark
                } else {
                    ThemeName::Light
                }
            }
            Self::Named(name) => *name,
        }
    }

    #[cfg(test)]
    pub fn as_settings_value(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Named(name) => name.as_settings_value(),
        }
    }
}

// ---------------------------------------------------------------------------
// Theme colour table
// ---------------------------------------------------------------------------

/// A complete colour table for one theme variant.
///
/// Each field corresponds to a key in the TypeScript `Theme` interface
/// (`theme-types.ts`).  Fields use the canonical upstream name so that the
/// mapping is obvious.
#[derive(Debug, Clone)]
#[allow(non_snake_case)]
pub struct ThemeColors {
    // -- Core palette --
    pub accent: Color,
    pub accentDim: Color,
    pub accentText: Color,
    pub inverted: Color,
    pub invertedText: Color,

    // -- Semantic status --
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    pub suggestion: Color,
    pub info: Color,

    // -- Surface / chrome --
    pub surface: Color,
    pub surfaceText: Color,
    pub muted: Color,
    pub mutedText: Color,
    pub inactive: Color,
    pub inactiveText: Color,
    pub border: Color,
    pub borderFocus: Color,
    pub borderError: Color,
    pub permission: Color,
    pub permissionText: Color,

    // -- Typography helpers --
    pub dim: Color,
    pub bold: Color,
    pub link: Color,
    pub code: Color,
    pub codeBg: Color,
    pub heading: Color,
    pub blockquote: Color,
    pub blockquoteBorder: Color,
    pub hr: Color,

    // -- Input / selection --
    pub selection: Color,
    pub selectionText: Color,
    pub cursor: Color,
    pub cursorText: Color,
    pub searchHighlight: Color,
    pub searchHighlightText: Color,

    // -- Diff --
    pub diffAdd: Color,
    pub diffAddBg: Color,
    pub diffRemove: Color,
    pub diffRemoveBg: Color,
    pub diffHeader: Color,
    pub diffContext: Color,

    // -- Syntax highlighting (code blocks) --
    pub syntaxKeyword: Color,
    pub syntaxString: Color,
    pub syntaxNumber: Color,
    pub syntaxType: Color,
    pub syntaxFunction: Color,
    pub syntaxComment: Color,
    pub syntaxOperator: Color,
    pub syntaxPunctuation: Color,
    pub syntaxBuiltin: Color,
    pub syntaxConstant: Color,
    pub syntaxVariable: Color,
    pub syntaxParameter: Color,
    pub syntaxLabel: Color,

    // -- Status icons --
    pub iconSuccess: Color,
    pub iconError: Color,
    pub iconWarning: Color,
    pub iconInfo: Color,
    pub iconPending: Color,
    pub iconLoading: Color,

    // -- Agent colours (per-agent dot / label) --
    pub agentRed: Color,
    pub agentOrange: Color,
    pub agentYellow: Color,
    pub agentGreen: Color,
    pub agentCyan: Color,
    pub agentBlue: Color,
    pub agentPurple: Color,
    pub agentPink: Color,
}

// ---------------------------------------------------------------------------
// Built-in theme definitions
// ---------------------------------------------------------------------------

/// Dark theme — default, matching upstream `getTheme("dark")`.
fn dark_theme() -> ThemeColors {
    ThemeColors {
        accent: Color::Rgb(190, 140, 255),
        accentDim: Color::Rgb(120, 80, 180),
        accentText: Color::Rgb(245, 235, 255),
        inverted: Color::Rgb(240, 240, 240),
        invertedText: Color::Rgb(20, 20, 20),

        success: Color::Rgb(100, 220, 100),
        error: Color::Rgb(255, 100, 100),
        warning: Color::Rgb(255, 200, 80),
        suggestion: Color::Rgb(130, 200, 255),
        info: Color::Rgb(130, 200, 255),

        surface: Color::Rgb(25, 25, 30),
        surfaceText: Color::Rgb(220, 220, 220),
        muted: Color::Rgb(50, 50, 55),
        mutedText: Color::Rgb(140, 140, 145),
        inactive: Color::Rgb(70, 70, 75),
        inactiveText: Color::Rgb(160, 160, 165),
        border: Color::Rgb(80, 80, 85),
        borderFocus: Color::Rgb(190, 140, 255),
        borderError: Color::Rgb(255, 100, 100),
        permission: Color::Rgb(255, 190, 70),
        permissionText: Color::Rgb(30, 25, 20),

        dim: Color::Rgb(100, 100, 105),
        bold: Color::Rgb(255, 255, 255),
        link: Color::Rgb(100, 180, 255),
        code: Color::Rgb(220, 220, 180),
        codeBg: Color::Rgb(35, 35, 40),
        heading: Color::Rgb(255, 255, 255),
        blockquote: Color::Rgb(180, 180, 180),
        blockquoteBorder: Color::Rgb(80, 80, 85),
        hr: Color::Rgb(80, 80, 85),

        selection: Color::Rgb(100, 200, 255),
        selectionText: Color::Rgb(0, 0, 0),
        cursor: Color::Rgb(255, 255, 255),
        cursorText: Color::Rgb(0, 0, 0),
        searchHighlight: Color::Rgb(255, 240, 120),
        searchHighlightText: Color::Rgb(0, 0, 0),

        diffAdd: Color::Rgb(100, 220, 100),
        diffAddBg: Color::Rgb(30, 60, 30),
        diffRemove: Color::Rgb(255, 100, 100),
        diffRemoveBg: Color::Rgb(60, 30, 30),
        diffHeader: Color::Rgb(130, 170, 255),
        diffContext: Color::Rgb(180, 180, 180),

        syntaxKeyword: Color::Rgb(255, 120, 180),
        syntaxString: Color::Rgb(160, 220, 130),
        syntaxNumber: Color::Rgb(220, 180, 120),
        syntaxType: Color::Rgb(120, 200, 255),
        syntaxFunction: Color::Rgb(200, 180, 255),
        syntaxComment: Color::Rgb(100, 100, 110),
        syntaxOperator: Color::Rgb(200, 200, 200),
        syntaxPunctuation: Color::Rgb(180, 180, 180),
        syntaxBuiltin: Color::Rgb(255, 200, 100),
        syntaxConstant: Color::Rgb(255, 160, 100),
        syntaxVariable: Color::Rgb(200, 200, 220),
        syntaxParameter: Color::Rgb(180, 200, 220),
        syntaxLabel: Color::Rgb(255, 120, 120),

        iconSuccess: Color::Rgb(100, 220, 100),
        iconError: Color::Rgb(255, 100, 100),
        iconWarning: Color::Rgb(255, 200, 80),
        iconInfo: Color::Rgb(130, 200, 255),
        iconPending: Color::Rgb(160, 160, 165),
        iconLoading: Color::Rgb(160, 160, 165),

        agentRed: Color::Rgb(255, 80, 80),
        agentOrange: Color::Rgb(255, 160, 60),
        agentYellow: Color::Rgb(255, 220, 80),
        agentGreen: Color::Rgb(80, 220, 120),
        agentCyan: Color::Rgb(60, 200, 220),
        agentBlue: Color::Rgb(80, 160, 255),
        agentPurple: Color::Rgb(190, 140, 255),
        agentPink: Color::Rgb(255, 130, 200),
    }
}

/// Light theme.
fn light_theme() -> ThemeColors {
    ThemeColors {
        accent: Color::Rgb(130, 80, 220),
        accentDim: Color::Rgb(180, 150, 240),
        accentText: Color::Rgb(255, 255, 255),
        inverted: Color::Rgb(30, 30, 35),
        invertedText: Color::Rgb(240, 240, 240),

        success: Color::Rgb(40, 160, 60),
        error: Color::Rgb(200, 50, 50),
        warning: Color::Rgb(200, 150, 30),
        suggestion: Color::Rgb(50, 120, 200),
        info: Color::Rgb(50, 120, 200),

        surface: Color::Rgb(248, 248, 250),
        surfaceText: Color::Rgb(30, 30, 35),
        muted: Color::Rgb(230, 230, 235),
        mutedText: Color::Rgb(130, 130, 140),
        inactive: Color::Rgb(200, 200, 205),
        inactiveText: Color::Rgb(150, 150, 155),
        border: Color::Rgb(200, 200, 205),
        borderFocus: Color::Rgb(130, 80, 220),
        borderError: Color::Rgb(200, 50, 50),
        permission: Color::Rgb(200, 150, 30),
        permissionText: Color::Rgb(255, 255, 255),

        dim: Color::Rgb(150, 150, 155),
        bold: Color::Rgb(0, 0, 0),
        link: Color::Rgb(30, 100, 220),
        code: Color::Rgb(40, 40, 30),
        codeBg: Color::Rgb(235, 235, 240),
        heading: Color::Rgb(0, 0, 0),
        blockquote: Color::Rgb(100, 100, 100),
        blockquoteBorder: Color::Rgb(200, 200, 205),
        hr: Color::Rgb(200, 200, 205),

        selection: Color::Rgb(50, 120, 200),
        selectionText: Color::Rgb(255, 255, 255),
        cursor: Color::Rgb(0, 0, 0),
        cursorText: Color::Rgb(255, 255, 255),
        searchHighlight: Color::Rgb(255, 220, 60),
        searchHighlightText: Color::Rgb(0, 0, 0),

        diffAdd: Color::Rgb(40, 160, 60),
        diffAddBg: Color::Rgb(220, 250, 220),
        diffRemove: Color::Rgb(200, 50, 50),
        diffRemoveBg: Color::Rgb(255, 220, 220),
        diffHeader: Color::Rgb(50, 80, 200),
        diffContext: Color::Rgb(120, 120, 120),

        syntaxKeyword: Color::Rgb(180, 40, 120),
        syntaxString: Color::Rgb(60, 140, 40),
        syntaxNumber: Color::Rgb(160, 110, 30),
        syntaxType: Color::Rgb(30, 100, 200),
        syntaxFunction: Color::Rgb(100, 60, 200),
        syntaxComment: Color::Rgb(150, 150, 160),
        syntaxOperator: Color::Rgb(60, 60, 60),
        syntaxPunctuation: Color::Rgb(100, 100, 100),
        syntaxBuiltin: Color::Rgb(180, 120, 20),
        syntaxConstant: Color::Rgb(200, 100, 20),
        syntaxVariable: Color::Rgb(60, 60, 80),
        syntaxParameter: Color::Rgb(60, 80, 100),
        syntaxLabel: Color::Rgb(200, 50, 50),

        iconSuccess: Color::Rgb(40, 160, 60),
        iconError: Color::Rgb(200, 50, 50),
        iconWarning: Color::Rgb(200, 150, 30),
        iconInfo: Color::Rgb(50, 120, 200),
        iconPending: Color::Rgb(150, 150, 155),
        iconLoading: Color::Rgb(150, 150, 155),

        agentRed: Color::Rgb(200, 50, 50),
        agentOrange: Color::Rgb(220, 120, 30),
        agentYellow: Color::Rgb(180, 150, 20),
        agentGreen: Color::Rgb(40, 160, 60),
        agentCyan: Color::Rgb(20, 140, 160),
        agentBlue: Color::Rgb(30, 100, 200),
        agentPurple: Color::Rgb(130, 80, 220),
        agentPink: Color::Rgb(200, 70, 140),
    }
}

/// Light daltonized theme (deuteranopia / protanopia friendly).
fn light_daltonized_theme() -> ThemeColors {
    // TODO: load from upstream daltonized palette.
    // For now we produce a variant that swaps red/green hues for blue/orange.
    let mut c = light_theme();
    c.success = Color::Rgb(50, 130, 220); // blue instead of green
    c.error = Color::Rgb(220, 140, 40); // orange instead of red
    c.diffAdd = Color::Rgb(50, 130, 220);
    c.diffRemove = Color::Rgb(220, 140, 40);
    c.diffAddBg = Color::Rgb(220, 235, 250);
    c.diffRemoveBg = Color::Rgb(250, 235, 220);
    c.iconSuccess = c.success;
    c.iconError = c.error;
    c.syntaxKeyword = Color::Rgb(200, 80, 40);
    c.syntaxString = Color::Rgb(50, 130, 220);
    c.syntaxBuiltin = Color::Rgb(200, 140, 20);
    c
}

/// Dark daltonized theme.
fn dark_daltonized_theme() -> ThemeColors {
    let mut c = dark_theme();
    c.success = Color::Rgb(80, 160, 240);
    c.error = Color::Rgb(240, 160, 60);
    c.diffAdd = Color::Rgb(80, 160, 240);
    c.diffRemove = Color::Rgb(240, 160, 60);
    c.diffAddBg = Color::Rgb(25, 50, 70);
    c.diffRemoveBg = Color::Rgb(70, 45, 20);
    c.iconSuccess = c.success;
    c.iconError = c.error;
    c.syntaxKeyword = Color::Rgb(240, 120, 60);
    c.syntaxString = Color::Rgb(80, 160, 240);
    c.syntaxBuiltin = Color::Rgb(240, 180, 40);
    c
}

/// Light ANSI theme.
fn light_ansi_theme() -> ThemeColors {
    // Use terminal ANSI colours with light background.
    let mut c = light_theme();
    c.accent = Color::Indexed(5); // magenta
    c.success = Color::Indexed(2); // green
    c.error = Color::Indexed(1); // red
    c.warning = Color::Indexed(3); // yellow
    c.suggestion = Color::Indexed(4); // blue
    c.info = Color::Indexed(6); // cyan
    c
}

/// Dark ANSI theme.
fn dark_ansi_theme() -> ThemeColors {
    // Use terminal ANSI colours with dark background.
    let mut c = dark_theme();
    c.accent = Color::Indexed(5);
    c.success = Color::Indexed(2);
    c.error = Color::Indexed(1);
    c.warning = Color::Indexed(3);
    c.suggestion = Color::Indexed(4);
    c.info = Color::Indexed(6);
    c
}

/// Retrieve the colour table for a given theme name.
pub fn get_theme(name: &ThemeName) -> &'static ThemeColors {
    // One-shot initialisation; the maps are tiny so a single static is fine.
    static THEMES: OnceLock<[ThemeColors; 6]> = OnceLock::new();
    let themes = THEMES.get_or_init(|| {
        let arr = [
            dark_theme(),
            light_theme(),
            light_daltonized_theme(),
            dark_daltonized_theme(),
            light_ansi_theme(),
            dark_ansi_theme(),
        ];
        arr
    });
    match name {
        ThemeName::Dark => &themes[0],
        ThemeName::Light => &themes[1],
        ThemeName::LightDaltonized => &themes[2],
        ThemeName::DarkDaltonized => &themes[3],
        ThemeName::LightAnsi => &themes[4],
        ThemeName::DarkAnsi => &themes[5],
    }
}

// ---------------------------------------------------------------------------
// ThemeProvider — runtime state
// ---------------------------------------------------------------------------

/// Runtime theme holder.
///
/// Wraps the current `ThemeName` and caches the resolved `ThemeColors` pointer.
/// Attach one instance to `App` and pass `&ThemeColors` to render functions.
pub struct ThemeProvider {
    #[cfg(test)]
    setting: ThemeSetting,
    current: ThemeName,
}

impl ThemeProvider {
    /// Create a new provider defaulting to Dark.
    pub fn new() -> Self {
        Self::from_setting(ThemeSetting::Named(ThemeName::Dark))
    }

    /// Create a provider with an explicit starting theme.
    #[cfg(test)]
    pub fn with_name(name: ThemeName) -> Self {
        Self::from_setting(ThemeSetting::Named(name))
    }

    pub fn from_setting(setting: ThemeSetting) -> Self {
        let current = setting.resolved_name();
        Self {
            #[cfg(test)]
            setting,
            current,
        }
    }

    pub fn from_setting_str(value: Option<&str>) -> Self {
        Self::from_setting(ThemeSetting::parse(value))
    }

    pub fn from_user_settings() -> Self {
        Self::from_setting(load_theme_setting().unwrap_or_else(|err| {
            tracing::warn!(error = %err, "failed to load UI theme setting; using dark theme");
            ThemeSetting::Named(ThemeName::Dark)
        }))
    }

    /// The current `ThemeName`.
    #[cfg(test)]
    pub fn name(&self) -> &ThemeName {
        &self.current
    }

    #[cfg(test)]
    pub fn setting(&self) -> &ThemeSetting {
        &self.setting
    }

    /// The resolved colour table for the current theme.
    pub fn colors(&self) -> &'static ThemeColors {
        get_theme(&self.current)
    }

    pub fn legacy_theme(&self) -> Theme {
        Theme::from_design_colors(self.colors())
    }

    /// Switch to a different theme.
    #[cfg(test)]
    pub fn set_theme(&mut self, name: ThemeName) {
        self.set_setting(ThemeSetting::Named(name));
    }

    #[cfg(test)]
    pub fn set_setting(&mut self, setting: ThemeSetting) {
        self.current = setting.resolved_name();
        self.setting = setting;
    }

    #[cfg(test)]
    pub fn refresh_auto(&mut self) {
        if matches!(self.setting, ThemeSetting::Auto) {
            self.current = self.setting.resolved_name();
        }
    }

    /// Iterate over all theme names (for selection UIs).
    #[cfg(test)]
    pub fn all_themes() -> &'static [ThemeName] {
        ThemeName::ALL
    }
}

impl Default for ThemeProvider {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export the legacy Theme struct from rendering/theme.rs so existing
// `use crate::ui::theme::Theme` imports continue to work.
pub use super::rendering_theme::Theme;

impl Theme {
    pub fn from_design_colors(colors: &ThemeColors) -> Self {
        let theme_color =
            |key: &str, fallback: Color| color::resolve_color(key, colors).unwrap_or(fallback);

        macro_rules! build_theme {
            ($($extra:tt)*) => {
                Self {
                    assistant_name: Style::default()
                        .fg(theme_color("accent", colors.accent))
                        .add_modifier(Modifier::BOLD),
                    user_name: Style::default()
                        .fg(theme_color("suggestion", colors.suggestion))
                        .add_modifier(Modifier::BOLD),
                    system_name: Style::default()
                        .fg(theme_color("dim", colors.dim))
                        .add_modifier(Modifier::ITALIC),
                    tool_name: Style::default()
                        .fg(theme_color("code", colors.code))
                        .add_modifier(Modifier::BOLD),
                    tool_result: Style::default().fg(colors.diffContext),
                    error: Style::default()
                        .fg(colors.error)
                        .add_modifier(Modifier::BOLD),
                    warning: Style::default().fg(colors.warning),
                    info: Style::default().fg(colors.info),
                    prompt: Style::default()
                        .fg(colors.suggestion)
                        .add_modifier(Modifier::BOLD),
                    border: Style::default().fg(colors.border),
                    code: Style::default().fg(colors.code).bg(colors.codeBg),
                    code_bg: colors.codeBg,
                    thinking: Style::default()
                        .fg(colors.dim)
                        .add_modifier(Modifier::ITALIC),
                    dim: Style::default().fg(colors.dim),
                    heading: Style::default()
                        .fg(colors.heading)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    bold: Style::default()
                        .fg(colors.bold)
                        .add_modifier(Modifier::BOLD),
                    italic: Style::default().add_modifier(Modifier::ITALIC),
                    link: Style::default()
                        .fg(colors.link)
                        .add_modifier(Modifier::UNDERLINED),
                    syntax_keyword: Style::default().fg(colors.syntaxKeyword),
                    syntax_string: Style::default().fg(colors.syntaxString),
                    syntax_comment: Style::default()
                        .fg(colors.syntaxComment)
                        .add_modifier(Modifier::ITALIC),
                    syntax_type: Style::default().fg(colors.syntaxType),
                    syntax_function: Style::default().fg(colors.syntaxFunction),
                    syntax_number: Style::default().fg(colors.syntaxNumber),
                    syntax_operator: Style::default().fg(colors.syntaxOperator),
                    syntax_builtin: Style::default().fg(colors.syntaxBuiltin),
                    syntax_punctuation: Style::default().fg(colors.syntaxPunctuation),
                    diff_add: Style::default().fg(colors.diffAdd),
                    diff_remove: Style::default().fg(colors.diffRemove),
                    diff_context: Style::default().fg(colors.diffContext),
                    diff_header: Style::default()
                        .fg(colors.diffHeader)
                        .add_modifier(Modifier::BOLD),
                    selected: Style::default()
                        .fg(colors.selectionText)
                        .bg(colors.selection)
                        .add_modifier(Modifier::BOLD),
                    unselected: Style::default().fg(colors.inactiveText),
                    progress_fill: Style::default()
                        .fg(color::resolve_color("success", colors).unwrap_or(colors.success)),
                    progress_empty: Style::default()
                        .fg(color::resolve_color("inactive", colors).unwrap_or(colors.inactive)),
                    $($extra)*
                }
            };
        }
        build_theme!()
    }
}

pub fn load_theme_setting() -> Result<ThemeSetting, String> {
    read_theme_setting_from_path(&allthecodes_config::settings::user_settings_path())
}

fn read_theme_setting_from_path(path: &Path) -> Result<ThemeSetting, String> {
    if !path.exists() {
        return Ok(ThemeSetting::Named(ThemeName::Dark));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;
    if content.trim().is_empty() {
        return Ok(ThemeSetting::Named(ThemeName::Dark));
    }
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|err| format!("failed to parse {}: {}", path.display(), err))?;
    Ok(ThemeSetting::parse(
        value.get("theme").and_then(serde_json::Value::as_str),
    ))
}

#[cfg(test)]
fn write_theme_setting_to_path(path: &Path, setting: &ThemeSetting) -> Result<(), String> {
    let mut value = if path.exists() {
        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;
        if content.trim().is_empty() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&content)
                .map_err(|err| format!("failed to parse {}: {}", path.display(), err))?
        }
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    if !value.is_object() {
        value = serde_json::Value::Object(serde_json::Map::new());
    }
    value
        .as_object_mut()
        .expect("theme settings value is object")
        .insert(
            "theme".to_string(),
            serde_json::Value::String(setting.as_settings_value().to_string()),
        );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|err| format!("failed to serialize settings: {}", err))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, pretty)
        .map_err(|err| format!("failed to write {}: {}", tmp.display(), err))?;
    std::fs::rename(&tmp, path).map_err(|err| {
        format!(
            "failed to rename {} -> {}: {}",
            tmp.display(),
            path.display(),
            err
        )
    })
}

fn terminal_prefers_dark() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| terminal_prefers_dark_from_colorfgbg(&value))
        .unwrap_or(true)
}

fn terminal_prefers_dark_from_colorfgbg(value: &str) -> Option<bool> {
    let bg = value.rsplit(';').next()?.trim().parse::<u16>().ok()?;
    match bg {
        0..=6 | 8 => Some(true),
        7 | 9..=15 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark() {
        let p = ThemeProvider::new();
        assert_eq!(p.name(), &ThemeName::Dark);
    }

    #[test]
    fn set_theme_switches_active() {
        let mut p = ThemeProvider::new();
        p.set_theme(ThemeName::Light);
        assert_eq!(p.name(), &ThemeName::Light);
        assert_eq!(p.setting(), &ThemeSetting::Named(ThemeName::Light));
    }

    #[test]
    fn get_theme_returns_non_empty_table() {
        for name in ThemeName::ALL {
            let colors = get_theme(name);
            // spot-check a few fields are distinct per variant
            assert_ne!(
                colors.accent, colors.inverted,
                "accent != inverted for {name:?}"
            );
        }
    }

    #[test]
    fn dark_and_light_have_different_surface() {
        let dark = get_theme(&ThemeName::Dark);
        let light = get_theme(&ThemeName::Light);
        assert_ne!(dark.surface, light.surface);
    }

    #[test]
    fn theme_name_is_dark_matches_intent() {
        assert!(ThemeName::Dark.is_dark());
        assert!(!ThemeName::Light.is_dark());
    }

    #[test]
    fn theme_names_have_labels_and_provider_lists_all_variants() {
        assert_eq!(ThemeName::Dark.label(), "Dark");
        assert_eq!(ThemeName::LightDaltonized.label(), "Light (Daltonized)");
        assert_eq!(ThemeProvider::all_themes(), ThemeName::ALL);
    }

    #[test]
    fn auto_provider_can_refresh_from_terminal_preference() {
        let mut p = ThemeProvider::from_setting(ThemeSetting::Auto);
        let before = *p.name();
        p.refresh_auto();
        assert_eq!(*p.name(), before);
        assert_eq!(p.setting(), &ThemeSetting::Auto);
    }

    #[test]
    fn parses_theme_settings() {
        assert_eq!(ThemeSetting::parse(Some("auto")), ThemeSetting::Auto);
        assert_eq!(
            ThemeSetting::parse(Some("light_ansi")),
            ThemeSetting::Named(ThemeName::LightAnsi)
        );
        assert_eq!(
            ThemeSetting::parse(Some("unknown")),
            ThemeSetting::Named(ThemeName::Dark)
        );
    }

    #[test]
    fn colorfgbg_auto_detection_uses_background_slot() {
        assert_eq!(terminal_prefers_dark_from_colorfgbg("15;0"), Some(true));
        assert_eq!(terminal_prefers_dark_from_colorfgbg("0;15"), Some(false));
        assert_eq!(terminal_prefers_dark_from_colorfgbg("bad"), None);
    }

    #[test]
    fn theme_setting_round_trips_settings_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model":"x"}"#).unwrap();

        write_theme_setting_to_path(&path, &ThemeSetting::Auto).unwrap();
        assert_eq!(
            read_theme_setting_from_path(&path).unwrap(),
            ThemeSetting::Auto
        );

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["model"], "x");
        assert_eq!(value["theme"], "auto");
    }

    #[test]
    fn legacy_theme_uses_selected_theme_colors() {
        let provider = ThemeProvider::with_name(ThemeName::Light);
        let legacy = provider.legacy_theme();
        assert_eq!(legacy.info.fg, Some(provider.colors().info));
    }
}
