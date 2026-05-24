use std::fmt;
use std::io;
use std::io::stdout;

use crossterm::Command;
use ratatui::crossterm::execute;

#[derive(Debug, Default)]
pub struct Osc9Backend;

impl Osc9Backend {
    pub fn notify(&mut self, message: &str) -> io::Result<()> {
        execute!(stdout(), PostNotification(message.to_string()))
    }
}

/// Command that emits an OSC 9 desktop notification with a message.
#[derive(Debug, Clone)]
pub struct PostNotification(pub String);

impl Command for PostNotification {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b]9;{}\x07", self.0)
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
    fn post_notification_writes_osc9_escape() {
        let mut out = String::new();
        PostNotification("hello".to_string())
            .write_ansi(&mut out)
            .expect("ansi");
        assert_eq!(out, "\x1b]9;hello\x07");
        let _: fn(&mut Osc9Backend, &str) -> io::Result<()> = Osc9Backend::notify;
    }
}
