//! Scrollable pager overlays for transcript and static text views.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::text::Line;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Static(StaticOverlay),
    Transcript(TranscriptOverlay),
}

impl Overlay {
    pub fn title(&self) -> &str {
        match self {
            Self::Static(overlay) => &overlay.title,
            Self::Transcript(overlay) => &overlay.title,
        }
    }
    pub fn handle_key(&mut self, key: KeyEvent, viewport_height: usize) -> bool {
        match self {
            Self::Static(overlay) => overlay.handle_key(key, viewport_height),
            Self::Transcript(overlay) => overlay.inner.handle_key(key, viewport_height),
        }
    }
    pub fn visible_lines(&self, viewport_height: usize) -> Vec<Line<'static>> {
        match self {
            Self::Static(overlay) => overlay.visible_lines(viewport_height),
            Self::Transcript(overlay) => overlay.inner.visible_lines(viewport_height),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticOverlay {
    pub title: String,
    lines: Vec<Line<'static>>,
    scroll: usize,
}

impl StaticOverlay {
    pub fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
            scroll: 0,
        }
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn handle_key(&mut self, key: KeyEvent, viewport_height: usize) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(1, viewport_height),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(viewport_height.max(1)),
            KeyCode::PageDown | KeyCode::Char(' ') => {
                self.scroll_down(viewport_height.max(1), viewport_height)
            }
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll_to_end(viewport_height),
            KeyCode::Esc | KeyCode::Char('q') => return true,
            _ => {}
        }
        false
    }

    pub fn visible_lines(&self, viewport_height: usize) -> Vec<Line<'static>> {
        render_offset_content(&self.lines, self.scroll, viewport_height)
    }
    fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        let max_scroll = self.lines.len().saturating_sub(viewport_height);
        self.scroll = self.scroll.saturating_add(amount).min(max_scroll);
    }

    fn scroll_to_end(&mut self, viewport_height: usize) {
        self.scroll = self.lines.len().saturating_sub(viewport_height);
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptOverlay {
    pub title: String,
    inner: StaticOverlay,
}

impl TranscriptOverlay {
    pub fn new(lines: Vec<Line<'static>>) -> Self {
        Self {
            title: "Transcript".to_string(),
            inner: StaticOverlay::new("Transcript", lines),
        }
    }
    pub fn scroll(&self) -> usize {
        self.inner.scroll()
    }
}
pub fn render_offset_content(
    lines: &[Line<'static>],
    offset: usize,
    height: usize,
) -> Vec<Line<'static>> {
    lines.iter().skip(offset).take(height).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyModifiers};

    #[test]
    fn page_down_moves_scroll() {
        let mut overlay = StaticOverlay::new(
            "test",
            (0..10).map(|i| Line::from(format!("{i}"))).collect(),
        );
        overlay.handle_key(
            KeyEvent::new_with_kind(KeyCode::PageDown, KeyModifiers::NONE, KeyEventKind::Press),
            3,
        );
        assert_eq!(overlay.scroll(), 3);
    }

    #[test]
    fn overlay_variants_expose_title_scroll_and_visible_lines() {
        let static_overlay = Overlay::Static(StaticOverlay::new(
            "Help",
            vec![Line::from("one"), Line::from("two")],
        ));
        assert_eq!(static_overlay.title(), "Help");
        assert_eq!(static_overlay.visible_lines(1).len(), 1);

        let transcript =
            TranscriptOverlay::new((0..5).map(|i| Line::from(format!("line {i}"))).collect());
        assert_eq!(transcript.scroll(), 0);

        let mut overlay = Overlay::Transcript(transcript);
        assert_eq!(overlay.title(), "Transcript");
        assert_eq!(overlay.visible_lines(2).len(), 2);
        assert!(!overlay.handle_key(
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press),
            2,
        ));
        let Overlay::Transcript(transcript) = overlay else {
            panic!("expected transcript overlay");
        };
        assert_eq!(transcript.scroll(), 1);

        let lines = render_offset_content(
            &(0..4)
                .map(|i| Line::from(format!("{i}")))
                .collect::<Vec<_>>(),
            1,
            2,
        );
        assert_eq!(lines.len(), 2);
    }
}
