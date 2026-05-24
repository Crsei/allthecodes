//! On-disk [`keybindings.json`] shape + parser.
//!
//! Matches the Claude Code spec (`customize-keyboard-shortcuts.md`):
//!
//! ```json
//! {
//!   "$schema": "https://www.schemastore.org/claude-code-keybindings.json",
//!   "$docs": "https://code.claude.com/docs/en/keybindings",
//!   "bindings": [
//!     {
//!       "context": "Chat",
//!       "bindings": {
//!         "ctrl+e": "chat:externalEditor",
//!         "ctrl+u": null
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! A `null` value unbinds the default for that chord in the given context.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::action::{Action, ActionParseError};
use super::context::{Context, UnknownContextError};
use super::keystroke::{Chord, KeystrokeError};

/// Raw JSON shape — closely mirrors the disk format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawKeybindingsFile {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(rename = "$docs", default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default)]
    pub bindings: Vec<RawBindingBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawBindingBlock {
    pub context: String,
    #[serde(default)]
    pub bindings: HashMap<String, Option<String>>,
}

/// A parsed binding entry, typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingValue {
    /// Bind this chord to an action.
    Bind(Action),
    /// Unbind (remove) whatever default lives at this chord.
    Unbind,
}

/// Fully-parsed user config — context → (chord → bind/unbind).
#[derive(Debug, Clone, Default)]
pub struct UserBindings {
    pub schema: Option<String>,
    pub docs: Option<String>,
    pub per_context: HashMap<Context, Vec<(Chord, BindingValue)>>,
}

impl UserBindings {
    /// Parse from raw JSON text.
    ///
    /// Collects **every** parse error (up to `MAX_ERRORS`) instead of
    /// stopping at the first so the `/keybindings` command can surface a
    /// useful diagnostic block when the user makes multiple typos.
    pub fn parse_json(text: &str) -> Result<Self, KeybindingsConfigError> {
        let raw: RawKeybindingsFile = serde_json::from_str(text)
            .map_err(|e| KeybindingsConfigError::InvalidJson(e.to_string()))?;
        Self::from_raw(raw)
    }

    pub fn from_raw(raw: RawKeybindingsFile) -> Result<Self, KeybindingsConfigError> {
        let mut out = UserBindings {
            schema: raw.schema,
            docs: raw.docs,
            per_context: HashMap::new(),
        };
        let mut errors = Vec::new();
        for block in raw.bindings {
            let ctx = match block.context.parse::<Context>() {
                Ok(c) => c,
                Err(UnknownContextError(bad)) => {
                    errors.push(KeybindingsConfigIssue::UnknownContext(bad));
                    continue;
                }
            };
            let entry = out.per_context.entry(ctx).or_default();
            for (chord_raw, value) in block.bindings {
                let chord = match Chord::parse(&chord_raw) {
                    Ok(c) => c,
                    Err(e) => {
                        errors.push(KeybindingsConfigIssue::BadChord {
                            context: ctx,
                            chord: chord_raw,
                            error: e,
                        });
                        continue;
                    }
                };
                match value {
                    None => entry.push((chord, BindingValue::Unbind)),
                    Some(action_raw) => match Action::parse(&action_raw) {
                        Ok(a) => entry.push((chord, BindingValue::Bind(a))),
                        Err(e) => {
                            errors.push(KeybindingsConfigIssue::BadAction {
                                context: ctx,
                                chord: chord_raw,
                                error: e,
                            });
                        }
                    },
                }
            }
        }
        if !errors.is_empty() {
            return Err(KeybindingsConfigError::Issues(errors));
        }
        Ok(out)
    }
}

/// Top-level error type for config parsing.
#[derive(Debug, Clone)]
pub enum KeybindingsConfigError {
    /// File is not valid JSON.
    InvalidJson(String),
    /// File is valid JSON but contains structural or semantic issues.
    Issues(Vec<KeybindingsConfigIssue>),
}

