//! Action vocabulary for the keybinding system.
//!
//! Actions are namespaced strings in `namespace:action` format, matching the
//! Claude Code spec (`docs/claude-code-configuration/customize-keyboard-shortcuts.md`).
//!
//! Both the Rust TUI and the OpenTUI frontend consume this vocabulary — the
//! vocabulary is the unification point. Each layer maps a resolved action
//! string to its own handler without disagreeing about names.

use std::fmt;

/// Canonical action name (e.g. `"app:interrupt"`, `"chat:submit"`).
///
/// Wraps a `String` so plugins can register custom actions at runtime
/// without changing an enum, while still benefitting from `Eq`/`Hash` for
/// registry lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Action(String);

impl Action {
    /// Parse an action from a raw `namespace:action` string.
    ///
    /// Accepts any non-empty string that contains exactly one `:`; multiple
    /// colons or empty sides are rejected so typos surface early.
    pub fn parse(raw: &str) -> Result<Self, ActionParseError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ActionParseError::Empty);
        }
        let colon_count = trimmed.bytes().filter(|b| *b == b':').count();
        if colon_count != 1 {
            return Err(ActionParseError::InvalidFormat(trimmed.to_string()));
        }
        let Some((ns, name)) = trimmed.split_once(':') else {
            return Err(ActionParseError::InvalidFormat(trimmed.to_string()));
        };
        if ns.is_empty() || name.is_empty() {
            return Err(ActionParseError::InvalidFormat(trimmed.to_string()));
        }
        Ok(Action(format!("{}:{}", ns, name)))
    }

    /// Runtime helper for the built-in default set. Debug-assertions catch
    /// malformed literals in tests.
    pub fn new_static(s: &'static str) -> Self {
        debug_assert!(
            Action::parse(s).is_ok(),
            "invalid action literal '{}' — must be 'namespace:action'",
            s
        );
        Action(s.to_string())
    }

    pub fn namespace(&self) -> &str {
        self.0.split_once(':').map(|(ns, _)| ns).unwrap_or("")
    }

    pub fn name(&self) -> &str {
        self.0.split_once(':').map(|(_, n)| n).unwrap_or("")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Error produced by [`Action::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionParseError {
    Empty,
    InvalidFormat(String),
}

impl fmt::Display for ActionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionParseError::Empty => f.write_str("action is empty"),
            ActionParseError::InvalidFormat(s) => write!(
                f,
                "'{}' is not a valid action — expected 'namespace:action'",
                s
            ),
        }
    }
}

impl std::error::Error for ActionParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_action() {
        let a = Action::parse("chat:submit").unwrap();
        assert_eq!(a.namespace(), "chat");
        assert_eq!(a.name(), "submit");
        assert_eq!(a.as_str(), "chat:submit");
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(Action::parse(""), Err(ActionParseError::Empty));
        assert_eq!(Action::parse("   "), Err(ActionParseError::Empty));
    }

    #[test]
    fn parse_rejects_no_colon() {
        assert!(matches!(
            Action::parse("submit"),
            Err(ActionParseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_rejects_multiple_colons() {
        assert!(matches!(
            Action::parse("chat:submit:extra"),
            Err(ActionParseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn parse_rejects_missing_side() {
        assert!(matches!(
            Action::parse(":submit"),
            Err(ActionParseError::InvalidFormat(_))
        ));
        assert!(matches!(
            Action::parse("chat:"),
            Err(ActionParseError::InvalidFormat(_))
        ));
    }

    #[test]
    fn display_matches_as_str() {
        let a = Action::new_static("app:interrupt");
        assert_eq!(format!("{}", a), "app:interrupt");
    }

    #[test]
    fn equality_is_case_sensitive() {
        assert_ne!(
            Action::new_static("chat:submit"),
            Action::new_static("Chat:Submit")
        );
    }
}
