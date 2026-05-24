//! Platform-native desktop input simulation.
//!
//! Supports mouse clicks, keyboard input, scrolling, and cursor movement.

/// Mouse button for click actions.
#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// A desktop input action.
#[derive(Debug, Clone)]
pub enum InputAction {
    /// Click at screen coordinates.
    Click { x: i32, y: i32, button: MouseButton },
    /// Double-click at screen coordinates.
    DoubleClick { x: i32, y: i32 },
    /// Type text via keyboard.
    TypeText { text: String },
    /// Press a key combination (e.g. "ctrl+c", "Return", "alt+tab").
    KeyPress { key: String },
    /// Scroll at coordinates. Positive = down, negative = up.
    Scroll {
        x: i32,
        y: i32,
        /// Number of scroll "clicks". Positive = down, negative = up.
        amount: i32,
    },
    /// Move mouse cursor to coordinates.
    MouseMove { x: i32, y: i32 },
}

/// Result of a cursor position query.
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

/// Execute a desktop input action.
pub async fn execute_input(action: InputAction) -> anyhow::Result<String> {
    platform::execute(action).await
}

/// Get the current cursor position.
pub async fn get_cursor_position() -> anyhow::Result<CursorPosition> {
    platform::cursor_position().await
}

// ---------------------------------------------------------------------------
// Platform dispatch
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[path = "win32.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "darwin.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::{CursorPosition, InputAction};
    pub async fn execute(_action: InputAction) -> anyhow::Result<String> {
        anyhow::bail!("Input simulation not supported on this platform")
    }
    pub async fn cursor_position() -> anyhow::Result<CursorPosition> {
        anyhow::bail!("Cursor position not supported on this platform")
    }
}
