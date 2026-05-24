//! Virtual cursor management for Windows.
//!
//! Tracks a "virtual cursor" state that is bound to a specific window.
//! When a window is focused, the virtual cursor operates within that window's
//! coordinate space, automatically adjusting for window position changes.
//!
//! This is used by Computer Use tools to maintain stable cursor positioning
//! within an application across multiple interactions, even if the window
//! is moved or resized between operations.

use super::shared::{get_window_rect, get_window_text, Hwnd};

/// State of the virtual cursor.
#[derive(Debug, Clone)]
pub struct VirtualCursor {
    /// The window this cursor is bound to (if any).
    pub target_window: Option<Hwnd>,
    /// Window title at last bind (for validation).
    pub window_title: Option<String>,
    /// X position within the window (client coordinates).
    pub x: i32,
    /// Y position within the window (client coordinates).
    pub y: i32,
    /// Whether the cursor is currently active.
    pub active: bool,
    /// Last known window bounds (left, top, right, bottom) for offset calculation.
    pub window_bounds: Option<(i32, i32, i32, i32)>,
}

impl VirtualCursor {
    /// Create a new, inactive virtual cursor.
    pub fn new() -> Self {
        Self {
            target_window: None,
            window_title: None,
            x: 0,
            y: 0,
            active: false,
            window_bounds: None,
        }
    }

    /// Bind the virtual cursor to a specific window.
    ///
    /// Returns the window's current bounds on success.
    pub async fn bind_to_window(&mut self, hwnd: Hwnd) -> anyhow::Result<(i32, i32, i32, i32)> {
        let bounds = get_window_rect(hwnd).await?;
        let title = get_window_text(hwnd).await?;

        self.target_window = Some(hwnd);
        self.window_title = Some(title);
        self.window_bounds = Some(bounds);
        self.active = true;

        // Center cursor in the window.
        let (left, top, right, bottom) = bounds;
        self.x = (left + right) / 2 - left; // client-relative
        self.y = (top + bottom) / 2 - top;

        Ok(bounds)
    }

    /// Unbind from the current window.
    pub fn unbind(&mut self) {
        self.target_window = None;
        self.window_title = None;
        self.window_bounds = None;
        self.active = false;
    }

    /// Set the virtual cursor position in client coordinates.
    pub fn set_position(&mut self, client_x: i32, client_y: i32) {
        self.x = client_x;
        self.y = client_y;
    }

    /// Get the absolute screen coordinates of the virtual cursor.
    ///
    /// Returns `None` if not bound to a window or bounds are unknown.
    pub fn screen_coordinates(&self) -> Option<(i32, i32)> {
        let (left, top, _, _) = self.window_bounds?;
        Some((left + self.x, top + self.y))
    }

    /// Update the stored window bounds (e.g., after a move/resize).
    pub async fn refresh_window_bounds(&mut self) -> anyhow::Result<()> {
        let hwnd = self
            .target_window
            .ok_or_else(|| anyhow::anyhow!("No window bound"))?;
        let bounds = get_window_rect(hwnd).await?;
        self.window_bounds = Some(bounds);
        Ok(())
    }

    /// Check whether the bound window is still valid (exists and title matches).
    pub async fn validate_binding(&mut self) -> bool {
        let hwnd = match self.target_window {
            Some(h) => h,
            None => return false,
        };
        match get_window_text(hwnd).await {
            Ok(title) => {
                let is_valid = self
                    .window_title
                    .as_ref()
                    .map(|t| title.contains(t) || t.contains(&title))
                    .unwrap_or(false);
                if !is_valid {
                    self.active = false;
                }
                is_valid
            }
            Err(_) => {
                self.active = false;
                false
            }
        }
    }

    /// Move the virtual cursor by a delta (for relative movement).
    pub fn move_by(&mut self, dx: i32, dy: i32) {
        self.x = self.x.saturating_add(dx).max(0);
        self.y = self.y.saturating_add(dy).max(0);
    }

    /// Clamp the virtual cursor to the window bounds.
    pub fn clamp_to_window(&mut self) {
        if let Some((left, top, right, bottom)) = self.window_bounds {
            let width = right - left;
            let height = bottom - top;
            self.x = self.x.clamp(0, width);
            self.y = self.y.clamp(0, height);
        }
    }

    /// Reset to inactive state with zero position.
    pub fn reset(&mut self) {
        self.x = 0;
        self.y = 0;
        self.active = false;
    }
}

impl Default for VirtualCursor {
    fn default() -> Self {
        Self::new()
    }
}

/// The global virtual cursor instance.
use std::sync::{LazyLock, Mutex};
static GLOBAL_VIRTUAL_CURSOR: LazyLock<Mutex<VirtualCursor>> =
    LazyLock::new(|| Mutex::new(VirtualCursor::new()));

/// Access the global virtual cursor.
pub fn global_cursor() -> &'static Mutex<VirtualCursor> {
    &GLOBAL_VIRTUAL_CURSOR
}

/// Bind the global cursor to a window by handle.
pub async fn bind_global(hwnd: Hwnd) -> anyhow::Result<(i32, i32, i32, i32)> {
    let mut cursor = global_cursor().lock().unwrap();
    cursor.bind_to_window(hwnd).await
}

/// Get the screen coordinates from the global virtual cursor.
pub fn get_global_screen_coords() -> Option<(i32, i32)> {
    let cursor = global_cursor().lock().ok()?;
    cursor.screen_coordinates()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_virtual_cursor_bind_unbind() {
        let mut cursor = VirtualCursor::new();
        assert!(!cursor.active);

        // Without a valid window handle, bind_to_window should fail.
        cursor.bind_to_window(Hwnd(999999)).await.unwrap_err();
        assert!(!cursor.active);

        cursor.unbind();
        assert!(!cursor.active);
    }

    #[test]
    fn test_virtual_cursor_position() {
        let mut cursor = VirtualCursor::new();
        cursor.set_position(100, 200);
        assert_eq!(cursor.x, 100);
        assert_eq!(cursor.y, 200);
    }

    #[test]
    fn test_virtual_cursor_move_by() {
        let mut cursor = VirtualCursor::new();
        cursor.set_position(10, 20);
        cursor.move_by(5, -3);
        assert_eq!(cursor.x, 15);
        assert_eq!(cursor.y, 17);
    }

    #[test]
    fn test_screen_coordinates_no_window() {
        let cursor = VirtualCursor::new();
        assert!(cursor.screen_coordinates().is_none());
    }

    #[test]
    fn test_global_cursor_instance() {
        let cursor = global_cursor().lock().unwrap();
        assert!(!cursor.active);
    }
}
