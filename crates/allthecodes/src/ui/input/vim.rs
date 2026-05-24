//! Vim mode — modal editing for the input field.
//!
//! Provides Normal, Insert, and Visual modes with standard vim keybindings:
//! - Normal: hjkl navigation, dd/yy/p, w/b word motion, x delete, etc.
//! - Insert: standard typing, Esc to return to Normal
//! - Visual: character selection with y/d/x operations
//!
//! Corresponds to TypeScript: vim/ (5 files)

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// ---------------------------------------------------------------------------
// Vim mode state
// ---------------------------------------------------------------------------

/// The current vim editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    /// Normal mode — navigation and commands.
    Normal,
    /// Insert mode — direct text entry.
    Insert,
    /// Visual mode — character selection.
    Visual,
}

impl VimMode {
    /// Mode indicator character for the status line.
    pub fn indicator(&self) -> &'static str {
        match self {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::Visual => "VISUAL",
        }
    }

    /// Short single-char indicator.
    pub fn short_indicator(&self) -> char {
        match self {
            VimMode::Normal => 'N',
            VimMode::Insert => 'I',
            VimMode::Visual => 'V',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorModeSetting {
    Normal,
    Vim,
}

impl EditorModeSetting {
    pub fn parse(editor_mode: Option<&str>) -> Self {
        match editor_mode.map(str::trim) {
            Some("vim") => EditorModeSetting::Vim,
            _ => EditorModeSetting::Normal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EditorModeSetting::Normal => "normal",
            EditorModeSetting::Vim => "vim",
        }
    }
}

/// Result of processing a vim key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimAction {
    /// No-op; key was consumed but no visible action.
    None,
    /// Insert a character at cursor.
    InsertChar(char),
    /// Delete character(s).
    Delete { start: usize, end: usize },
    /// Move cursor to position.
    MoveCursor(usize),
    /// Yank (copy) text range into register.
    Yank { start: usize, end: usize },
    /// Paste register contents at cursor.
    Paste(String),
    /// Delete entire line (dd).
    DeleteLine,
    /// Yank entire line (yy).
    YankLine,
    /// Submit the input (Enter in normal mode or mapped command).
    Submit,
    /// Switch to a different mode.
    SwitchMode(VimMode),
    /// Undo last change.
    Undo,
    /// Pass-through: this key should be handled by the normal input handler.
    Passthrough(KeyEvent),
}

// ---------------------------------------------------------------------------
// Vim state machine
// ---------------------------------------------------------------------------

/// Full vim state for an input field.
pub struct VimState {
    /// Current mode.
    pub mode: VimMode,
    /// Whether vim mode is enabled.
    pub enabled: bool,
    /// Pending operator (for compound commands like `dw`, `cw`).
    pending_op: Option<PendingOp>,
    /// Numeric prefix for repetition (e.g. `3w` = move 3 words).
    repeat_count: Option<usize>,
    /// Register (clipboard) contents.
    register: String,
    /// Visual mode anchor position (byte offset).
    visual_anchor: usize,
}

/// A pending operator waiting for a motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingOp {
    Delete, // d
    Yank,   // y
    Change, // c
}

