//! Keystroke parser — converts config strings into [`Chord`]s.
//!
//! Syntax (matching the Claude Code spec):
//!
//! - Modifiers (ordered or not): `ctrl`, `alt`/`opt`/`option`, `shift`,
//!   `meta`/`cmd`/`command`
//! - Join with `+`: `ctrl+k`, `shift+tab`, `ctrl+shift+c`
//! - Chords: space-separated keystrokes: `ctrl+x ctrl+k`
//! - Uppercase bare letters imply `shift`: `K` == `shift+k`. Uppercase with
//!   modifiers is stylistic (`ctrl+K` == `ctrl+k`).
//! - Special keys: `escape`/`esc`, `enter`/`return`, `tab`, `space`, `up`,
//!   `down`, `left`, `right`, `backspace`, `delete`, `home`, `end`,
//!   `pageup`, `pagedown`, `f1`..`f24`.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Parsed modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Self = Modifiers {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    pub fn from_crossterm(m: KeyModifiers) -> Self {
        Modifiers {
            ctrl: m.contains(KeyModifiers::CONTROL),
            alt: m.contains(KeyModifiers::ALT),
            shift: m.contains(KeyModifiers::SHIFT),
            meta: m.contains(KeyModifiers::META) || m.contains(KeyModifiers::SUPER),
        }
    }

    pub fn is_none(&self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.meta)
    }
}

/// Parsed key identity — `KeyCode` plus a display-friendly owned string for
/// round-tripping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    BackTab,
    Space,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
}

impl Key {
    pub fn from_crossterm(code: KeyCode) -> Option<Self> {
        Some(match code {
            KeyCode::Char(c) => Key::Char(c),
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Escape,
            KeyCode::Tab => Key::Tab,
            KeyCode::BackTab => Key::BackTab,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Delete => Key::Delete,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::Insert => Key::Insert,
            KeyCode::F(n) => Key::F(n),
            _ => return None,
        })
    }

    pub fn display(&self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::Enter => "enter".into(),
            Key::Escape => "escape".into(),
            Key::Tab => "tab".into(),
            Key::BackTab => "backtab".into(),
            Key::Space => "space".into(),
            Key::Backspace => "backspace".into(),
            Key::Delete => "delete".into(),
            Key::Left => "left".into(),
            Key::Right => "right".into(),
            Key::Up => "up".into(),
            Key::Down => "down".into(),
            Key::Home => "home".into(),
            Key::End => "end".into(),
            Key::PageUp => "pageup".into(),
            Key::PageDown => "pagedown".into(),
            Key::Insert => "insert".into(),
            Key::F(n) => format!("f{}", n),
        }
    }
}

/// Single keystroke — modifier bundle + [`Key`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keystroke {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Keystroke {
    pub fn parse(raw: &str) -> Result<Self, KeystrokeError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(KeystrokeError::Empty);
        }
        // Split on '+' but keep the special case of `++` meaning literal '+'.
        // The spec does not document '+' as a bindable character, so we
        // reject it for now to keep the parser simple.
        let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err(KeystrokeError::MalformedToken(trimmed.to_string()));
        }

        let mut mods = Modifiers::NONE;
        let mut final_key: Option<Key> = None;

        for (idx, part) in parts.iter().enumerate() {
            let part_lower = part.to_ascii_lowercase();
            match part_lower.as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "opt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "meta" | "cmd" | "command" => mods.meta = true,
                _ => {
                    if idx != parts.len() - 1 {
                        return Err(KeystrokeError::UnknownModifier(part.to_string()));
                    }
                    final_key = Some(parse_key_name(part)?);
                }
            }
        }

        let mut key = final_key.ok_or_else(|| KeystrokeError::MissingKey(trimmed.to_string()))?;

        // Uppercase bare letter implies shift, per spec: `K == shift+k`.
        // With modifiers, uppercase is stylistic: `ctrl+K == ctrl+k`.
        if let Key::Char(c) = &key {
            if c.is_ascii_uppercase() {
                if mods.is_none() {
                    mods.shift = true;
                }
                key = Key::Char(c.to_ascii_lowercase());
            }
        }

        Ok(Keystroke {
            modifiers: mods,
            key,
        })
    }

    /// Construct from a crossterm `KeyEvent`.
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        let key = Key::from_crossterm(event.code)?;
        let mods = Modifiers::from_crossterm(event.modifiers);
        // Normalize: crossterm gives us the shifted character for capitals
        // with SHIFT present. Lower-case it and keep the shift flag so
        // lookups match the parsed config shape.
        let (final_key, final_mods) = match key {
            Key::Char(c) if c.is_ascii_uppercase() => {
                let mut m = mods;
                m.shift = true;
                (Key::Char(c.to_ascii_lowercase()), m)
            }
            other => (other, mods),
        };
        Some(Keystroke {
            modifiers: final_mods,
            key: final_key,
        })
    }

    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.modifiers.ctrl {
            parts.push("ctrl".into());
        }
        if self.modifiers.meta {
            parts.push("meta".into());
        }
        if self.modifiers.alt {
            parts.push("alt".into());
        }
        if self.modifiers.shift {
            parts.push("shift".into());
        }
        parts.push(self.key.display());
        parts.join("+")
    }
}

impl fmt::Display for Keystroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

/// A chord is a sequence of keystrokes separated by spaces.
///
/// A single-keystroke binding is just a chord with one element.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chord(pub Vec<Keystroke>);

