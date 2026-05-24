// test infrastructure — theme variant support not wired to production TUI
use ratatui::style::{Color, Modifier, Style};

/// Named theme variants.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
    Tron,
}

#[cfg(test)]
impl ThemeKind {
    pub fn build(self) -> Theme {
        match self {
            ThemeKind::Dark => Theme::default(),
            ThemeKind::Light => light_theme(),
            ThemeKind::Tron => tron_theme(),
        }
    }
}

/// Color theme for the terminal UI.
///
/// All styles are pre-composed so widgets can simply clone the style they need
/// without having to compose foreground/background/modifiers at render time.
pub struct Theme {
    /// Legacy assistant-name style retained for renderers that still need a label.
    pub assistant_name: Style,
    /// Legacy user-name style retained for renderers that still need a label.
    pub user_name: Style,
    /// Style for system messages.
    pub system_name: Style,
    /// Style for tool names shown in tool_use blocks.
    pub tool_name: Style,
    /// Style for tool result content.
    pub tool_result: Style,
    /// Style for error text.
    pub error: Style,
    /// Style for warning text.
    pub warning: Style,
    /// Style for informational text.
    pub info: Style,
    /// Style for the input prompt symbol (e.g. "> ").
    pub prompt: Style,
    /// Style for borders and separators.
    pub border: Style,
    /// Style for inline code and code blocks.
    pub code: Style,
    /// Style for code block background (used by syntax highlighting).
    pub code_bg: Color,
    /// Style for thinking blocks.
    pub thinking: Style,
    /// Style for dimmed / secondary text.
    pub dim: Style,
    /// Style for headings in markdown.
    pub heading: Style,
    /// Style for bold text.
    pub bold: Style,
    /// Style for italic text.
    pub italic: Style,
    /// Style for links in markdown.
    pub link: Style,
    /// Style for syntax keyword tokens.
    pub syntax_keyword: Style,
    /// Style for syntax string tokens.
    pub syntax_string: Style,
    /// Style for syntax comment tokens.
    pub syntax_comment: Style,
    /// Style for syntax type tokens.
    pub syntax_type: Style,
    /// Style for syntax function/method tokens.
    pub syntax_function: Style,
    /// Style for syntax number tokens.
    pub syntax_number: Style,
    /// Style for syntax operator tokens.
    pub syntax_operator: Style,
    /// Style for syntax built-in constants.
    pub syntax_builtin: Style,
    /// Style for syntax punctuation / delimiters.
    pub syntax_punctuation: Style,
    /// Style for diff additions.
    pub diff_add: Style,
    /// Style for diff deletions.
    pub diff_remove: Style,
    /// Style for diff context (unchanged) lines.
    pub diff_context: Style,
    /// Style for the diff header/separator.
    pub diff_header: Style,
    /// Style for selected item in dialogs.
    pub selected: Style,
    /// Style for unselected items in dialogs.
    pub unselected: Style,
    /// Style for the filled portion of a progress bar.
    pub progress_fill: Style,
    /// Style for the empty portion of a progress bar.
    pub progress_empty: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            assistant_name: Style::default()
                .fg(Color::Rgb(190, 140, 255))
                .add_modifier(Modifier::BOLD),
            user_name: Style::default()
                .fg(Color::Rgb(100, 200, 255))
                .add_modifier(Modifier::BOLD),
            system_name: Style::default()
                .fg(Color::Rgb(180, 180, 180))
                .add_modifier(Modifier::ITALIC),
            tool_name: Style::default()
                .fg(Color::Rgb(255, 200, 100))
                .add_modifier(Modifier::BOLD),
            tool_result: Style::default().fg(Color::Rgb(160, 160, 160)),
            error: Style::default()
                .fg(Color::Rgb(255, 100, 100))
                .add_modifier(Modifier::BOLD),
            warning: Style::default().fg(Color::Rgb(255, 200, 80)),
            info: Style::default().fg(Color::Rgb(130, 200, 255)),
            prompt: Style::default()
                .fg(Color::Rgb(100, 200, 255))
                .add_modifier(Modifier::BOLD),
            border: Style::default().fg(Color::Rgb(80, 80, 80)),
            code: Style::default()
                .fg(Color::Rgb(220, 220, 180))
                .bg(Color::Rgb(40, 40, 40)),
            code_bg: Color::Rgb(40, 40, 40),
            thinking: Style::default()
                .fg(Color::Rgb(120, 120, 120))
                .add_modifier(Modifier::ITALIC),
            dim: Style::default().fg(Color::Rgb(100, 100, 100)),
            heading: Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            bold: Style::default().add_modifier(Modifier::BOLD),
            italic: Style::default().add_modifier(Modifier::ITALIC),
            link: Style::default()
                .fg(Color::Rgb(100, 180, 255))
                .add_modifier(Modifier::UNDERLINED),
            syntax_keyword: Style::default().fg(Color::Rgb(255, 100, 200)),
            syntax_string: Style::default().fg(Color::Rgb(180, 220, 100)),
            syntax_comment: Style::default()
                .fg(Color::Rgb(120, 120, 120))
                .add_modifier(Modifier::ITALIC),
            syntax_type: Style::default().fg(Color::Rgb(100, 200, 255)),
            syntax_function: Style::default().fg(Color::Rgb(130, 180, 255)),
            syntax_number: Style::default().fg(Color::Rgb(255, 180, 80)),
            syntax_operator: Style::default().fg(Color::Rgb(200, 200, 200)),
            syntax_builtin: Style::default().fg(Color::Rgb(255, 200, 150)),
            syntax_punctuation: Style::default().fg(Color::Rgb(180, 180, 180)),
            diff_add: Style::default().fg(Color::Rgb(100, 220, 100)),
            diff_remove: Style::default().fg(Color::Rgb(255, 100, 100)),
            diff_context: Style::default().fg(Color::Rgb(180, 180, 180)),
            diff_header: Style::default()
                .fg(Color::Rgb(130, 170, 255))
                .add_modifier(Modifier::BOLD),
            selected: Style::default()
                .fg(Color::Rgb(0, 0, 0))
                .bg(Color::Rgb(100, 200, 255))
                .add_modifier(Modifier::BOLD),
            unselected: Style::default().fg(Color::Rgb(200, 200, 200)),
            progress_fill: Style::default().fg(Color::Rgb(100, 220, 100)),
            progress_empty: Style::default().fg(Color::Rgb(60, 60, 60)),
        }
    }
}

