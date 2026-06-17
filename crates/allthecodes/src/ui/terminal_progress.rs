use std::env;
use std::fmt;
use std::io::{self, IsTerminal, Write};

use crossterm::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalProgressState {
    Running(u8),
    Error(u8),
    Indeterminate,
    Clear,
}

const _: fn() = terminal_progress_symbol_anchors;

fn terminal_progress_symbol_anchors() {
    let _ = TerminalProgressState::Running(0);
    let _ = TerminalProgressState::Error(0);
}

#[derive(Debug)]
pub struct TerminalProgressBackend {
    enabled: bool,
    available: bool,
    active: bool,
}

impl TerminalProgressBackend {
    pub fn detect(enabled: bool) -> Self {
        Self {
            enabled,
            available: is_progress_reporting_available(),
            active: false,
        }
    }

    pub fn set_enabled<W: Write>(&mut self, enabled: bool, writer: &mut W) -> io::Result<()> {
        if self.enabled == enabled {
            return Ok(());
        }
        self.enabled = enabled;
        if !enabled {
            self.clear(writer)?;
        }
        Ok(())
    }

    pub fn indeterminate<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.emit(TerminalProgressState::Indeterminate, writer)
    }

    #[cfg(test)]
    pub fn running<W: Write>(&mut self, percentage: u8, writer: &mut W) -> io::Result<()> {
        self.emit(TerminalProgressState::Running(percentage), writer)
    }

    #[cfg(test)]
    pub fn error<W: Write>(&mut self, percentage: u8, writer: &mut W) -> io::Result<()> {
        self.emit(TerminalProgressState::Error(percentage), writer)
    }

    pub fn clear<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.emit(TerminalProgressState::Clear, writer)
    }

    fn emit<W: Write>(&mut self, state: TerminalProgressState, writer: &mut W) -> io::Result<()> {
        if !self.available {
            self.active = false;
            return Ok(());
        }
        if state == TerminalProgressState::Clear {
            crossterm::execute!(writer, SetTerminalProgress(state))?;
            writer.flush()?;
            self.active = false;
            return Ok(());
        }
        if !self.enabled {
            return Ok(());
        }
        crossterm::execute!(writer, SetTerminalProgress(state))?;
        writer.flush()?;
        self.active = true;
        Ok(())
    }
}

pub fn is_progress_reporting_available() -> bool {
    is_progress_reporting_available_with_tty(io::stdout().is_terminal())
}

fn is_progress_reporting_available_with_tty(stdout_is_tty: bool) -> bool {
    if !stdout_is_tty {
        return false;
    }
    if env::var_os("WT_SESSION").is_some() {
        return false;
    }
    if env::var_os("ConEmuANSI").is_some()
        || env::var_os("ConEmuPID").is_some()
        || env::var_os("ConEmuTask").is_some()
    {
        return true;
    }

    let Some(version) = env::var("TERM_PROGRAM_VERSION")
        .ok()
        .and_then(|value| coerce_version(&value))
    else {
        return false;
    };

    match env::var("TERM_PROGRAM").ok().as_deref() {
        Some("ghostty") => version_at_least(version, (1, 2, 0)),
        Some("iTerm.app") => version_at_least(version, (3, 6, 6)),
        _ => false,
    }
}

fn coerce_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut seen_digit = false;

    for ch in value.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
            seen_digit = true;
            continue;
        }
        if ch == '.' && seen_digit {
            if current.is_empty() {
                break;
            }
            parts.push(current.parse::<u64>().ok()?);
            current.clear();
            continue;
        }
        if seen_digit {
            break;
        }
    }

    if !current.is_empty() {
        parts.push(current.parse::<u64>().ok()?);
    }
    if parts.is_empty() {
        return None;
    }
    while parts.len() < 3 {
        parts.push(0);
    }
    Some((parts[0], parts[1], parts[2]))
}

fn version_at_least(version: (u64, u64, u64), minimum: (u64, u64, u64)) -> bool {
    version >= minimum
}

#[derive(Debug, Clone, Copy)]
pub struct SetTerminalProgress(pub TerminalProgressState);

