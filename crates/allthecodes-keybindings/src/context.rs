//! Context vocabulary for keybinding resolution.
//!
//! A [`Context`] identifies where in the UI a keystroke was produced.
//! Matches the Claude Code spec list at
//! `docs/claude-code-configuration/customize-keyboard-shortcuts.md#contexts`.
//!
//! Resolution order: specific context first, then fall back to `Global`.

use std::fmt;
use std::str::FromStr;

/// UI context for keybinding resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    /// Applies everywhere.
    Global,
    /// Chat input area.
    Chat,
    /// Autocomplete menu open.
    Autocomplete,
    /// Settings menu.
    Settings,
    /// Confirmation / permission dialog.
    Confirmation,
    /// Tab navigation.
    Tabs,
    /// Help overlay.
    Help,
    /// Transcript viewer.
    Transcript,
    /// History search mode (Ctrl+R).
    HistorySearch,
    /// Background task running.
    Task,
    /// Theme picker dialog.
    ThemePicker,
    /// Image attachment navigation.
    Attachments,
    /// Footer indicator navigation.
    Footer,
    /// Rewind / summarize message selection.
    MessageSelector,
    /// Fullscreen selected-message action cursor.
    MessageActions,
    /// Diff viewer navigation.
    DiffDialog,
    /// Model picker effort level.
    ModelPicker,
    /// Generic select/list components.
    Select,
    /// Plugin dialog.
    Plugin,
    /// Conversation scroll / text selection (fullscreen).
    Scroll,
    /// Chat input while a model turn is running.
    Busy,
    /// `/doctor` diagnostics screen.
    Doctor,
}

impl Context {
    /// Canonical spec name (PascalCase).
    pub fn as_str(self) -> &'static str {
        match self {
            Context::Global => "Global",
            Context::Chat => "Chat",
            Context::Autocomplete => "Autocomplete",
            Context::Settings => "Settings",
            Context::Confirmation => "Confirmation",
            Context::Tabs => "Tabs",
            Context::Help => "Help",
            Context::Transcript => "Transcript",
            Context::HistorySearch => "HistorySearch",
            Context::Task => "Task",
            Context::ThemePicker => "ThemePicker",
            Context::Attachments => "Attachments",
            Context::Footer => "Footer",
            Context::MessageSelector => "MessageSelector",
            Context::MessageActions => "MessageActions",
            Context::DiffDialog => "DiffDialog",
            Context::ModelPicker => "ModelPicker",
            Context::Select => "Select",
            Context::Plugin => "Plugin",
            Context::Scroll => "Scroll",
            Context::Busy => "Busy",
            Context::Doctor => "Doctor",
        }
    }

    pub fn all() -> &'static [Context] {
        &[
            Context::Global,
            Context::Chat,
            Context::Autocomplete,
            Context::Settings,
            Context::Confirmation,
            Context::Tabs,
            Context::Help,
            Context::Transcript,
            Context::HistorySearch,
            Context::Task,
            Context::ThemePicker,
            Context::Attachments,
            Context::Footer,
            Context::MessageSelector,
            Context::MessageActions,
            Context::DiffDialog,
            Context::ModelPicker,
            Context::Select,
            Context::Plugin,
            Context::Scroll,
            Context::Busy,
            Context::Doctor,
        ]
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Context {
    type Err = UnknownContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-insensitive match on the canonical names so users can write
        // `chat` instead of `Chat` without surprise.
        let lower = s.trim().to_ascii_lowercase();
        for ctx in Context::all() {
            if ctx.as_str().eq_ignore_ascii_case(&lower) {
                return Ok(*ctx);
            }
        }
        Err(UnknownContextError(s.to_string()))
    }
}

/// Error produced when a config uses an unknown context name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownContextError(pub String);

impl fmt::Display for UnknownContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown context '{}'. Known contexts: {}",
            self.0,
            Context::all()
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl std::error::Error for UnknownContextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_name() {
        assert_eq!("Chat".parse::<Context>().unwrap(), Context::Chat);
        assert_eq!(
            "Autocomplete".parse::<Context>().unwrap(),
            Context::Autocomplete
        );
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!("chat".parse::<Context>().unwrap(), Context::Chat);
        assert_eq!("CHAT".parse::<Context>().unwrap(), Context::Chat);
    }

    #[test]
    fn parse_unknown() {
        let err = "NotReal".parse::<Context>().unwrap_err();
        assert_eq!(err.0, "NotReal");
        assert!(err.to_string().contains("Known contexts"));
    }

    #[test]
    fn display_round_trip() {
        for ctx in Context::all() {
            assert_eq!(ctx.as_str().parse::<Context>().unwrap(), *ctx);
        }
    }

    #[test]
    fn all_contexts_listed() {
        assert_eq!(Context::all().len(), 22);
    }
}
