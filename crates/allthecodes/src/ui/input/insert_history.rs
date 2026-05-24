// test infrastructure — terminal scroll-region insert not wired to production TUI
//! Insert already-rendered history lines into a terminal stream.

use std::fmt;
use std::io::{self, Write};

use crossterm::Command;
use ratatui::text::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertHistoryMode {
    PreserveAnsi,
    PlainText,
}

impl InsertHistoryMode {
    pub fn preserves_ansi(self) -> bool {
        matches!(self, Self::PreserveAnsi)
    }
}

pub fn insert_history_lines<W: Write>(
    writer: &mut W,
    lines: &[Line<'_>],
    wrap_width: usize,
) -> io::Result<()> {
    insert_history_lines_with_mode(writer, lines, wrap_width, InsertHistoryMode::PlainText)
}

pub fn insert_history_lines_with_mode<W: Write>(
    writer: &mut W,
    lines: &[Line<'_>],
    wrap_width: usize,
    _mode: InsertHistoryMode,
) -> io::Result<()> {
    for line in lines {
        write_history_line(writer, line, wrap_width)?;
    }
    Ok(())
}

fn write_history_line<W: Write>(
    writer: &mut W,
    line: &Line<'_>,
    wrap_width: usize,
) -> io::Result<()> {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let width = wrap_width.max(1);
    for wrapped in textwrap::wrap(&text, width) {
        writer.write_all(wrapped.as_bytes())?;
        writer.write_all(b"\r\n")?;
    }
    if text.is_empty() {
        writer.write_all(b"\r\n")?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetScrollRegion(pub std::ops::Range<u16>);

impl Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        let start = self.0.start.saturating_add(1);
        let end = self.0.end.max(self.0.start.saturating_add(1));
        write!(f, "\x1b[{start};{end}r")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "scroll regions require ANSI escape support",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetScrollRegion;

impl Command for ResetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        f.write_str("\x1b[r")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "scroll regions require ANSI escape support",
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
    use ratatui::text::Span;

    #[test]
    fn writes_plain_history_text() {
        let mut out = Vec::new();
        insert_history_lines(&mut out, &[Line::from(Span::raw("hello"))], 80).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "hello\r\n");
    }

    #[test]
    fn preserve_ansi_mode_and_scroll_region_commands_emit_ansi() {
        assert!(InsertHistoryMode::PreserveAnsi.preserves_ansi());
        assert!(!InsertHistoryMode::PlainText.preserves_ansi());

        let mut set = String::new();
        SetScrollRegion(1..5).write_ansi(&mut set).unwrap();
        assert_eq!(set, "\x1b[2;5r");

        let mut reset = String::new();
        ResetScrollRegion.write_ansi(&mut reset).unwrap();
        assert_eq!(reset, "\x1b[r");
    }
}