impl Command for SetTerminalProgress {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(
            f,
            "{}",
            wrap_for_multiplexer(&terminal_progress_sequence(self.0))
        )
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute SetTerminalProgress using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

fn terminal_progress_sequence(state: TerminalProgressState) -> String {
    let (operation, value) = match state {
        TerminalProgressState::Clear => (0, String::new()),
        TerminalProgressState::Running(percentage) => (1, percentage.min(100).to_string()),
        TerminalProgressState::Error(percentage) => (2, percentage.min(100).to_string()),
        TerminalProgressState::Indeterminate => (3, String::new()),
    };
    format!("\x1b]9;4;{operation};{value}\x07")
}

fn wrap_for_multiplexer(sequence: &str) -> String {
    if env::var_os("TMUX").is_some() {
        return format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"));
    }
    if env::var_os("STY").is_some() {
        return format!("\x1bP{sequence}\x1b\\");
    }
    sequence.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::ffi::OsString;

    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = env::var_os(key);
            unsafe {
                env::set_var(key, value);
            }
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = env::var_os(key);
            unsafe {
                env::remove_var(key);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.original {
                    Some(value) => env::set_var(self.key, value),
                    None => env::remove_var(self.key),
                }
            }
        }
    }

    fn clear_progress_env() -> Vec<EnvVarGuard> {
        vec![
            EnvVarGuard::remove("WT_SESSION"),
            EnvVarGuard::remove("ConEmuANSI"),
            EnvVarGuard::remove("ConEmuPID"),
            EnvVarGuard::remove("ConEmuTask"),
            EnvVarGuard::remove("TERM_PROGRAM"),
            EnvVarGuard::remove("TERM_PROGRAM_VERSION"),
            EnvVarGuard::remove("TMUX"),
            EnvVarGuard::remove("STY"),
        ]
    }

    fn write_command(state: TerminalProgressState) -> String {
        let mut out = String::new();
        SetTerminalProgress(state)
            .write_ansi(&mut out)
            .expect("ansi");
        out
    }

    #[test]
    #[serial]
    fn writes_progress_sequences() {
        let _env = clear_progress_env();
        assert_eq!(
            write_command(TerminalProgressState::Clear),
            "\x1b]9;4;0;\x07"
        );
        assert_eq!(
            write_command(TerminalProgressState::Indeterminate),
            "\x1b]9;4;3;\x07"
        );
        assert_eq!(
            write_command(TerminalProgressState::Running(42)),
            "\x1b]9;4;1;42\x07"
        );
        assert_eq!(
            write_command(TerminalProgressState::Error(101)),
            "\x1b]9;4;2;100\x07"
        );
    }

    #[test]
    #[serial]
    fn wraps_progress_for_tmux_and_screen() {
        let _env = clear_progress_env();
        let _tmux = EnvVarGuard::set("TMUX", "/tmp/tmux");
        assert_eq!(
            write_command(TerminalProgressState::Indeterminate),
            "\x1bPtmux;\x1b\x1b]9;4;3;\x07\x1b\\"
        );
        drop(_tmux);

        let _screen = EnvVarGuard::set("STY", "screen-session");
        assert_eq!(
            write_command(TerminalProgressState::Clear),
            "\x1bP\x1b]9;4;0;\x07\x1b\\"
        );
    }

    #[test]
    #[serial]
    fn detects_supported_terminals() {
        let _env = clear_progress_env();
        assert!(!is_progress_reporting_available_with_tty(false));
        assert!(!is_progress_reporting_available_with_tty(true));

        let _conemu = EnvVarGuard::set("ConEmuANSI", "ON");
        assert!(is_progress_reporting_available_with_tty(true));
        drop(_conemu);

        let _term = EnvVarGuard::set("TERM_PROGRAM", "ghostty");
        let _version = EnvVarGuard::set("TERM_PROGRAM_VERSION", "1.2.0");
        assert!(is_progress_reporting_available_with_tty(true));
        drop(_version);
        let _version = EnvVarGuard::set("TERM_PROGRAM_VERSION", "1.1.9");
        assert!(!is_progress_reporting_available_with_tty(true));
        drop(_term);
        drop(_version);

        let _term = EnvVarGuard::set("TERM_PROGRAM", "iTerm.app");
        let _version = EnvVarGuard::set("TERM_PROGRAM_VERSION", "Build 3.6.6");
        assert!(is_progress_reporting_available_with_tty(true));
        drop(_version);
        let _version = EnvVarGuard::set("TERM_PROGRAM_VERSION", "3.6.5");
        assert!(!is_progress_reporting_available_with_tty(true));
    }

    #[test]
    #[serial]
    fn windows_terminal_is_excluded() {
        let _env = clear_progress_env();
        let _wt = EnvVarGuard::set("WT_SESSION", "session");
        let _conemu = EnvVarGuard::set("ConEmuANSI", "ON");
        assert!(!is_progress_reporting_available_with_tty(true));
    }

    #[test]
    #[serial]
    fn backend_does_not_emit_when_disabled_or_unavailable() {
        let _env = clear_progress_env();
        let mut out = Vec::new();
        let mut backend = TerminalProgressBackend {
            enabled: false,
            available: true,
            active: false,
        };
        backend.indeterminate(&mut out).expect("indeterminate");
        assert!(out.is_empty());

        backend.enabled = true;
        backend.available = false;
        backend.indeterminate(&mut out).expect("indeterminate");
        assert!(out.is_empty());
    }

    #[test]
    #[serial]
    fn disabling_backend_clears_active_progress() {
        let _env = clear_progress_env();
        let mut out = Vec::new();
        let mut backend = TerminalProgressBackend {
            enabled: true,
            available: true,
            active: false,
        };
        backend.indeterminate(&mut out).expect("indeterminate");
        backend.set_enabled(false, &mut out).expect("disable");
        assert_eq!(out, b"\x1b]9;4;3;\x07\x1b]9;4;0;\x07");
    }

    #[test]
    #[serial]
    fn clear_emits_even_without_prior_active_progress() {
        let _env = clear_progress_env();
        let mut out = Vec::new();
        let mut backend = TerminalProgressBackend {
            enabled: false,
            available: true,
            active: false,
        };
        backend.clear(&mut out).expect("clear");
        assert_eq!(out, b"\x1b]9;4;0;\x07");
    }

    #[test]
    #[serial]
    fn running_and_error_helpers_emit_sequences() {
        let _env = clear_progress_env();
        let mut out = Vec::new();
        let mut backend = TerminalProgressBackend {
            enabled: true,
            available: true,
            active: false,
        };
        backend.running(7, &mut out).expect("running");
        backend.error(12, &mut out).expect("error");
        assert_eq!(out, b"\x1b]9;4;1;7\x07\x1b]9;4;2;12\x07");
    }
}
