//! Overlay / modal dialog infrastructure.
//!
//! Provides the z-indexed [`OverlayStack`] and themed [`Dialog`] component.

pub mod dialog;
#[cfg(test)]
pub mod overlay_stack;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

use crate::ui::theme::ThemeColors;

use self::dialog::{Dialog, ExitGuard};
use super::panel_layout::{centered_rect, prompt_adjacent_rect, PanelSizePreset, PanelSizeSpec};

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    let _ = CenteredOverlayFrame::new("Overlay")
        .width(24, 96)
        .height(5, 24);
    let _ = render_centered_dialog_lines;
    let _ = render_prompt_adjacent_dialog_lines;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CenteredOverlayFrame<'a> {
    pub title: &'a str,
    pub color: Option<&'a str>,
    pub min_width: u16,
    pub max_width: u16,
    pub min_height: u16,
    pub max_height: u16,
}

impl<'a> CenteredOverlayFrame<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            color: None,
            min_width: 24,
            max_width: 96,
            min_height: 5,
            max_height: 24,
        }
    }

    pub fn with_preset(title: &'a str, preset: PanelSizePreset) -> Self {
        Self::with_spec(title, preset.spec())
    }

    pub fn with_spec(title: &'a str, spec: PanelSizeSpec) -> Self {
        Self {
            title,
            color: None,
            min_width: spec.min_width,
            max_width: spec.max_width,
            min_height: spec.min_height,
            max_height: spec.max_height,
        }
    }

    pub fn color(mut self, color: &'a str) -> Self {
        self.color = Some(color);
        self
    }

    pub fn width(mut self, min: u16, max: u16) -> Self {
        self.min_width = min;
        self.max_width = max.max(min);
        self
    }

    pub fn height(mut self, min: u16, max: u16) -> Self {
        self.min_height = min;
        self.max_height = max.max(min);
        self
    }
}

pub fn render_centered_dialog_lines(
    frame: CenteredOverlayFrame<'_>,
    body: Vec<Line<'static>>,
    area: Rect,
    buf: &mut Buffer,
    colors: &ThemeColors,
    style: Style,
) {
    if area.width < 8 || area.height < 4 {
        return;
    }

    let max_width = frame.max_width.min(area.width.saturating_sub(2)).max(1);
    let min_width = frame.min_width.min(max_width);
    let width = area.width.saturating_sub(4).min(max_width).max(min_width);

    let mut dialog = Dialog::new().title(frame.title).hide_input_guide();
    if let Some(color) = frame.color {
        dialog = dialog.color(color);
    }
    let lines = dialog.render(colors, width as usize, body, &ExitGuard::new(), false);

    let max_height = frame.max_height.min(area.height.saturating_sub(2)).max(1);
    let min_height = frame.min_height.min(max_height);
    let height = (lines.len() as u16).max(min_height).min(max_height);
    let overlay = centered_rect(area, width, height);

    Clear.render(overlay, buf);
    if style.bg.is_some() {
        for y in overlay.y..overlay.y + overlay.height {
            for x in overlay.x..overlay.x + overlay.width {
                buf[(x, y)].set_style(style);
            }
        }
    }
    Paragraph::new(lines)
        .style(style)
        .wrap(Wrap { trim: false })
        .render(overlay, buf);
}

pub fn render_prompt_adjacent_dialog_lines(
    frame: CenteredOverlayFrame<'_>,
    body: Vec<Line<'static>>,
    area: Rect,
    prompt_area: Rect,
    buf: &mut Buffer,
    colors: &ThemeColors,
    style: Style,
) {
    if area.width < 8 || area.height < 4 {
        return;
    }

    let spec = PanelSizeSpec::fixed(
        frame.min_width,
        frame.max_width,
        frame.min_height,
        frame.max_height,
    )
    .with_padding(4, 2);
    let width = spec
        .resolve_prompt_adjacent_rect(area, prompt_area, frame.min_height)
        .map(|rect| rect.width)
        .unwrap_or_else(|| area.width.saturating_sub(4).max(1));

    let mut dialog = Dialog::new().title(frame.title).hide_input_guide();
    if let Some(color) = frame.color {
        dialog = dialog.color(color);
    }
    let lines = dialog.render(colors, width as usize, body, &ExitGuard::new(), false);

    let max_height = frame.max_height.min(area.height.saturating_sub(2)).max(1);
    let min_height = frame.min_height.min(max_height);
    let height = (lines.len() as u16).max(min_height).min(max_height);
    let overlay = spec
        .resolve_prompt_adjacent_rect(area, prompt_area, height)
        .unwrap_or_else(|| prompt_adjacent_rect(area, prompt_area, width, height));

    Clear.render(overlay, buf);
    if style.bg.is_some() {
        for y in overlay.y..overlay.y + overlay.height {
            for x in overlay.x..overlay.x + overlay.width {
                buf[(x, y)].set_style(style);
            }
        }
    }
    Paragraph::new(lines)
        .style(style)
        .wrap(Wrap { trim: false })
        .render(overlay, buf);
}