impl Chord {
    pub fn parse(raw: &str) -> Result<Self, KeystrokeError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(KeystrokeError::Empty);
        }
        let mut strokes = Vec::new();
        for piece in trimmed.split_whitespace() {
            strokes.push(Keystroke::parse(piece)?);
        }
        if strokes.is_empty() {
            return Err(KeystrokeError::Empty);
        }
        Ok(Chord(strokes))
    }

    pub fn strokes(&self) -> &[Keystroke] {
        &self.0
    }

    pub fn is_single(&self) -> bool {
        self.0.len() == 1
    }

    pub fn display(&self) -> String {
        self.0
            .iter()
            .map(Keystroke::display)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

fn parse_key_name(raw: &str) -> Result<Key, KeystrokeError> {
    let lower = raw.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "escape" | "esc" => Key::Escape,
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "backtab" => Key::BackTab,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "insert" | "ins" => Key::Insert,
        other => {
            // f1..f24
            if let Some(rest) = other.strip_prefix('f') {
                if let Ok(n) = rest.parse::<u8>() {
                    if (1..=24).contains(&n) {
                        return Ok(Key::F(n));
                    }
                }
            }
            // Single-character literal (preserve the *original* case so we
            // can later detect uppercase → shift).
            let chars: Vec<char> = raw.chars().collect();
            if chars.len() == 1 {
                return Ok(Key::Char(chars[0]));
            }
            return Err(KeystrokeError::UnknownKey(raw.to_string()));
        }
    })
}

/// Errors produced by keystroke parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeystrokeError {
    Empty,
    MalformedToken(String),
    UnknownModifier(String),
    UnknownKey(String),
    MissingKey(String),
}

impl fmt::Display for KeystrokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeystrokeError::Empty => f.write_str("keystroke is empty"),
            KeystrokeError::MalformedToken(s) => write!(f, "malformed token '{}'", s),
            KeystrokeError::UnknownModifier(s) => write!(f, "unknown modifier '{}'", s),
            KeystrokeError::UnknownKey(s) => write!(f, "unknown key '{}'", s),
            KeystrokeError::MissingKey(s) => {
                write!(f, "'{}' has modifiers but no key", s)
            }
        }
    }
}

impl std::error::Error for KeystrokeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_char() {
        let k = Keystroke::parse("a").unwrap();
        assert_eq!(k.modifiers, Modifiers::NONE);
        assert_eq!(k.key, Key::Char('a'));
    }

    #[test]
    fn parse_ctrl_plus_key() {
        let k = Keystroke::parse("ctrl+k").unwrap();
        assert!(k.modifiers.ctrl);
        assert_eq!(k.key, Key::Char('k'));
    }

    #[test]
    fn parse_multi_modifier() {
        let k = Keystroke::parse("ctrl+shift+c").unwrap();
        assert!(k.modifiers.ctrl);
        assert!(k.modifiers.shift);
        assert_eq!(k.key, Key::Char('c'));
    }

    #[test]
    fn modifier_aliases() {
        for alias in ["alt", "opt", "option"] {
            let k = Keystroke::parse(&format!("{}+a", alias)).unwrap();
            assert!(k.modifiers.alt, "{} should map to alt", alias);
        }
        for alias in ["meta", "cmd", "command"] {
            let k = Keystroke::parse(&format!("{}+a", alias)).unwrap();
            assert!(k.modifiers.meta);
        }
    }

    #[test]
    fn special_keys_parse() {
        for (s, expected) in [
            ("enter", Key::Enter),
            ("Escape", Key::Escape),
            ("tab", Key::Tab),
            ("space", Key::Space),
            ("backspace", Key::Backspace),
            ("delete", Key::Delete),
            ("pageup", Key::PageUp),
            ("PageDown", Key::PageDown),
            ("up", Key::Up),
            ("f12", Key::F(12)),
        ] {
            assert_eq!(Keystroke::parse(s).unwrap().key, expected);
        }
    }

    #[test]
    fn uppercase_bare_implies_shift() {
        let k = Keystroke::parse("K").unwrap();
        assert!(k.modifiers.shift);
        assert_eq!(k.key, Key::Char('k'));
    }

    #[test]
    fn uppercase_with_modifier_is_stylistic() {
        let k = Keystroke::parse("ctrl+K").unwrap();
        assert!(k.modifiers.ctrl);
        assert!(!k.modifiers.shift);
        assert_eq!(k.key, Key::Char('k'));
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(Keystroke::parse(""), Err(KeystrokeError::Empty));
        assert!(matches!(
            Keystroke::parse("ctrl+"),
            Err(KeystrokeError::MalformedToken(_))
        ));
    }

    #[test]
    fn chord_parses_multi() {
        let c = Chord::parse("ctrl+x ctrl+k").unwrap();
        assert_eq!(c.strokes().len(), 2);
        assert!(!c.is_single());
    }

    #[test]
    fn chord_single() {
        let c = Chord::parse("ctrl+e").unwrap();
        assert!(c.is_single());
    }

    #[test]
    fn display_round_trip() {
        for s in ["ctrl+k", "ctrl+shift+c", "enter", "f12"] {
            let k = Keystroke::parse(s).unwrap();
            let re = Keystroke::parse(&k.display()).unwrap();
            assert_eq!(k, re, "{} round-trip failed", s);
        }
    }

    #[test]
    fn from_event_normalizes_uppercase() {
        let ev = KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT);
        let k = Keystroke::from_event(&ev).unwrap();
        assert!(k.modifiers.shift);
        assert_eq!(k.key, Key::Char('k'));
    }

    #[test]
    fn invalid_modifier_errors() {
        assert!(matches!(
            Keystroke::parse("bogus+k"),
            Err(KeystrokeError::UnknownModifier(_))
        ));
    }
}
