//! Generic fuzzy-finder / search-and-select picker component.
//!
//! Combines SearchBox, ListItem, Byline, and Pane into a single picker
//! surface.  The caller owns filtering, focus tracking, and event handling;
//! this component owns the layout and rendering.
//!
//! Mirrors the upstream TypeScript `FuzzyPicker.tsx` design-system component.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::list_item::ListItem;
use crate::ui::search_box::SearchBox;
use crate::ui::theme::ThemeColors;

type PreviewRenderer<'a, T> = dyn Fn(&T) -> Vec<Line<'static>> + 'a;

/// Which direction the list grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerDirection {
    /// Items appear below the search box (default).
    Down,
    /// Items appear above the search box (atuin-style).
    Up,
}

/// Configuration for a fuzzy picker over items of type `T`.
///
/// The caller manages filtering, focus, and selection externally.
/// Call [`render`] each frame to produce the visual output.
///
/// # Type parameters
///
/// * `T` — The item type.  Only used for rendering; equality/comparison is
///   the caller's responsibility.
pub struct FuzzyPicker<'a, T> {
    /// Dialog title shown at the top.
    pub title: String,
    /// Placeholder text for the search box.
    pub placeholder: String,
    /// Message shown when the items list is empty.
    pub empty_message: String,
    /// Optional status line below the list (e.g. "42 matches").
    pub match_label: Option<String>,
    /// Label for the primary select action ("select", "open", …).
    pub select_action: String,
    /// Direction the list grows.
    pub direction: PickerDirection,
    /// Number of items visible at once (window height).
    pub visible_count: usize,

    // -- Caller-controlled state (set each frame) --
    /// Current search query.
    pub query: &'a str,
    /// Cursor offset for the search box (character index).
    pub cursor_offset: Option<usize>,
    /// The items to display (already filtered by the caller).
    pub items: &'a [T],
    /// Index of the currently focused item.
    pub focused_index: usize,
    /// Whether the terminal (not just the picker) is focused.
    pub is_terminal_focused: bool,

    // -- Render callbacks --
    /// Render a single item into its display text.
    /// Called with `(item, is_focused)`.
    pub render_item: &'a dyn Fn(&T, bool) -> String,
    /// Optional preview rendered when an item is focused.
    pub render_preview: Option<&'a PreviewRenderer<'a, T>>,
}

impl<'a, T> FuzzyPicker<'a, T> {
    /// Create a new fuzzy picker.
    pub fn new(
        title: impl Into<String>,
        query: &'a str,
        items: &'a [T],
        render_item: &'a dyn Fn(&T, bool) -> String,
    ) -> Self {
        Self {
            title: title.into(),
            placeholder: "Type to search…".into(),
            empty_message: "No results".into(),
            match_label: None,
            select_action: "select".into(),
            direction: PickerDirection::Down,
            visible_count: 8,
            query,
            cursor_offset: None,
            items,
            focused_index: 0,
            is_terminal_focused: true,
            render_item,
            render_preview: None,
        }
    }

    // Builder-style setters.
    pub fn placeholder(mut self, v: impl Into<String>) -> Self {
        self.placeholder = v.into();
        self
    }
    pub fn empty_message(mut self, v: impl Into<String>) -> Self {
        self.empty_message = v.into();
        self
    }

    pub fn match_label(mut self, v: impl Into<String>) -> Self {
        self.match_label = Some(v.into());
        self
    }
    pub fn select_action(mut self, v: impl Into<String>) -> Self {
        self.select_action = v.into();
        self
    }

    pub fn direction(mut self, v: PickerDirection) -> Self {
        self.direction = v;
        self
    }

    pub fn visible_count(mut self, v: usize) -> Self {
        self.visible_count = v.max(2);
        self
    }
    pub fn cursor_offset(mut self, v: usize) -> Self {
        self.cursor_offset = Some(v);
        self
    }
    pub fn terminal_focused(mut self, v: bool) -> Self {
        self.is_terminal_focused = v;
        self
    }

    pub fn render_preview(mut self, v: &'a dyn Fn(&T) -> Vec<Line<'static>>) -> Self {
        self.render_preview = Some(v);
        self
    }

    // -- Rendering --

