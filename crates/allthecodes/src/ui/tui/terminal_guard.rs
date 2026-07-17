use crossterm::event::{DisableFocusChange, DisableMouseCapture};
use crossterm::terminal::{self, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use std::io;
pub(super) struct TerminalGuard {
    mouse_capture_enabled: bool,
}

impl TerminalGuard {
    pub(super) fn new(mouse_capture_enabled: bool) -> Self {
        Self {
            mouse_capture_enabled,
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        if self.mouse_capture_enabled {
            let _ = execute!(
                io::stdout(),
                DisableFocusChange,
                DisableMouseCapture,
                LeaveAlternateScreen,
                cursor::Show
            );
        } else {
            let _ = execute!(
                io::stdout(),
                DisableFocusChange,
                LeaveAlternateScreen,
                cursor::Show
            );
        }
    }
}