impl VimState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            enabled: false,
            pending_op: None,
            repeat_count: None,
            register: String::new(),
            visual_anchor: 0,
        }
    }

    /// Build a [`VimState`] honouring the `editorMode` setting.
    ///
    /// - `Some("vim")` → enabled, starting in Normal (matches `/config`
    ///   editor mode = vim)
    /// - `Some("normal")` / `None` → disabled (plain editing)
    /// - Any other value → disabled (validation warns at config-load)
    pub fn from_editor_mode(editor_mode: Option<&str>) -> Self {
        let mut state = Self::new();
        state.apply_editor_mode(editor_mode);
        state
    }

    pub fn apply_editor_mode(&mut self, editor_mode: Option<&str>) {
        match EditorModeSetting::parse(editor_mode) {
            EditorModeSetting::Normal => self.disable(),
            EditorModeSetting::Vim => self.enable(),
        }
    }

    pub fn editor_mode_setting(&self) -> EditorModeSetting {
        if self.enabled {
            EditorModeSetting::Vim
        } else {
            EditorModeSetting::Normal
        }
    }

    /// Enable vim mode, starting in Normal.
    pub fn enable(&mut self) {
        self.enabled = true;
        self.mode = VimMode::Normal;
        self.pending_op = None;
        self.repeat_count = None;
    }

    /// Disable vim mode.
    pub fn disable(&mut self) {
        self.enabled = false;
        self.mode = VimMode::Insert; // fallback to normal editing
        self.pending_op = None;
        self.repeat_count = None;
    }

    /// Toggle vim mode on/off.
    pub fn toggle(&mut self) {
        if self.enabled {
            self.disable();
        } else {
            self.enable();
        }
    }

    /// Get the effective repeat count (default 1).
    fn count(&mut self) -> usize {
        self.repeat_count.take().unwrap_or(1)
    }

    /// Process a key event and return the action(s) to take.
    ///
    /// `input` is the current input text, `cursor` is the byte-offset cursor.
    pub fn handle_key(&mut self, key: KeyEvent, input: &str, cursor: usize) -> VimAction {
        if !self.enabled {
            return VimAction::Passthrough(key);
        }

        match self.mode {
            VimMode::Normal => self.handle_normal(key, input, cursor),
            VimMode::Insert => self.handle_insert(key),
            VimMode::Visual => self.handle_visual(key, input, cursor),
        }
    }

    // ── Normal mode ────────────────────────────────────────────────

    fn handle_normal(&mut self, key: KeyEvent, input: &str, cursor: usize) -> VimAction {
        // Handle Ctrl combinations first
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return VimAction::Passthrough(key);
        }

        match key.code {
            // -- Digit prefix --
            KeyCode::Char(c @ '1'..='9') => {
                let digit = (c as usize) - ('0' as usize);
                self.repeat_count = Some(self.repeat_count.unwrap_or(0) * 10 + digit);
                VimAction::None
            }
            KeyCode::Char('0') if self.repeat_count.is_some() => {
                self.repeat_count = Some(self.repeat_count.unwrap_or(0) * 10);
                VimAction::None
            }

            // -- Mode switching --
            KeyCode::Char('i') => {
                self.mode = VimMode::Insert;
                self.pending_op = None;
                self.repeat_count = None;
                VimAction::SwitchMode(VimMode::Insert)
            }
            KeyCode::Char('a') => {
                self.mode = VimMode::Insert;
                self.pending_op = None;
                self.repeat_count = None;
                // Move cursor one right then enter insert
                let new_pos = next_char_pos(input, cursor);
                if new_pos != cursor {
                    return VimAction::MoveCursor(new_pos);
                }
                VimAction::SwitchMode(VimMode::Insert)
            }
            KeyCode::Char('I') => {
                self.mode = VimMode::Insert;
                VimAction::MoveCursor(0)
            }
            KeyCode::Char('A') => {
                self.mode = VimMode::Insert;
                VimAction::MoveCursor(input.len())
            }
            KeyCode::Char('v') => {
                self.mode = VimMode::Visual;
                self.visual_anchor = cursor;
                VimAction::SwitchMode(VimMode::Visual)
            }

            // -- Navigation --
            KeyCode::Char('h') | KeyCode::Left => {
                let count = self.count();
                let mut pos = cursor;
                for _ in 0..count {
                    pos = prev_char_pos(input, pos);
                }
                VimAction::MoveCursor(pos)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let count = self.count();
                let mut pos = cursor;
                for _ in 0..count {
                    pos = next_char_pos(input, pos);
                }
                VimAction::MoveCursor(pos)
            }
            KeyCode::Char('0') => VimAction::MoveCursor(0),
            KeyCode::Char('$') => VimAction::MoveCursor(input.len()),
            KeyCode::Char('^') => {
                // First non-whitespace
                let pos = input
                    .char_indices()
                    .find(|(_, c)| !c.is_whitespace())
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                VimAction::MoveCursor(pos)
            }

            // -- Word motions --
            KeyCode::Char('w') => {
                if let Some(PendingOp::Delete) = self.pending_op {
                    self.pending_op = None;
                    let end = word_end_pos(input, cursor);
                    VimAction::Delete { start: cursor, end }
                } else if let Some(PendingOp::Yank) = self.pending_op {
                    self.pending_op = None;
                    let end = word_end_pos(input, cursor);
                    VimAction::Yank { start: cursor, end }
                } else if let Some(PendingOp::Change) = self.pending_op {
                    self.pending_op = None;
                    self.mode = VimMode::Insert;
                    let end = word_end_pos(input, cursor);
                    VimAction::Delete { start: cursor, end }
                } else {
                    let count = self.count();
                    let mut pos = cursor;
                    for _ in 0..count {
                        pos = word_end_pos(input, pos);
                    }
                    VimAction::MoveCursor(pos)
                }
            }
            KeyCode::Char('b') => {
                let count = self.count();
                let mut pos = cursor;
                for _ in 0..count {
                    pos = word_start_pos(input, pos);
                }
                VimAction::MoveCursor(pos)
            }
            KeyCode::Char('e') => {
                let count = self.count();
                let mut pos = cursor;
                for _ in 0..count {
                    pos = word_end_inclusive_pos(input, pos);
                }
                VimAction::MoveCursor(pos)
            }

            // -- Operators --
            KeyCode::Char('d') => {
                if self.pending_op == Some(PendingOp::Delete) {
                    // dd = delete line
                    self.pending_op = None;
                    VimAction::DeleteLine
                } else {
                    self.pending_op = Some(PendingOp::Delete);
                    VimAction::None
                }
            }
            KeyCode::Char('y') => {
                if self.pending_op == Some(PendingOp::Yank) {
                    // yy = yank line
                    self.pending_op = None;
                    self.register = input.to_string();
                    VimAction::YankLine
                } else {
                    self.pending_op = Some(PendingOp::Yank);
                    VimAction::None
                }
            }
            KeyCode::Char('c') => {
                if self.pending_op == Some(PendingOp::Change) {
                    // cc = change entire line
                    self.pending_op = None;
                    self.mode = VimMode::Insert;
                    VimAction::DeleteLine
                } else {
                    self.pending_op = Some(PendingOp::Change);
                    VimAction::None
                }
            }

            // -- Single-key operations --
            KeyCode::Char('x') => {
                let end = next_char_pos(input, cursor);
                if end > cursor {
                    self.register = input[cursor..end].to_string();
                    VimAction::Delete { start: cursor, end }
                } else {
                    VimAction::None
                }
            }
            KeyCode::Char('X') => {
                let start = prev_char_pos(input, cursor);
                if start < cursor {
                    self.register = input[start..cursor].to_string();
                    VimAction::Delete { start, end: cursor }
                } else {
                    VimAction::None
                }
            }
            KeyCode::Char('p') => {
                if !self.register.is_empty() {
                    VimAction::Paste(self.register.clone())
                } else {
                    VimAction::None
                }
            }
            KeyCode::Char('u') => VimAction::Undo,
            KeyCode::Char('D') => {
                // Delete to end of line
                if cursor < input.len() {
                    self.register = input[cursor..].to_string();
                    VimAction::Delete {
                        start: cursor,
                        end: input.len(),
                    }
                } else {
                    VimAction::None
                }
            }
            KeyCode::Char('C') => {
                // Change to end of line
                self.mode = VimMode::Insert;
                if cursor < input.len() {
                    VimAction::Delete {
                        start: cursor,
                        end: input.len(),
                    }
                } else {
                    VimAction::SwitchMode(VimMode::Insert)
                }
            }

            // -- Submit --
            KeyCode::Enter => VimAction::Submit,

            // -- Escape resets pending --
            KeyCode::Esc => {
                self.pending_op = None;
                self.repeat_count = None;
                VimAction::None
            }

            _ => VimAction::None,
        }
    }

    // ── Insert mode ────────────────────────────────────────────────

    fn handle_insert(&mut self, key: KeyEvent) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                VimAction::SwitchMode(VimMode::Normal)
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                VimAction::InsertChar(c)
            }
            // Pass everything else through to normal input handling
            _ => VimAction::Passthrough(key),
        }
    }

    // ── Visual mode ────────────────────────────────────────────────

    fn handle_visual(&mut self, key: KeyEvent, input: &str, cursor: usize) -> VimAction {
        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                VimAction::SwitchMode(VimMode::Normal)
            }
            // Navigation extends selection
            KeyCode::Char('h') | KeyCode::Left => {
                VimAction::MoveCursor(prev_char_pos(input, cursor))
            }
            KeyCode::Char('l') | KeyCode::Right => {
                VimAction::MoveCursor(next_char_pos(input, cursor))
            }
            KeyCode::Char('w') => VimAction::MoveCursor(word_end_pos(input, cursor)),
            KeyCode::Char('b') => VimAction::MoveCursor(word_start_pos(input, cursor)),
            KeyCode::Char('0') => VimAction::MoveCursor(0),
            KeyCode::Char('$') => VimAction::MoveCursor(input.len()),

            // Operations on selection
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.mode = VimMode::Normal;
                let (start, end) = self.selection_range(cursor);
                self.register = input.get(start..end).unwrap_or("").to_string();
                VimAction::Delete { start, end }
            }
            KeyCode::Char('y') => {
                self.mode = VimMode::Normal;
                let (start, end) = self.selection_range(cursor);
                self.register = input.get(start..end).unwrap_or("").to_string();
                VimAction::Yank { start, end }
            }
            KeyCode::Char('c') => {
                self.mode = VimMode::Insert;
                let (start, end) = self.selection_range(cursor);
                VimAction::Delete { start, end }
            }
            _ => VimAction::None,
        }
    }

    /// Get the (start, end) byte range of the visual selection.
    fn selection_range(&self, cursor: usize) -> (usize, usize) {
        if cursor < self.visual_anchor {
            (cursor, self.visual_anchor)
        } else {
            (self.visual_anchor, cursor)
        }
    }
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Text navigation helpers
// ---------------------------------------------------------------------------

