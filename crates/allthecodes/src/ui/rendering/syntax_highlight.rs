//! Syntax highlighting for code blocks.
//!
//! Uses `syntect` when the `syntect` feature is enabled, providing
//! token-level coloring for fenced code blocks.
//!
//! When the feature is disabled, all functions degrade gracefully to
//! plain-text passthrough.

#[cfg(any(test, feature = "syntect"))]
use std::borrow::Cow;

use ratatui::text::Span;

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Language detection helpers
// ---------------------------------------------------------------------------

/// Map pulldown_cmark language identifiers to syntect syntax names.
///
/// pulldown_cmark gives us the raw info string from ```lang. Syntect
/// uses slightly different names for some languages.
#[cfg(any(test, feature = "syntect"))]
const LANG_ALIASES: &[(&str, &[&str])] = &[
    ("javascript", &["javascript", "js", "node"]),
    ("typescript", &["typescript", "ts"]),
    ("tsx", &["tsx", "typescriptreact"]),
    ("jsx", &["jsx", "javascriptreact"]),
    ("python", &["python", "py", "python3"]),
    ("ruby", &["ruby", "rb"]),
    ("rust", &["rust", "rs"]),
    ("go", &["go", "golang"]),
    ("bash", &["shell", "sh", "bash", "zsh"]),
    ("yaml", &["yaml", "yml"]),
    ("json", &["json"]),
    ("toml", &["toml"]),
    ("markdown", &["markdown", "md"]),
    ("html", &["html"]),
    ("css", &["css"]),
    ("sql", &["sql"]),
    ("c", &["c", "h"]),
    ("cpp", &["cpp", "c++", "cc"]),
    ("java", &["java"]),
    ("kotlin", &["kotlin", "kt"]),
    ("swift", &["swift"]),
    ("dart", &["dart"]),
    ("lua", &["lua"]),
    ("php", &["php"]),
    ("r", &["r"]),
    ("scala", &["scala"]),
    ("haskell", &["haskell", "hs"]),
    ("ocaml", &["ocaml", "ml"]),
    ("nim", &["nim"]),
    ("powershell", &["powershell", "ps1"]),
    ("dockerfile", &["dockerfile"]),
    ("makefile", &["makefile", "make"]),
    ("graphql", &["graphql", "gql"]),
    ("protobuf", &["protobuf", "proto"]),
    ("latex", &["latex", "tex"]),
    ("xml", &["xml"]),
];

/// Resolve a raw language string (from fence info) to a syntect-compatible
/// syntax name, or None if unknown.
#[cfg(any(test, feature = "syntect"))]
fn resolve_lang(lang: &str) -> Option<&'static str> {
    let lower = normalize_lang_token(lang)?;
    for &(canonical, aliases) in LANG_ALIASES {
        if canonical == lower.as_str() || aliases.contains(&lower.as_str()) {
            return Some(canonical);
        }
    }
    None
}

#[cfg(any(test, feature = "syntect"))]
fn normalize_lang_token(lang: &str) -> Option<String> {
    let token = lang
        .trim()
        .trim_start_matches('.')
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '{' | '}'))
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('.');
    if token.is_empty() {
        None
    } else {
        Some(token.to_ascii_lowercase())
    }
}

#[cfg(any(test, feature = "syntect"))]
fn preferred_syntect_token(lang: &str) -> Option<Cow<'static, str>> {
    let normalized = normalize_lang_token(lang)?;
    if let Some(canonical) = resolve_lang(&normalized) {
        Some(Cow::Borrowed(canonical))
    } else {
        Some(Cow::Owned(normalized))
    }
}

// ---------------------------------------------------------------------------
// Non-syntect fallback
// ---------------------------------------------------------------------------

/// When syntect is disabled, highlight_code_block returns spans with the
/// theme's code style.
fn fallback_highlight(code: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for segment in code.split_inclusive('\n') {
        let (line, has_newline) = match segment.strip_suffix('\n') {
            Some(line) => (line, true),
            None => (segment, false),
        };
        spans.push(Span::styled(line.to_string(), theme.code));
        if has_newline {
            spans.push(Span::raw("\n"));
        }
    }
    spans
}

// ---------------------------------------------------------------------------
// Syntect-enabled implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "syntect")]
mod imp {
    use std::sync::OnceLock;

    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::Span;
    use syntect::highlighting::{FontStyle, Style as SyntectStyle, ThemeSet};
    use syntect::parsing::SyntaxSet;

    use super::preferred_syntect_token;
    use crate::ui::theme::Theme;