    /// Render the full fuzzy picker as a `Vec<Line>`.
    ///
    /// The output includes:
    /// 1. Title line (styled with `permission` color)
    /// 2. Search box (styled with the configured colors)
    /// 3. Item list with scroll indicators and focus highlight
    /// 4. Optional match label
    /// 5. Optional preview for the focused item
    /// 6. Byline with keyboard shortcut hints
    pub fn render(&self, colors: &ThemeColors) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // 1. Title.
        lines.push(Line::from(Span::styled(
            self.title.clone(),
            Style::default()
                .fg(colors.permission)
                .add_modifier(Modifier::BOLD),
        )));

        // 2. Search box (input first if direction is Down).
        let input_above = self.direction == PickerDirection::Down;
        if input_above {
            lines.push(self.render_search_line(colors));
        }

        // 3. Item list with optional match label.
        lines.extend(self.render_list(colors));

        // 4. Search box below if direction is Up.
        if !input_above {
            lines.push(self.render_search_line(colors));
        }

        // 5. Optional preview (after the list, before the byline).
        if let Some(preview_fn) = self.render_preview {
            if !self.items.is_empty() {
                let focused = &self.items[self.focused_index.min(self.items.len() - 1)];
                let preview_lines = preview_fn(focused);
                if !preview_lines.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "─".repeat(40),
                        Style::default().fg(colors.border),
                    )));
                    lines.extend(preview_lines);
                }
            }
        }

        // 6. Byline with keyboard hints.
        lines.push(self.render_byline(colors, input_above));

        lines
    }

    /// Render the search box line.
    fn render_search_line(&self, colors: &ThemeColors) -> Line<'static> {
        let search = SearchBox::new(self.query)
            .placeholder(&self.placeholder)
            .focused(true)
            .terminal_focused(self.is_terminal_focused);
        let search = match self.cursor_offset {
            Some(off) => search.cursor_offset(off),
            None => search,
        };
        let rendered = search.render();
        Line::from(Span::styled(
            rendered,
            Style::default().fg(colors.surfaceText),
        ))
    }

    /// Render the item list with windowed scrolling.
    fn render_list(&self, colors: &ThemeColors) -> Vec<Line<'static>> {
        if self.items.is_empty() {
            return vec![Line::from(Span::styled(
                self.empty_message.clone(),
                Style::default().fg(colors.dim),
            ))];
        }

        let total = self.items.len();
        let visible = self.visible_count.min(total);

        // Window start: center the focused item if possible.
        let window_start = if self.focused_index + 1 >= visible {
            (self.focused_index + 1 - visible).min(total - visible)
        } else {
            0
        };
        let window_end = window_start + visible;

        let mut lines: Vec<Line<'static>> = Vec::with_capacity(visible);
        for i in window_start..window_end {
            let item = &self.items[i];
            let is_focused = i == self.focused_index;
            let at_low_edge = i == window_start && window_start > 0;
            let at_high_edge = i == window_end - 1 && window_end < total;

            // Scroll indicator logic reverses for Up direction.
            let (show_up, show_down) = if self.direction == PickerDirection::Up {
                (at_high_edge, at_low_edge)
            } else {
                (at_low_edge, at_high_edge)
            };

            let text = (self.render_item)(item, is_focused);

            let list_item = ListItem::new(&text).is_focused(is_focused).styled(false);

            let list_item = if show_up {
                list_item.show_scroll_up()
            } else if show_down {
                list_item.show_scroll_down()
            } else {
                list_item
            };

            lines.push(list_item.render(colors));
        }

        // Match label (e.g. "42 matches").
        if let Some(label) = &self.match_label {
            lines.push(Line::from(Span::styled(
                label.clone(),
                Style::default().fg(colors.dim),
            )));
        }

        lines
    }

    /// Render the byline with keyboard shortcut hints.
    fn render_byline(&self, colors: &ThemeColors, _input_above: bool) -> Line<'static> {
        use crate::ui::keyboard_shortcut::ShortcutHint;

        let nav_label = if self.direction == PickerDirection::Up {
            "nav"
        } else {
            "navigate"
        };

        let hints = [
            ShortcutHint::new("↑/↓", nav_label),
            ShortcutHint::new("Enter", &self.select_action),
            ShortcutHint::new("Esc", "cancel"),
        ];

        let spans = crate::ui::keyboard_shortcut::render_byline(&hints, colors);
        Line::from(spans)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};
    use insta::assert_snapshot;

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[allow(clippy::ptr_arg)]
    fn render_fn(item: &String, _is_focused: bool) -> String {
        item.clone()
    }

    fn preview_fn(item: &String) -> Vec<Line<'static>> {
        vec![Line::from(format!("  Preview: {}", item))]
    }

    fn render_text(picker: &FuzzyPicker<'_, String>, colors: &ThemeColors) -> String {
        picker
            .render(colors)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn snapshot_fuzzy_picker_states() {
        let items = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];

        let basic = FuzzyPicker::new("Select item", "beta", &items, &render_fn);

        let with_label = FuzzyPicker::new("Pick", "", &items, &render_fn).match_label("3 matches");

        let with_preview =
            FuzzyPicker::new("Pick", "", &items, &render_fn).render_preview(&preview_fn);

        let direction_up =
            FuzzyPicker::new("Pick (up)", "", &items, &render_fn).direction(PickerDirection::Up);

        let empty = FuzzyPicker::<String>::new("No items", "", &[], &render_fn);

        let focused_last = FuzzyPicker::new("Pick", "", &items, &render_fn).visible_count(5);

        assert_snapshot!(
            "fuzzy_picker_states",
            format!(
                "## basic\n{}\n\n## match_label\n{}\n\n## preview\n{}\n\n## direction=up\n{}\n\n## empty\n{}\n\n## focused_last\n{}",
                render_text(&basic, dark()),
                render_text(&with_label, dark()),
                render_text(&with_preview, dark()),
                render_text(&direction_up, dark()),
                render_text(&empty, dark()),
                render_text(&focused_last, dark()),
            )
        );
    }

    #[test]
    fn renders_title_and_search_box() {
        let items = vec!["A".to_string(), "B".to_string()];
        let picker = FuzzyPicker::new("Pick an item", "", &items, &render_fn);
        let lines = picker.render(dark());
        assert!(lines.len() >= 3);
        assert!(lines[0].spans.iter().any(|s| s.content.contains("Pick")));
    }

    #[test]
    fn renders_empty_message() {
        let items: Vec<String> = vec![];
        let picker = FuzzyPicker::new("Pick", "", &items, &render_fn);
        let lines = picker.render(dark());
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("No results"));
    }

    #[test]
    fn renders_focused_item_with_pointer() {
        let items = vec!["Alpha".to_string(), "Beta".to_string()];
        let picker = FuzzyPicker::new("Pick", "", &items, &render_fn).visible_count(5);
        let lines = picker.render(dark());
        // Focused item (index 0) should show ❯ pointer.
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains('❯'));
    }

    #[test]
    fn renders_match_label() {
        let items = vec!["X".to_string()];
        let picker = FuzzyPicker::new("Pick", "", &items, &render_fn).match_label("1 match");
        let lines = picker.render(dark());
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("1 match"));
    }

    #[test]
    fn renders_preview() {
        let items = vec!["Item".to_string()];
        let picker = FuzzyPicker::new("Pick", "", &items, &render_fn).render_preview(&preview_fn);
        let lines = picker.render(dark());
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("Preview:"));
    }

    #[test]
    fn direction_up_places_search_after_items() {
        let items = vec!["A".to_string(), "B".to_string()];
        let picker =
            FuzzyPicker::new("Pick", "", &items, &render_fn).direction(PickerDirection::Up);
        let lines = picker.render(dark());
        // When direction is Up, the search box appears after the item list.
        let mut search_pos = None;
        let mut item_pos = None;
        for (i, line) in lines.iter().enumerate() {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            if text.contains('/') || text.contains('[') {
                search_pos = Some(i);
            }
            if text.contains('A') || text.contains('B') {
                item_pos = Some(i);
            }
        }
        // Search should appear after items in Up mode.
        if let (Some(s), Some(it)) = (search_pos, item_pos) {
            assert!(s > it, "search {} should be after items {}", s, it);
        }
    }

    #[test]
    fn visible_count_caps_at_two() {
        let picker = FuzzyPicker::<String>::new("Pick", "", &[], &render_fn).visible_count(0);
        assert_eq!(picker.visible_count, 2);
    }

    #[test]
    fn builder_options_flow_into_rendering() {
        let items = vec!["Alpha".to_string()];
        let picker = FuzzyPicker::new("Pick", "Al", &items, &render_fn)
            .placeholder("Find")
            .empty_message("Nothing")
            .select_action("open")
            .cursor_offset(1)
            .terminal_focused(false);

        assert_eq!(picker.placeholder, "Find");
        assert_eq!(picker.empty_message, "Nothing");
        assert_eq!(picker.select_action, "open");
        assert_eq!(picker.cursor_offset, Some(1));
        assert!(!picker.is_terminal_focused);
        assert!(render_text(&picker, dark()).contains("open"));
    }
}