impl Theme {
    /// Create a theme with default colors (dark theme).
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a theme from a named variant.
    #[cfg(test)]
    pub fn named(kind: ThemeKind) -> Self {
        kind.build()
    }
}

#[cfg(test)]
fn light_theme() -> Theme {
    let mut t = Theme::default();
    t.assistant_name = Style::default()
        .fg(Color::Rgb(120, 60, 200))
        .add_modifier(Modifier::BOLD);
    t.user_name = Style::default()
        .fg(Color::Rgb(0, 100, 200))
        .add_modifier(Modifier::BOLD);
    t.system_name = Style::default()
        .fg(Color::Rgb(100, 100, 100))
        .add_modifier(Modifier::ITALIC);
    t.code = Style::default()
        .fg(Color::Rgb(30, 30, 30))
        .bg(Color::Rgb(230, 230, 220));
    t.code_bg = Color::Rgb(230, 230, 220);
    t.border = Style::default().fg(Color::Rgb(180, 180, 180));
    t.dim = Style::default().fg(Color::Rgb(140, 140, 140));
    t.heading = Style::default()
        .fg(Color::Rgb(0, 0, 0))
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    t.link = Style::default()
        .fg(Color::Rgb(0, 80, 200))
        .add_modifier(Modifier::UNDERLINED);
    t.diff_add = Style::default().fg(Color::Rgb(0, 140, 0));
    t.diff_remove = Style::default().fg(Color::Rgb(200, 0, 0));
    t.progress_fill = Style::default().fg(Color::Rgb(0, 160, 0));
    t.progress_empty = Style::default().fg(Color::Rgb(200, 200, 200));
    t.syntax_keyword = Style::default().fg(Color::Rgb(180, 0, 120));
    t.syntax_string = Style::default().fg(Color::Rgb(80, 140, 0));
    t.syntax_comment = Style::default()
        .fg(Color::Rgb(120, 120, 120))
        .add_modifier(Modifier::ITALIC);
    t.syntax_type = Style::default().fg(Color::Rgb(0, 100, 200));
    t.syntax_function = Style::default().fg(Color::Rgb(0, 60, 180));
    t.syntax_number = Style::default().fg(Color::Rgb(200, 100, 0));
    t.syntax_operator = Style::default().fg(Color::Rgb(60, 60, 60));
    t.syntax_builtin = Style::default().fg(Color::Rgb(180, 100, 0));
    t.syntax_punctuation = Style::default().fg(Color::Rgb(80, 80, 80));
    t
}

