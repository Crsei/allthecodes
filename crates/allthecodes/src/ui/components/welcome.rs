//! Welcome screen -- compact startup info panel.
//!
//! Rendered once when the TUI starts, before any messages are displayed.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::brand_logo::{
    render_brand_logo, LogoFrame, BRAND_COLUMN_HEIGHT, BRAND_COLUMN_WIDTH,
};
use crate::ui::theme::ThemeColors;

const PANEL_WIDTH: u16 = 64;
const PANEL_HEIGHT: u16 = 8;
const LOGO_LAYOUT_MIN_WIDTH: u16 = 48;
const BRAND_GAP: u16 = 2;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WelcomeInfo<'a> {
    pub(crate) version: &'a str,
    pub(crate) model_name: &'a str,
    pub(crate) session_id: &'a str,
    pub(crate) cwd: &'a str,
}

/// Render a small rectangular welcome summary.
///
/// The panel intentionally avoids the old ASCII logo so the prompt can sit
/// directly below a compact startup summary.
pub(crate) fn render_welcome(
    area: Rect,
    buf: &mut Buffer,
    info: WelcomeInfo<'_>,
    logo_frame: LogoFrame,
    colors: &ThemeColors,
) -> bool {
    if area.width < 20 || area.height < PANEL_HEIGHT {
        let line = Line::from(vec![
            Span::styled(
                "allthecodes ",
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("v{}", info.version),
                Style::default().fg(colors.mutedText),
            ),
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
        return false;
    }

    let panel = left_aligned_panel(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.accentDim))
        .title(Line::from(vec![Span::styled(
            " allthecodes ",
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        )]))
        .title_alignment(Alignment::Left);
    let inner = block.inner(panel);
    block.render(panel, buf);

    let show_logo = area.width >= LOGO_LAYOUT_MIN_WIDTH
        && area.height >= PANEL_HEIGHT
        && inner.height >= BRAND_COLUMN_HEIGHT;
    if show_logo {
        let columns = Layout::horizontal([
            Constraint::Length(BRAND_COLUMN_WIDTH),
            Constraint::Length(BRAND_GAP),
            Constraint::Min(0),
        ])
        .split(inner);
        render_brand_logo(columns[0], buf, logo_frame, colors);
        render_info_lines(columns[2], buf, info, colors);
    } else {
        render_info_lines(inner, buf, info, colors);
    }

    show_logo
}

fn render_info_lines(area: Rect, buf: &mut Buffer, info: WelcomeInfo<'_>, colors: &ThemeColors) {
    let raw_model = info
        .model_name
        .strip_prefix("claude-")
        .unwrap_or(info.model_name);
    let max_value_width = area.width.saturating_sub(9) as usize;
    let display_version = truncate_str(&format!("v{}", info.version), max_value_width);
    let display_model = truncate_str(raw_model, max_value_width);
    let short_session = info
        .session_id
        .chars()
        .take(max_value_width.min(8))
        .collect::<String>();
    let display_cwd = truncate_start(info.cwd, max_value_width);
    let tip = truncate_str("Enter to send, /help for commands", max_value_width);
    let label = Style::default().fg(colors.mutedText);
    let value = Style::default().fg(colors.surfaceText);

    let lines = vec![
        Line::from(vec![
            Span::styled("Version: ", label),
            Span::styled(
                display_version,
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model:   ", label),
            Span::styled(display_model, value),
        ]),
        Line::from(vec![
            Span::styled("Session: ", label),
            Span::styled(short_session, value),
        ]),
        Line::from(vec![
            Span::styled("CWD:     ", label),
            Span::styled(display_cwd, label),
        ]),
        Line::from(vec![
            Span::styled("Tips:    ", label),
            Span::styled(tip, value),
        ]),
    ];

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buf);
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
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
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
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
        );
        let content = buf_to_string(&buf, area);
        assert!(content.contains("Version:"));
        assert!(content.contains("Model:"));
        assert!(content.contains("Session:"));
        assert!(content.contains("CWD:"));
        assert!(content.contains("Tips:"));
    }

    #[test]
    fn test_render_welcome_is_left_aligned() {
        let area = Rect::new(4, 2, 80, 24);
        let mut buf = Buffer::empty(area);
        render_welcome(
            area,
            &mut buf,
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
        );

        assert_eq!(buf[(area.x, area.y)].symbol(), "┌");
        assert_ne!(buf[(area.x + 8, area.y)].symbol(), "┌");
    }

    #[test]
    fn test_render_welcome_medium() {
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        render_welcome(
            area,
            &mut buf,
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
        );
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
    fn wide_welcome_renders_source_a_and_preserves_details() {
        let area = Rect::new(0, 0, 64, 8);
        let mut buf = Buffer::empty(area);
        let rendered = render_welcome(
            area,
            &mut buf,
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
        );
        let content = buf_to_string(&buf, area);
        assert!(rendered);
        assert!(content.contains("░░ ██ ░░"));
        assert!(content.contains("██ ██ ██"));
        assert!(content.contains("██ ░░ ██"));
        assert!(content.contains("ALLTHECODES"));
        assert!(content.contains("Version:"));
        assert!(content.contains("Tips:"));
    }

    #[test]
    fn medium_welcome_keeps_details_without_logo() {
        let area = Rect::new(0, 0, 47, 8);
        let mut buf = Buffer::empty(area);
        let rendered = render_welcome(
            area,
            &mut buf,
            test_info(),
            crate::ui::brand_logo::frame_at(0),
            dark_colors(),
        );
        let content = buf_to_string(&buf, area);
        assert!(!rendered);
        assert!(!content.contains("ALLTHECODES"));
        assert!(content.contains("Version:"));
        assert!(content.contains("Session:"));
    }

    #[test]
    fn welcome_responsive_layouts() {
        insta::assert_snapshot!(
            "welcome_responsive_layouts",
            render_responsive_test_cases([(64, 8), (48, 8), (47, 8), (19, 5), (64, 7)]),
        );
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

    fn test_info() -> WelcomeInfo<'static> {
        WelcomeInfo {
            version: "0.1.0",
            model_name: "claude-sonnet-4",
            session_id: "abcdef1234567890",
            cwd: "/home/user/project",
        }
    }

    fn dark_colors() -> &'static ThemeColors {
        crate::ui::theme::ThemeProvider::with_name(crate::ui::theme::ThemeName::Dark).colors()
    }

    fn render_responsive_test_cases<const N: usize>(cases: [(u16, u16); N]) -> String {
        cases
            .into_iter()
            .map(|(width, height)| {
                let area = Rect::new(0, 0, width, height);
                let mut buf = Buffer::empty(area);
                let logo = render_welcome(
                    area,
                    &mut buf,
                    test_info(),
                    crate::ui::brand_logo::frame_at(0),
                    dark_colors(),
                );
                format!(
                    "{width}x{height} logo={logo}\n{}",
                    buf_to_string(&buf, area).trim_end(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