/// Move to previous character boundary.
fn prev_char_pos(input: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut p = pos - 1;
    while p > 0 && !input.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Move to next character boundary.
fn next_char_pos(input: &str, pos: usize) -> usize {
    if pos >= input.len() {
        return input.len();
    }
    let mut p = pos + 1;
    while p < input.len() && !input.is_char_boundary(p) {
        p += 1;
    }
    p
}

/// Move forward to the start of the next word.
fn word_end_pos(input: &str, pos: usize) -> usize {
    let bytes = input.as_bytes();
    let len = bytes.len();
    if pos >= len {
        return len;
    }

    let mut p = pos;

    // Skip current word characters
    while p < len && !bytes[p].is_ascii_whitespace() {
        p += 1;
    }
    // Skip whitespace
    while p < len && bytes[p].is_ascii_whitespace() {
        p += 1;
    }

    p
}

/// Move backward to the start of the current/previous word.
fn word_start_pos(input: &str, pos: usize) -> usize {
    let bytes = input.as_bytes();
    if pos == 0 {
        return 0;
    }

    let mut p = pos;

    // Skip whitespace backwards
    while p > 0 && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }
    // Skip word characters backwards
    while p > 0 && !bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }

    p
}

/// Move to end of current word (inclusive, for `e` motion).
fn word_end_inclusive_pos(input: &str, pos: usize) -> usize {
    let bytes = input.as_bytes();
    let len = bytes.len();
    if pos >= len {
        return len;
    }

    let mut p = pos;

    // If on whitespace, skip to next word first
    if bytes[p].is_ascii_whitespace() {
        while p < len && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
    } else {
        // Move past current char
        p += 1;
    }

    // Move to end of word
    while p < len && !bytes[p].is_ascii_whitespace() {
        p += 1;
    }

    // Back up one to be inclusive (on the last char of the word)
    if p > pos + 1 {
        p -= 1;
    }

    p
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn from_editor_mode_normal_is_disabled() {
        let v = VimState::from_editor_mode(Some("normal"));
        assert!(!v.enabled);
    }

    #[test]
    fn from_editor_mode_vim_is_enabled() {
        let v = VimState::from_editor_mode(Some("vim"));
        assert!(v.enabled);
        assert_eq!(v.mode, VimMode::Normal);
    }

    #[test]
    fn from_editor_mode_unset_is_disabled() {
        let v = VimState::from_editor_mode(None);
        assert!(!v.enabled);
    }

    #[test]
    fn from_editor_mode_unknown_is_disabled() {
        let v = VimState::from_editor_mode(Some("emacs"));
        assert!(!v.enabled);
    }

    #[test]
    fn editor_mode_setting_round_trips() {
        let mut v = VimState::new();
        assert_eq!(v.editor_mode_setting(), EditorModeSetting::Normal);
        assert_eq!(
            EditorModeSetting::parse(Some("vim")),
            EditorModeSetting::Vim
        );
        assert_eq!(EditorModeSetting::parse(Some("normal")).as_str(), "normal");

        v.apply_editor_mode(Some("vim"));
        assert!(v.enabled);
        assert_eq!(v.editor_mode_setting(), EditorModeSetting::Vim);

        v.apply_editor_mode(Some("normal"));
        assert!(!v.enabled);
        assert_eq!(v.editor_mode_setting(), EditorModeSetting::Normal);
    }

    #[test]
    fn test_vim_mode_indicators() {
        assert_eq!(VimMode::Normal.indicator(), "NORMAL");
        assert_eq!(VimMode::Insert.indicator(), "INSERT");
        assert_eq!(VimMode::Visual.indicator(), "VISUAL");
        assert_eq!(VimMode::Normal.short_indicator(), 'N');
    }

    #[test]
    fn test_enable_disable() {
        let mut vim = VimState::new();
        assert!(!vim.enabled);

        vim.enable();
        assert!(vim.enabled);
        assert_eq!(vim.mode, VimMode::Normal);

        vim.disable();
        assert!(!vim.enabled);
    }

    #[test]
    fn test_toggle() {
        let mut vim = VimState::new();
        vim.toggle();
        assert!(vim.enabled);
        vim.toggle();
        assert!(!vim.enabled);
    }

    #[test]
    fn test_passthrough_when_disabled() {
        let mut vim = VimState::new();
        let action = vim.handle_key(char_key('a'), "hello", 0);
        assert!(matches!(action, VimAction::Passthrough(_)));
    }

    #[test]
    fn test_normal_mode_h_l_movement() {
        let mut vim = VimState::new();
        vim.enable();

        // 'l' moves right
        let action = vim.handle_key(char_key('l'), "hello", 0);
        assert_eq!(action, VimAction::MoveCursor(1));

        // 'h' moves left
        let action = vim.handle_key(char_key('h'), "hello", 3);
        assert_eq!(action, VimAction::MoveCursor(2));

        // 'h' at start stays at 0
        let action = vim.handle_key(char_key('h'), "hello", 0);
        assert_eq!(action, VimAction::MoveCursor(0));
    }

    #[test]
    fn test_normal_mode_0_dollar() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('0'), "hello world", 5);
        assert_eq!(action, VimAction::MoveCursor(0));

        let action = vim.handle_key(char_key('$'), "hello world", 5);
        assert_eq!(action, VimAction::MoveCursor(11));
    }

    #[test]
    fn test_insert_mode_switch() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('i'), "hello", 3);
        assert_eq!(action, VimAction::SwitchMode(VimMode::Insert));
        assert_eq!(vim.mode, VimMode::Insert);

        // Esc returns to normal
        let action = vim.handle_key(key(KeyCode::Esc), "hello", 3);
        assert_eq!(action, VimAction::SwitchMode(VimMode::Normal));
        assert_eq!(vim.mode, VimMode::Normal);
    }

    #[test]
    fn test_insert_mode_passthrough() {
        let mut vim = VimState::new();
        vim.enable();
        vim.mode = VimMode::Insert;

        let action = vim.handle_key(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            "hello",
            5,
        );
        assert!(matches!(action, VimAction::Passthrough(_)));
    }

    #[test]
    fn test_insert_mode_char_inserts() {
        let mut vim = VimState::new();
        vim.enable();
        vim.mode = VimMode::Insert;

        let action = vim.handle_key(char_key('a'), "hello", 5);
        assert_eq!(action, VimAction::InsertChar('a'));
    }

    #[test]
    fn test_dd_deletes_line() {
        let mut vim = VimState::new();
        vim.enable();

        vim.handle_key(char_key('d'), "hello", 0);
        let action = vim.handle_key(char_key('d'), "hello", 0);
        assert_eq!(action, VimAction::DeleteLine);
    }

    #[test]
    fn test_yy_yanks_line() {
        let mut vim = VimState::new();
        vim.enable();

        vim.handle_key(char_key('y'), "hello world", 0);
        let action = vim.handle_key(char_key('y'), "hello world", 0);
        assert_eq!(action, VimAction::YankLine);
        assert_eq!(vim.register, "hello world");
    }

    #[test]
    fn test_x_deletes_char() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('x'), "hello", 1);
        assert_eq!(action, VimAction::Delete { start: 1, end: 2 });
        assert_eq!(vim.register, "e");
    }

    #[test]
    fn test_p_paste() {
        let mut vim = VimState::new();
        vim.enable();
        vim.register = "world".to_string();

        let action = vim.handle_key(char_key('p'), "hello ", 6);
        assert_eq!(action, VimAction::Paste("world".to_string()));
    }

    #[test]
    fn test_w_word_motion() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('w'), "hello world", 0);
        assert_eq!(action, VimAction::MoveCursor(6));
    }

    #[test]
    fn test_b_word_back() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('b'), "hello world", 8);
        assert_eq!(action, VimAction::MoveCursor(6));
    }

    #[test]
    fn test_repeat_count() {
        let mut vim = VimState::new();
        vim.enable();

        // 3l = move right 3 times
        vim.handle_key(char_key('3'), "hello world", 0);
        let action = vim.handle_key(char_key('l'), "hello world", 0);
        assert_eq!(action, VimAction::MoveCursor(3));
    }

    #[test]
    fn test_visual_mode_select_and_delete() {
        let mut vim = VimState::new();
        vim.enable();

        // Enter visual mode
        vim.handle_key(char_key('v'), "hello", 1);
        assert_eq!(vim.mode, VimMode::Visual);

        // Move right
        vim.handle_key(char_key('l'), "hello", 2);

        // Delete selection
        let action = vim.handle_key(char_key('d'), "hello", 3);
        assert_eq!(action, VimAction::Delete { start: 1, end: 3 });
        assert_eq!(vim.mode, VimMode::Normal);
    }

    #[test]
    fn test_visual_mode_escape() {
        let mut vim = VimState::new();
        vim.enable();

        vim.handle_key(char_key('v'), "hello", 0);
        assert_eq!(vim.mode, VimMode::Visual);

        let action = vim.handle_key(key(KeyCode::Esc), "hello", 2);
        assert_eq!(action, VimAction::SwitchMode(VimMode::Normal));
        assert_eq!(vim.mode, VimMode::Normal);
    }

    #[test]
    fn test_enter_submits() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(key(KeyCode::Enter), "hello", 5);
        assert_eq!(action, VimAction::Submit);
    }

    #[test]
    fn test_capital_i_goes_to_start_insert() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('I'), "hello", 3);
        assert_eq!(action, VimAction::MoveCursor(0));
        assert_eq!(vim.mode, VimMode::Insert);
    }

    #[test]
    fn test_capital_a_goes_to_end_insert() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('A'), "hello", 2);
        assert_eq!(action, VimAction::MoveCursor(5));
        assert_eq!(vim.mode, VimMode::Insert);
    }

    #[test]
    fn test_capital_d_delete_to_end() {
        let mut vim = VimState::new();
        vim.enable();

        let action = vim.handle_key(char_key('D'), "hello world", 5);
        assert_eq!(action, VimAction::Delete { start: 5, end: 11 });
    }

    // -- Word motion helpers --

    #[test]
    fn test_word_end_pos() {
        assert_eq!(word_end_pos("hello world", 0), 6);
        assert_eq!(word_end_pos("hello world", 6), 11);
        assert_eq!(word_end_pos("hello", 0), 5);
    }

    #[test]
    fn test_word_start_pos() {
        assert_eq!(word_start_pos("hello world", 8), 6);
        assert_eq!(word_start_pos("hello world", 6), 0);
        assert_eq!(word_start_pos("hello", 0), 0);
    }
}
