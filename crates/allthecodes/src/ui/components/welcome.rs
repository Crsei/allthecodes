//! Welcome screen -- compact startup info panel.
//!
//! Rendered once when the TUI starts, before any messages are displayed.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const ACCENT: Color = Color::Rgb(190, 140, 255);
const ACCENT_DIM: Color = Color::Rgb(120, 90, 180);
const MUTED: Color = Color::Rgb(100, 100, 100);
const LIGHT: Color = Color::Rgb(200, 200, 200);
const PANEL_WIDTH: u16 = 64;
const PANEL_HEIGHT: u16 = 8;

/// Render a small rectangular welcome summary.
///
/// The panel intentionally avoids the old ASCII logo so the prompt can sit
/// directly below a compact startup summary.
pub fn render_welcome(
    area: Rect,
    buf: &mut Buffer,
    version: &str,
    model_name: &str,
    session_id: &str,
    cwd: &str,
) {
    if area.width < 20 || area.height < PANEL_HEIGHT {
        let line = Line::from(vec![
            Span::styled(
                "allthecodes ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("v{}", version), Style::default().fg(MUTED)),
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
        return;
    }

    let panel = left_aligned_panel(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_DIM))
        .title(Line::from(vec![Span::styled(
            " allthecodes ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )]))
        .title_alignment(Alignment::Left);
    let inner = block.inner(panel);
    block.render(panel, buf);

    let short_session = if session_id.len() > 8 {
        &session_id[..8]
    } else {
        session_id
    };
    let display_model = model_name.strip_prefix("claude-").unwrap_or(model_name);
    let max_value_width = inner.width.saturating_sub(9) as usize;
    let display_cwd = truncate_start(cwd, max_value_width);
    let tip = truncate_str("Enter to send, /help for commands", max_value_width);

    let lines = vec![
        Line::from(vec![
            Span::styled("Version: ", Style::default().fg(MUTED)),
            Span::styled(
                format!("v{}", version),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model:   ", Style::default().fg(MUTED)),
            Span::styled(display_model, Style::default().fg(LIGHT)),
        ]),
        Line::from(vec![
            Span::styled("Session: ", Style::default().fg(MUTED)),
            Span::styled(short_session, Style::default().fg(LIGHT)),
        ]),
        Line::from(vec![
            Span::styled("CWD:     ", Style::default().fg(MUTED)),
            Span::styled(display_cwd, Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled("Tips:    ", Style::default().fg(MUTED)),
            Span::styled(tip, Style::default().fg(LIGHT)),
        ]),
    ];

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(inner, buf);
}

fn left_aligned_panel(area: Rect) -> Rect {
    let width = PANEL_WIDTH.min(area.width);
    let height = PANEL_HEIGHT.min(area.height);
    Rect {
        x: area.x,
        y: area.y,
        width,
        height,
    }
}

fn truncate_start(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        let start = chars.len() - (max_width - 3);
        format!("...{}", chars[start..].iter().collect::<String>())
    }
}

fn truncate_str(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        format!("{}...", chars[..max_width - 3].iter().collect::<String>())
    }
}

/// Preferred minimum height of the welcome screen.
pub fn welcome_height_for(width: u16) -> u16 {
    if width < 20 {
        1
    } else {
        PANEL_HEIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn test_render_welcome_small_terminal() {
        let area = Rect::new(0, 0, 15, 5);
        let mut buf = Buffer::empty(area);
        render_welcome(
            area,
            &mut buf,
            "0.1.0",
            "claude-sonnet-4",
            "abcd1234",
            "/tmp",
        );
        let content = buf_to_string(&buf, area);
        assert!(content.contains("allthecodes"));
    }

    #[test]
    fn test_render_welcome_normal() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_welcome(
            area,
            &mut buf,
            "0.1.0",
            "claude-sonnet-4",
            "abcdef1234567890",
            "/home/user/project",
        );
        let content = buf_to_string(&buf, area);
        assert!(content.contains("Version:"));
        assert!(content.contains("Model:"));
        assert!(content.contains("Session:"));
        assert!(content.contains("CWD:"));
        assert!(content.contains("Tips:"));
        assert!(!content.contains('█'), "welcome should not render a logo");
    }

    #[test]
    fn test_render_welcome_is_left_aligned() {
        let area = Rect::new(4, 2, 80, 24);
        let mut buf = Buffer::empty(area);
        render_welcome(
            area,
            &mut buf,
            "0.1.0",
            "claude-sonnet-4",
            "abcdef1234567890",
            "/home/user/project",
        );

        assert_eq!(buf[(area.x, area.y)].symbol(), "┌");
        assert_ne!(buf[(area.x + 8, area.y)].symbol(), "┌");
    }

    #[test]
    fn test_render_welcome_medium() {
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        render_welcome(area, &mut buf, "0.1.0", "test-model", "sess1234", "/path");
        let content = buf_to_string(&buf, area);
        assert!(content.contains("Version:"));
    }

    #[test]
    fn test_welcome_height_is_compact() {
        assert_eq!(welcome_height_for(15), 1);
        assert_eq!(welcome_height_for(20), PANEL_HEIGHT);
        assert_eq!(welcome_height_for(80), PANEL_HEIGHT);
    }

    #[test]
    fn test_render_welcome_has_no_logo_at_any_width() {
        for width in [60, 80, 120] {
            let area = Rect::new(0, 0, width, 20);
            let mut buf = Buffer::empty(area);
            render_welcome(
                area,
                &mut buf,
                "0.1.0",
                "claude-sonnet-4",
                "abcdef1234567890",
                "/home/user/project",
            );
            let content = buf_to_string(&buf, area);
            assert!(
                !content.contains('█'),
                "welcome should not render ASCII logo at {width} cols"
            );
            assert!(content.contains("Tips"));
        }
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 8), "hello...");
        assert_eq!(truncate_str("hi", 2), "hi");
        assert_eq!(truncate_str("hello", 3), "...");
        assert_eq!(truncate_str("hello", 1), ".");
    }

    fn buf_to_string(buf: &Buffer, area: Rect) -> String {
        let mut s = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = &buf[(x, y)];
                s.push_str(cell.symbol());
            }
            s.push('\n');
        }
        s
    }
}