#[cfg(test)]
fn tron_theme() -> Theme {
    let mut t = Theme::default();
    t.assistant_name = Style::default()
        .fg(Color::Rgb(0, 255, 200))
        .add_modifier(Modifier::BOLD);
    t.user_name = Style::default()
        .fg(Color::Rgb(100, 200, 255))
        .add_modifier(Modifier::BOLD);
    t.system_name = Style::default()
        .fg(Color::Rgb(0, 180, 180))
        .add_modifier(Modifier::ITALIC);
    t.tool_name = Style::default()
        .fg(Color::Rgb(255, 200, 0))
        .add_modifier(Modifier::BOLD);
    t.code = Style::default()
        .fg(Color::Rgb(0, 255, 200))
        .bg(Color::Rgb(0, 20, 40));
    t.code_bg = Color::Rgb(0, 20, 40);
    t.border = Style::default().fg(Color::Rgb(0, 100, 100));
    t.heading = Style::default()
        .fg(Color::Rgb(0, 255, 200))
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    t.link = Style::default()
        .fg(Color::Rgb(0, 200, 255))
        .add_modifier(Modifier::UNDERLINED);
    t.info = Style::default().fg(Color::Rgb(0, 200, 255));
    t.prompt = Style::default()
        .fg(Color::Rgb(0, 255, 200))
        .add_modifier(Modifier::BOLD);
    t.syntax_keyword = Style::default().fg(Color::Rgb(255, 100, 200));
    t.syntax_string = Style::default().fg(Color::Rgb(200, 255, 100));
    t.syntax_comment = Style::default()
        .fg(Color::Rgb(0, 140, 140))
        .add_modifier(Modifier::ITALIC);
    t.syntax_type = Style::default().fg(Color::Rgb(100, 200, 255));
    t.syntax_function = Style::default().fg(Color::Rgb(0, 180, 255));
    t.syntax_number = Style::default().fg(Color::Rgb(255, 180, 80));
    t.syntax_operator = Style::default().fg(Color::Rgb(0, 200, 200));
    t.syntax_builtin = Style::default().fg(Color::Rgb(255, 200, 150));
    t.syntax_punctuation = Style::default().fg(Color::Rgb(0, 160, 160));
    t.diff_add = Style::default().fg(Color::Rgb(0, 255, 100));
    t.diff_remove = Style::default().fg(Color::Rgb(255, 80, 80));
    t.progress_fill = Style::default().fg(Color::Rgb(0, 255, 200));
    t.progress_empty = Style::default().fg(Color::Rgb(0, 60, 60));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_theme_variants_build_distinct_styles() {
        let dark = Theme::new();
        let light = Theme::named(ThemeKind::Light);
        let tron = ThemeKind::Tron.build();

        assert_ne!(dark.code_bg, light.code_bg);
        assert_ne!(light.code_bg, tron.code_bg);
        assert_eq!(ThemeKind::Dark.build().code_bg, dark.code_bg);
    }
}
