//! Output style → prompt section mapping.
//!
//! Output styles let the user influence the model's tone, depth of
//! explanation, and pedagogical approach without rewriting the full
//! system prompt.
//!
//! Per issue #9 scope:
//!   - Built-in styles (`Default` / `Explanatory` / `Learning`) are
//!     supported and emit a dedicated `# Output Style` section.
//!   - Custom styles are loaded from `<cwd>/.allthecodes/output-styles/<name>.md`
//!     or `~/.allthecodes/output-styles/<name>.md` (first match wins), with
//!     legacy project styles as read-only fallback.
//!   - `keep-coding-instructions` is NOT implemented (would gate the
//!     default coding-prompt sections, which is out of scope for this
//!     iteration).

use std::path::{Path, PathBuf};

use crate::config::paths::data_root;

/// Resolved output style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputStyle {
    /// Standard coding-assistant tone (no extra section emitted).
    Default,
    /// Add longer explanations and rationale.
    Explanatory,
    /// Frame work as teaching the user.
    Learning,
    /// User-defined style loaded from disk.
    Custom { name: String, body: String },
}

/// Resolved style plus any user-visible fallback diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStyleResolution {
    pub style: OutputStyle,
    pub fallback_diagnostic: Option<String>,
}

impl OutputStyle {
    /// Stable canonical name used in `/config show` and the schema.
    pub fn name(&self) -> &str {
        match self {
            OutputStyle::Default => "default",
            OutputStyle::Explanatory => "explanatory",
            OutputStyle::Learning => "learning",
            OutputStyle::Custom { name, .. } => name.as_str(),
        }
    }
}

/// Resolve the named output style.
///
/// Built-in names always win over custom files of the same name (so
/// users cannot accidentally shadow `default`). Unknown names fall
/// back to a disk lookup; if the file is missing, we return `Default`.
pub fn resolve(name: &str, cwd: &Path) -> OutputStyle {
    resolve_with_diagnostic(name, cwd).style
}

/// Resolve the named output style and report when a configured custom style
/// could not be loaded. Explicit `Default` remains a quiet no-op.
pub fn resolve_with_diagnostic(name: &str, cwd: &Path) -> OutputStyleResolution {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return OutputStyleResolution {
            style: OutputStyle::Default,
            fallback_diagnostic: None,
        };
    }
    let style = match trimmed.to_ascii_lowercase().as_str() {
        "default" | "" => OutputStyle::Default,
        "explanatory" => OutputStyle::Explanatory,
        "learning" => OutputStyle::Learning,
        _ => match load_custom_style(trimmed, cwd) {
            Some(body) => OutputStyle::Custom {
                name: trimmed.to_string(),
                body,
            },
            None => {
                return OutputStyleResolution {
                    style: OutputStyle::Default,
                    fallback_diagnostic: Some(format!(
                        "Configured output style `{}` was not found in project or user \
                         output-styles directories; using Default.",
                        trimmed
                    )),
                };
            }
        },
    };
    OutputStyleResolution {
        style,
        fallback_diagnostic: None,
    }
}

/// Render the prompt section for a style. Returns `None` for the
/// default style (which intentionally adds nothing).
pub fn style_section(style: &OutputStyle) -> Option<String> {
    match style {
        OutputStyle::Default => None,
        OutputStyle::Explanatory => Some(
            "# Output Style: Explanatory\n\
             When responding, include a short rationale alongside each non-trivial \
             change or recommendation. Explain *why* you chose an approach, what \
             trade-offs you considered, and call out the assumptions you are making. \
             Keep code blocks compact, but never strip the surrounding explanation."
                .to_string(),
        ),
        OutputStyle::Learning => Some(
            "# Output Style: Learning\n\
             Treat this conversation as pair-programming with someone who is \
             actively learning. Before applying a non-trivial change, briefly \
             explain the relevant concept (one or two sentences) and link it to \
             what is happening in the code. Prefer small, demonstrable steps over \
             one large change. When introducing a new API, idiom, or pattern, \
             show a minimal example before integrating it."
                .to_string(),
        ),
        OutputStyle::Custom { name, body } => {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("# Output Style: {}\n{}", name, trimmed))
            }
        }
    }
}

/// Render the prompt section for a resolved style, including fallback
/// diagnostics for missing configured custom styles.
pub fn resolution_section(resolution: &OutputStyleResolution) -> Option<String> {
    style_section(&resolution.style).or_else(|| {
        resolution
            .fallback_diagnostic
            .as_ref()
            .map(|diagnostic| format!("# Output Style Fallback\n{}", diagnostic))
    })
}

/// Search project-local then user-global directories for a custom
/// output-style markdown file.
fn load_custom_style(name: &str, cwd: &Path) -> Option<String> {
    for path in candidate_paths(name, cwd) {
        if let Ok(body) = std::fs::read_to_string(&path) {
            return Some(body);
        }
    }
    None
}