/// Render a self-contained command surface directly above the prompt.
///
/// Command surfaces already include their own title and borders. Avoiding an
/// additional dialog frame leaves the available rows for the actual panel.
/// The panel is anchored to the left edge to match the command palette.
pub fn render_prompt_adjacent_lines(
    lines: Vec<Line<'static>>,
    area: Rect,
    prompt_area: Rect,
    spec: PanelSizeSpec,
    buf: &mut Buffer,
    style: Style,
) {
    if area.width < 8 || area.height < 4 {
        return;
    }

    let preferred_height = lines.len().min(u16::MAX as usize) as u16;
    let centered = spec
        .resolve_prompt_adjacent_rect(area, prompt_area, preferred_height)
        .unwrap_or_else(|| centered_rect(area, area.width.min(spec.max_width), preferred_height));

    let overlay = Rect {
        x: area.x,
        ..centered
    };
    Clear.render(overlay, buf);
    Paragraph::new(lines)
        .style(style)
        .wrap(Wrap { trim: false })
        .render(overlay, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};

    #[test]
    fn centered_overlay_frame_renders_body_lines() {
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        let frame = CenteredOverlayFrame::new("Overlay")
            .color("permission")
            .width(30, 60)
            .height(6, 10);

        render_centered_dialog_lines(
            frame,
            vec![Line::from("body")],
            area,
            &mut buffer,
            get_theme(&ThemeName::Dark),
            Style::default(),
        );

        let content = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(content.contains("Overlay"));
        assert!(content.contains("body"));
    }

    #[test]
    fn centered_rect_stays_inside_area() {
        let rect = centered_rect(Rect::new(10, 5, 20, 10), 8, 4);
        assert_eq!(rect, Rect::new(16, 8, 8, 4));
    }

    #[test]
    fn prompt_adjacent_dialog_renders_above_prompt() {
        let area = Rect::new(0, 0, 80, 24);
        let prompt_area = Rect::new(0, 20, 80, 3);
        let mut buffer = Buffer::empty(area);
        let frame = CenteredOverlayFrame::new("Prompt Dialog")
            .color("permission")
            .width(30, 60)
            .height(6, 10);

        render_prompt_adjacent_dialog_lines(
            frame,
            vec![Line::from("body")],
            area,
            prompt_area,
            &mut buffer,
            get_theme(&ThemeName::Dark),
            Style::default(),
        );

        let lines = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let title_row = lines
            .iter()
            .position(|line| line.contains("Prompt Dialog"))
            .expect("title row");
        assert!(lines.iter().any(|line| line.contains("body")));
        assert!(title_row < prompt_area.y as usize);
    }

    #[test]
    fn prompt_adjacent_lines_clear_the_underlying_buffer() {
        let area = Rect::new(0, 0, 160, 24);
        let prompt_area = Rect::new(0, 20, 160, 3);
        let mut buffer = Buffer::empty(area);
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buffer[(x, y)].set_symbol("x");
            }
        }

        render_prompt_adjacent_lines(
            vec![Line::from("panel")],
            area,
            prompt_area,
            PanelSizePreset::BetterViewPanel.spec(),
            &mut buffer,
            Style::default(),
        );

        let resolved = PanelSizePreset::BetterViewPanel
            .spec()
            .resolve_prompt_adjacent_rect(area, prompt_area, 1)
            .expect("overlay rect");
        let overlay = Rect {
            x: area.x,
            ..resolved
        };
        assert_eq!(buffer[(overlay.x, overlay.y)].symbol(), "p");
        assert_eq!(
            buffer[(overlay.x + overlay.width - 1, overlay.y)].symbol(),
            " ",
            "the command surface must erase session text behind unused cells"
        );
    }
}
