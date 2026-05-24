//! Tabbed-panel component with header navigation and content rendering.
//!
//! Mirrors the upstream TypeScript `Tabs.tsx` design-system component.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::ThemeColors;

/// A single tab entry.
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: String,
    pub title: String,
}

impl Tab {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

/// Tabbed panel component.
///
/// Renders a horizontal header row with the selected tab highlighted, and
/// provides the content of the selected tab.
pub struct Tabs {
    /// Available tabs.
    tabs: Vec<Tab>,
    /// Index of the currently selected tab.
    selected: usize,
    /// Optional theme colour key for the active tab indicator (default: `accent`).
    color: Option<&'static str>,
    /// Whether to use full terminal width for the header.
    use_full_width: bool,
    /// Optional fixed content height in rows.
    content_height: Option<usize>,
    /// Whether the header is focused (keyboard navigation active).
    header_focused: bool,
}

impl Tabs {
    pub fn new(tabs: Vec<Tab>) -> Self {
        Self {
            tabs,
            selected: 0,
            color: None,
            use_full_width: false,
            content_height: None,
            header_focused: false,
        }
    }

    /// Select a tab by index (clamped to valid range).
    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.selected = index;
        }
    }

    /// Select the next tab (wrapping around).
    pub fn select_next(&mut self) {
        if !self.tabs.is_empty() {
            self.selected = (self.selected + 1) % self.tabs.len();
        }
    }

    /// Select the previous tab (wrapping around).
    pub fn select_prev(&mut self) {
        if !self.tabs.is_empty() {
            self.selected = if self.selected == 0 {
                self.tabs.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.selected)
    }

    // Builder setters.
    pub fn color(mut self, v: &'static str) -> Self {
        self.color = Some(v);
        self
    }

    pub fn use_full_width(mut self) -> Self {
        self.use_full_width = true;
        self
    }

    pub fn content_height(mut self, v: usize) -> Self {
        self.content_height = Some(v);
        self
    }

    pub fn header_focus(mut self, v: bool) -> Self {
        self.header_focused = v;
        self
    }

    /// Render the tab header row.
    pub fn render_header(&self, colors: &ThemeColors, term_width: usize) -> Line<'static> {
        let _ = self.content_height;
        let _ = self.header_focused;
        let accent = self
            .color
            .and_then(|k| crate::ui::theme::color::resolve_color(k, colors))
            .unwrap_or(colors.accent);

        let mut spans: Vec<Span<'static>> = Vec::new();

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_selected = i == self.selected;

            let span = if is_selected {
                Span::styled(
                    format!(" {} ", tab.title),
                    Style::default()
                        .fg(colors.inverted)
                        .bg(accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!(" {} ", tab.title),
                    Style::default().fg(colors.inactiveText),
                )
            };
            spans.push(span);

            // Separator between tabs.
            if i < self.tabs.len() - 1 {
                spans.push(Span::styled(" ", Style::default().fg(colors.border)));
            }
        }

        // Fill remaining width if use_full_width.
        if self.use_full_width && term_width > 0 {
            let header_width: usize = spans.iter().map(|s| s.content.len()).sum();
            if header_width < term_width {
                let fill = " ".repeat(term_width - header_width);
                spans.push(Span::styled(fill, Style::default()));
            }
        }

        Line::from(spans)
    }

    /// Render the tab header as a string (legacy-compatible).
    pub fn render_header_str(&self) -> String {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                if i == self.selected {
                    format!("[{}]", tab.title)
                } else {
                    format!(" {} ", tab.title)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// For backward compatibility — the old free function.
pub fn render_tabs(labels: &[impl AsRef<str>], selected: usize) -> String {
    let tabs: Vec<Tab> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| Tab::new(i.to_string(), l.as_ref()))
        .collect();
    let mut t = Tabs::new(tabs);
    t.select(selected);
    t.render_header_str()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[test]
    fn legacy_render_tabs_matches_old_pattern() {
        assert_eq!(render_tabs(&["Status", "Config"], 1), " Status  [Config]");
    }

    #[test]
    fn header_renders_all_tabs() {
        let tabs = vec![Tab::new("a", "Alpha"), Tab::new("b", "Beta")];
        let t = Tabs::new(tabs);
        let line = t.render_header(dark(), 80);
        assert_eq!(line.spans.len(), 3); // 2 tabs + 1 separator
    }

    #[test]
    fn selected_tab_has_accent_background() {
        let tabs = vec![Tab::new("a", "Alpha"), Tab::new("b", "Beta")];
        let t = Tabs::new(tabs).color("success");
        let line = t.render_header(dark(), 80);
        // First tab is selected by default.
        assert_eq!(line.spans[0].style.bg, Some(dark().success));
    }

    #[test]
    fn select_next_wraps_around() {
        let tabs = vec![Tab::new("a", "A"), Tab::new("b", "B"), Tab::new("c", "C")];
        let mut t = Tabs::new(tabs);
        assert_eq!(t.selected(), 0);
        t.select_next();
        assert_eq!(t.selected(), 1);
        t.select_next();
        assert_eq!(t.selected(), 2);
        t.select_next();
        assert_eq!(t.selected(), 0); // wraps
    }

    #[test]
    fn select_prev_wraps_around() {
        let tabs = vec![Tab::new("a", "A"), Tab::new("b", "B")];
        let mut t = Tabs::new(tabs);
        t.select_prev();
        assert_eq!(t.selected(), 1); // wraps to last
    }

    #[test]
    fn selected_tab_returns_correct() {
        let tabs = vec![Tab::new("x", "X"), Tab::new("y", "Y")];
        let mut t = Tabs::new(tabs);
        t.select(1);
        assert_eq!(t.selected_tab().unwrap().id, "y");
        assert_eq!(t.selected_tab().unwrap().title, "Y");
    }

    #[test]
    fn layout_builder_flags_are_preserved() {
        let tabs = vec![Tab::new("a", "Alpha")];
        let t = Tabs::new(tabs)
            .use_full_width()
            .content_height(12)
            .header_focus(true);

        assert!(t.use_full_width);
        assert_eq!(t.content_height, Some(12));
        assert!(t.header_focused);
        assert!(t.render_header(dark(), 20).spans.len() >= 2);
    }

    #[test]
    fn empty_tabs_safe() {
        let t = Tabs::new(vec![]);
        assert_eq!(t.selected(), 0);
        assert!(t.selected_tab().is_none());
    }
}