impl fmt::Display for KeybindingsConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeybindingsConfigError::InvalidJson(s) => {
                write!(f, "keybindings.json is not valid JSON: {}", s)
            }
            KeybindingsConfigError::Issues(v) => {
                writeln!(f, "keybindings.json has {} issue(s):", v.len())?;
                for issue in v {
                    writeln!(f, "  - {}", issue)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for KeybindingsConfigError {}

/// One structural/semantic issue in a keybindings file.
#[derive(Debug, Clone)]
pub enum KeybindingsConfigIssue {
    UnknownContext(String),
    BadChord {
        context: Context,
        chord: String,
        error: KeystrokeError,
    },
    BadAction {
        context: Context,
        chord: String,
        error: ActionParseError,
    },
}

impl fmt::Display for KeybindingsConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeybindingsConfigIssue::UnknownContext(c) => {
                write!(f, "unknown context '{}'", c)
            }
            KeybindingsConfigIssue::BadChord {
                context,
                chord,
                error,
            } => write!(f, "[{}] chord '{}': {}", context.as_str(), chord, error),
            KeybindingsConfigIssue::BadAction {
                context,
                chord,
                error,
            } => write!(
                f,
                "[{}] chord '{}' has invalid action: {}",
                context.as_str(),
                chord,
                error
            ),
        }
    }
}

/// The canonical empty template used by `/keybindings` when the file doesn't
/// exist yet.
pub const EMPTY_TEMPLATE: &str = r#"{
  "$schema": "https://cc-rust/keybindings.schema.json",
  "$docs": "https://code.claude.com/docs/en/keybindings",
  "bindings": []
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_template() {
        let b = UserBindings::parse_json(EMPTY_TEMPLATE).unwrap();
        assert!(b.per_context.is_empty());
    }

    #[test]
    fn parse_simple_bindings() {
        let text = r#"{
            "bindings": [
                {
                    "context": "Chat",
                    "bindings": {
                        "ctrl+e": "chat:externalEditor",
                        "ctrl+u": null
                    }
                }
            ]
        }"#;
        let b = UserBindings::parse_json(text).unwrap();
        let chat = b.per_context.get(&Context::Chat).unwrap();
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn parse_rejects_unknown_context() {
        let text = r#"{
            "bindings": [
                { "context": "NotReal", "bindings": {"ctrl+e": "app:interrupt"} }
            ]
        }"#;
        let err = UserBindings::parse_json(text).unwrap_err();
        match err {
            KeybindingsConfigError::Issues(issues) => {
                assert_eq!(issues.len(), 1);
                assert!(matches!(
                    issues[0],
                    KeybindingsConfigIssue::UnknownContext(_)
                ));
            }
            _ => panic!("expected Issues"),
        }
    }

    #[test]
    fn parse_rejects_bad_chord() {
        let text = r#"{
            "bindings": [
                { "context": "Chat", "bindings": {"ctrl+": "chat:submit"} }
            ]
        }"#;
        let err = UserBindings::parse_json(text).unwrap_err();
        match err {
            KeybindingsConfigError::Issues(issues) => {
                assert!(matches!(
                    &issues[0],
                    KeybindingsConfigIssue::BadChord { .. }
                ));
            }
            _ => panic!("expected Issues"),
        }
    }

    #[test]
    fn parse_rejects_bad_action() {
        let text = r#"{
            "bindings": [
                { "context": "Chat", "bindings": {"ctrl+e": "bogus"} }
            ]
        }"#;
        let err = UserBindings::parse_json(text).unwrap_err();
        match err {
            KeybindingsConfigError::Issues(issues) => {
                assert!(matches!(
                    &issues[0],
                    KeybindingsConfigIssue::BadAction { .. }
                ));
            }
            _ => panic!("expected Issues"),
        }
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let err = UserBindings::parse_json("{ not json").unwrap_err();
        assert!(matches!(err, KeybindingsConfigError::InvalidJson(_)));
    }

    #[test]
    fn collects_multiple_issues() {
        let text = r#"{
            "bindings": [
                {
                    "context": "Chat",
                    "bindings": {
                        "ctrl+e": "bogus",
                        "invalid+!": "chat:submit"
                    }
                },
                { "context": "NotReal", "bindings": {} }
            ]
        }"#;
        let err = UserBindings::parse_json(text).unwrap_err();
        match err {
            KeybindingsConfigError::Issues(issues) => {
                assert!(
                    issues.len() >= 2,
                    "expected at least 2 issues, got {}",
                    issues.len()
                );
            }
            _ => panic!("expected Issues"),
        }
    }
}