fn candidate_paths(name: &str, cwd: &Path) -> Vec<PathBuf> {
    let safe_name = sanitize_name(name);
    let mut out = Vec::new();
    out.push(
        cwd.join(".allthecodes/output-styles")
            .join(format!("{}.md", safe_name)),
    );
    out.push(
        cwd.join(".cc-rust/output-styles")
            .join(format!("{}.md", safe_name)),
    );
    out.push(
        data_root()
            .join("output-styles")
            .join(format!("{}.md", safe_name)),
    );
    out
}

/// Strip path separators and parent-directory tokens so a malicious
/// settings value can't escape the output-styles directory.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect::<String>()
        .replace("..", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn built_in_names_resolve_without_disk() {
        let cwd = Path::new("/nonexistent");
        assert!(matches!(resolve("default", cwd), OutputStyle::Default));
        assert!(matches!(resolve("Default", cwd), OutputStyle::Default));
        assert!(matches!(resolve("", cwd), OutputStyle::Default));
        assert!(matches!(
            resolve("Explanatory", cwd),
            OutputStyle::Explanatory
        ));
        assert!(matches!(resolve("LEARNING", cwd), OutputStyle::Learning));
    }

    #[test]
    fn unknown_name_falls_back_to_default() {
        let cwd = Path::new("/nonexistent-tmpdir");
        assert_eq!(resolve("nonsense", cwd), OutputStyle::Default);
    }

    #[test]
    fn unknown_name_reports_fallback_diagnostic() {
        let cwd = Path::new("/nonexistent-tmpdir");
        let resolution = resolve_with_diagnostic("nonsense", cwd);

        assert_eq!(resolution.style, OutputStyle::Default);
        let diagnostic = resolution
            .fallback_diagnostic
            .as_deref()
            .expect("fallback diagnostic");
        assert!(diagnostic.contains("nonsense"));
        assert!(diagnostic.contains("using Default"));

        let section = resolution_section(&resolution).expect("diagnostic section");
        assert!(section.starts_with("# Output Style Fallback"));
        assert!(section.contains("nonsense"));
    }

    #[test]
    fn explicit_default_has_no_fallback_diagnostic() {
        let cwd = Path::new("/nonexistent-tmpdir");
        let resolution = resolve_with_diagnostic("Default", cwd);

        assert_eq!(resolution.style, OutputStyle::Default);
        assert!(resolution.fallback_diagnostic.is_none());
        assert!(resolution_section(&resolution).is_none());
    }

    #[test]
    fn default_style_emits_no_section() {
        assert_eq!(style_section(&OutputStyle::Default), None);
    }

    #[test]
    fn explanatory_section_contains_rationale_hint() {
        let s = style_section(&OutputStyle::Explanatory).unwrap();
        assert!(s.starts_with("# Output Style: Explanatory"));
        assert!(s.contains("rationale"));
    }

    #[test]
    fn learning_section_contains_pair_programming_hint() {
        let s = style_section(&OutputStyle::Learning).unwrap();
        assert!(s.starts_with("# Output Style: Learning"));
        assert!(s.contains("pair-programming"));
    }

    #[test]
    fn custom_style_loaded_from_project_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let styles_dir = dir.path().join(".allthecodes/output-styles");
        fs::create_dir_all(&styles_dir).unwrap();
        fs::write(
            styles_dir.join("brevity.md"),
            "Be ruthlessly concise. One sentence per response.",
        )
        .unwrap();

        let style = resolve("brevity", dir.path());
        match style {
            OutputStyle::Custom { name, body } => {
                assert_eq!(name, "brevity");
                assert!(body.contains("ruthlessly concise"));
            }
            other => panic!("expected Custom, got {:?}", other),
        }
    }

    #[test]
    fn custom_style_loaded_from_legacy_project_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let styles_dir = dir.path().join(".cc-rust/output-styles");
        fs::create_dir_all(&styles_dir).unwrap();
        fs::write(styles_dir.join("legacy.md"), "Legacy fallback style.").unwrap();

        let style = resolve("legacy", dir.path());
        match style {
            OutputStyle::Custom { name, body } => {
                assert_eq!(name, "legacy");
                assert_eq!(body, "Legacy fallback style.");
            }
            other => panic!("expected Custom, got {:?}", other),
        }
    }

    #[test]
    fn custom_section_renders_with_header() {
        let style = OutputStyle::Custom {
            name: "brevity".into(),
            body: "Be ruthlessly concise.".into(),
        };
        let section = style_section(&style).unwrap();
        assert!(section.starts_with("# Output Style: brevity"));
        assert!(section.contains("ruthlessly"));
    }

    #[test]
    fn empty_custom_body_emits_no_section() {
        let style = OutputStyle::Custom {
            name: "empty".into(),
            body: "   \n  ".into(),
        };
        assert!(style_section(&style).is_none());
    }

    #[test]
    fn missing_custom_file_falls_back_to_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolution = resolve_with_diagnostic("does-not-exist", dir.path());
        assert_eq!(resolution.style, OutputStyle::Default);
        assert!(resolution
            .fallback_diagnostic
            .as_deref()
            .is_some_and(|diagnostic| diagnostic.contains("does-not-exist")));
    }

    #[test]
    fn sanitize_strips_path_traversal() {
        assert_eq!(sanitize_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_name("foo/bar"), "foobar");
        assert_eq!(sanitize_name("normal"), "normal");
    }
}
