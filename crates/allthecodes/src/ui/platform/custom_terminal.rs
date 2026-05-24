// test infrastructure — custom-terminal boundary for off-screen frame rendering; not wired to production TUI
//! Lightweight terminal primitives used by Codex-shaped UI adapters.
//!
//! The runtime TUI still uses `ratatui::Terminal` directly. This module keeps a
//! small custom-terminal boundary for code that wants to render into an
//! off-screen buffer and compare frames.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use unicode_width::UnicodeWidthStr;

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub struct Frame<'a> {
    area: Rect,
    buffer: &'a mut Buffer,
}

impl<'a> Frame<'a> {
    pub fn new(area: Rect, buffer: &'a mut Buffer) -> Self {
        Self { area, buffer }
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        self.buffer
    }

    pub fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellUpdate {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
}

pub fn diff_buffers(previous: &Buffer, next: &Buffer) -> Vec<CellUpdate> {
    let area = next.area;
    let mut updates = Vec::new();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            let next_symbol = next[(x, y)].symbol();
            let changed = previous.area != next.area
                || previous
                    .cell((x, y))
                    .map(|cell| cell.symbol() != next_symbol)
                    .unwrap_or(true);
            if changed {
                updates.push(CellUpdate {
                    x,
                    y,
                    symbol: next_symbol.to_string(),
                });
            }
        }
    }
    updates
}

pub fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
    let area = buffer.area;
    (area.y..area.y.saturating_add(area.height))
        .map(|y| {
            let mut line = String::new();
            for x in area.x..area.x.saturating_add(area.width) {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_changed_cells() {
        let area = Rect::new(0, 0, 4, 1);
        let old = Buffer::empty(area);
        let mut next = Buffer::empty(area);
        next.set_string(1, 0, "x", ratatui::style::Style::default());
        let diff = diff_buffers(&old, &next);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].symbol, "x");
    }

    #[test]
    fn frame_renders_widget_and_exports_lines() {
        let area = Rect::new(0, 0, 6, 1);
        let mut buffer = Buffer::empty(area);
        let mut frame = Frame::new(area, &mut buffer);
        assert_eq!(frame.area(), area);
        frame.render_widget(ratatui::widgets::Paragraph::new("wide"), area);
        frame
            .buffer_mut()
            .set_string(4, 0, "!", ratatui::style::Style::default());

        assert_eq!(display_width("a好"), 3);
        assert_eq!(buffer_to_lines(&buffer), vec!["wide!"]);
    }
}