    /// Lazily-loaded syntax set (cached across all calls).
    fn syntax_set() -> &'static SyntaxSet {
        static SS: OnceLock<SyntaxSet> = OnceLock::new();
        SS.get_or_init(SyntaxSet::load_defaults_newlines)
    }

    fn theme_set() -> &'static ThemeSet {
        static TS: OnceLock<ThemeSet> = OnceLock::new();
        TS.get_or_init(ThemeSet::load_defaults)
    }

    /// Convert a syntect token style to ratatui `Style`.
    fn syntect_style_to_ratatui(s: &SyntectStyle, base: Style) -> Style {
        let mut result = base;

        let c = s.foreground;
        if c.a > 0 {
            result = result.fg(Color::Rgb(c.r, c.g, c.b));
        }

        if s.font_style.contains(FontStyle::BOLD) {
            result = result.add_modifier(Modifier::BOLD);
        }
        if s.font_style.contains(FontStyle::ITALIC) {
            result = result.add_modifier(Modifier::ITALIC);
        }
        if s.font_style.contains(FontStyle::UNDERLINE) {
            result = result.add_modifier(Modifier::UNDERLINED);
        }

        result
    }

    /// Highlight code with syntect, producing ratatui spans.
    pub(crate) fn highlight(code: &str, lang: &str, theme: &Theme) -> Vec<Span<'static>> {
        let ss = syntax_set();

        let syntax = preferred_syntect_token(lang).and_then(|token| {
            ss.find_syntax_by_token(token.as_ref())
                .or_else(|| ss.find_syntax_by_extension(token.as_ref()))
        });
        let Some(syntax) = syntax else {
            return super::fallback_highlight(code, theme);
        };

        let mut highlighter =
            syntect::easy::HighlightLines::new(syntax, &theme_set().themes["base16-ocean.dark"]);

        let mut spans: Vec<Span<'static>> = Vec::new();
        for segment in code.split_inclusive('\n') {
            let (line, has_newline) = match segment.strip_suffix('\n') {
                Some(line) => (line, true),
                None => (segment, false),
            };

            let Ok(ranges) = highlighter.highlight_line(line, ss) else {
                // If syntect fails on a line, fall back to theme.code
                spans.push(Span::styled(line.to_string(), theme.code));
                if has_newline {
                    spans.push(Span::raw("\n"));
                }
                continue;
            };

            if ranges.is_empty() {
                spans.push(Span::styled(String::new(), theme.code));
            }
            for (style, text) in ranges {
                let ratatui_style = syntect_style_to_ratatui(&style, theme.code);
                spans.push(Span::styled(text.to_string(), ratatui_style));
            }
            if has_newline {
                spans.push(Span::raw("\n"));
            }
        }

        spans
    }

    pub(crate) fn supports_language(lang: &str) -> bool {
        let ss = syntax_set();
        let Some(token) = preferred_syntect_token(lang) else {
            return false;
        };
        ss.find_syntax_by_token(token.as_ref()).is_some()
            || ss.find_syntax_by_extension(token.as_ref()).is_some()
    }
}

#[cfg(not(feature = "syntect"))]
mod imp {
    use ratatui::text::Span;

    use super::fallback_highlight;
    use crate::ui::theme::Theme;

    pub(crate) fn highlight(code: &str, lang: &str, theme: &Theme) -> Vec<Span<'static>> {
        let _ = lang; // unused without syntect
        fallback_highlight(code, theme)
    }

    #[cfg(test)]
    pub(crate) fn supports_language(_lang: &str) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Highlight a code block with syntax coloring.
///
/// When the `syntect` feature is enabled, uses syntect for token-level
/// coloring based on the detected language. Falls back to `theme.code`
/// style when syntect is disabled or the language is unsupported.
pub fn highlight_code_block(code: &str, lang: &str, theme: &Theme) -> Vec<Span<'static>> {
    if code.is_empty() {
        return vec![Span::styled(String::new(), theme.code)];
    }

    // Fast path: no language = no syntax highlighting
    if lang.is_empty() {
        return fallback_highlight(code, theme);
    }

    imp::highlight(code, lang, theme)
}

/// Check whether a given language identifier is supported for highlighting.
#[cfg(test)]
pub fn supports_language(lang: &str) -> bool {
    imp::supports_language(lang)
}

/// Return the list of all supported language identifiers.
#[cfg(test)]
pub fn supported_languages() -> Vec<&'static str> {
    let mut langs: Vec<&str> = LANG_ALIASES
        .iter()
        .map(|(canonical, _)| *canonical)
        .collect();
    langs.sort();
    langs.dedup();
    langs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_common_languages() {
        assert_eq!(resolve_lang("rs"), Some("rust"));
        assert_eq!(resolve_lang("Rust"), Some("rust"));
        assert_eq!(resolve_lang("py"), Some("python"));
        assert_eq!(resolve_lang("javascript"), Some("javascript"));
        assert_eq!(resolve_lang("bash"), Some("bash"));
        assert_eq!(resolve_lang("sh"), Some("bash"));
        assert_eq!(resolve_lang("unknown_lang_12345"), None);
    }

    #[test]
    fn empty_code_returns_empty_span() {
        let theme = Theme::default();
        let spans = highlight_code_block("", "rs", &theme);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn no_lang_returns_fallback() {
        let theme = Theme::default();
        let spans = highlight_code_block("fn main() {}", "", &theme);
        assert!(!spans.is_empty());
    }

    #[test]
    fn supports_detects_known_languages() {
        #[cfg(feature = "syntect")]
        {
            assert!(supports_language("rust"));
            assert!(supports_language("python"));
            assert!(supports_language("bash"));
            assert!(supports_language("sh"));
            assert!(!supports_language(""));
            assert!(!supports_language("foobarbaz"));
        }

        #[cfg(not(feature = "syntect"))]
        {
            assert!(!supports_language("rust"));
            assert!(!supports_language("python"));
            assert!(!supports_language(""));
            assert!(!supports_language("foobarbaz"));
        }
    }

    #[test]
    fn preferred_tokens_and_supported_language_list_are_stable() {
        assert_eq!(preferred_syntect_token("rs").as_deref(), Some("rust"));
        assert_eq!(
            preferred_syntect_token(".custom-lang extra").as_deref(),
            Some("custom-lang")
        );
        let languages = supported_languages();
        assert!(languages.binary_search(&"rust").is_ok());
        assert!(languages.binary_search(&"python").is_ok());
    }

    #[test]
    fn unknown_language_uses_plain_code_style() {
        let theme = Theme::default();
        let spans = highlight_code_block("let x = 1;\n", "definitely_not_real_lang", &theme);
        assert!(!spans.is_empty());
        for span in spans {
            if span.content.as_ref() == "\n" {
                continue;
            }
            assert_eq!(span.style, theme.code);
        }
    }
}
