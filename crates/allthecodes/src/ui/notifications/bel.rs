use std::fmt;
use std::io;
use std::io::stdout;

use crossterm::Command;
use ratatui::crossterm::execute;

#[derive(Debug, Default)]
pub struct BelBackend;

impl BelBackend {
    pub fn notify(&mut self, _message: &str) -> io::Result<()> {
        execute!(stdout(), PostNotification)
    }
}

/// Command that emits a BEL desktop notification.
#[derive(Debug, Clone)]
pub struct PostNotification;

impl Command for PostNotification {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x07")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(std::io::Error::other(
            "tried to execute PostNotification using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_notification_writes_bel_escape() {
        let mut out = String::new();
        PostNotification.write_ansi(&mut out).expect("ansi");
        assert_eq!(out, "\x07");
        let _: fn(&mut BelBackend, &str) -> io::Result<()> = BelBackend::notify;
    }
}
