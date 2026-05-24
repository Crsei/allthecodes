use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use crate::ui::better_view_panel::BetterViewPanel;
use crate::ui::fuzzy_match::fuzzy_match;

const AGE_WIDTH: usize = 8;
const PREVIEW_ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySearchEntry {
    pub display: String,
    pub timestamp_secs: i64,
    pub source_session_id: Option<String>,
    pub source_title: Option<String>,
    pub source_cwd: Option<String>,
}

impl HistorySearchEntry {
    pub fn new(display: impl Into<String>, timestamp_secs: i64) -> Self {
        Self {
            display: display.into(),
            timestamp_secs,
            source_session_id: None,
            source_title: None,
            source_cwd: None,
        }
    }

    pub fn with_source(
        mut self,
        session_id: impl Into<String>,
        title: impl Into<String>,
        cwd: impl Into<String>,
    ) -> Self {
        self.source_session_id = Some(session_id.into());
        self.source_title = non_empty_string(title.into());
        self.source_cwd = non_empty_string(cwd.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistorySearchDialogEvent {
    None,
    Cancelled,
    Selected(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySearchDialog {
    items: Option<Vec<HistorySearchItem>>,
    query: String,
    selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistorySearchItem {
    entry: HistorySearchEntry,
    lower: String,
    first_line: String,
    age: String,
    source: String,
}

impl HistorySearchDialog {
    pub fn loading(initial_query: impl Into<String>, _now_secs: i64) -> Self {
        Self {
            items: None,
            query: initial_query.into(),
            selected: 0,
        }
    }

    pub fn from_entries(
        entries: impl IntoIterator<Item = HistorySearchEntry>,
        initial_query: impl Into<String>,
        now_secs: i64,
    ) -> Self {
        let mut dialog = Self {
            items: Some(
                entries
                    .into_iter()
                    .map(|entry| HistorySearchItem::new(entry, now_secs))
                    .collect(),
            ),
            query: initial_query.into(),
            selected: 0,
        };
        dialog.clamp_selection();
        dialog
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn selected_prompt(&self) -> Option<&str> {
        let idx = self.selected_item_index()?;
        self.items
            .as_ref()
            .and_then(|items| items.get(idx))
            .map(|item| item.entry.display.as_str())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> HistorySearchDialogEvent {
        if key.kind != KeyEventKind::Press {
            return HistorySearchDialogEvent::None;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.move_next();
                HistorySearchDialogEvent::None
            }
            (_, KeyCode::Up) => {
                self.move_prev();
                HistorySearchDialogEvent::None
            }
            (_, KeyCode::Down) | (_, KeyCode::Tab) => {
                self.move_next();
                HistorySearchDialogEvent::None
            }
            (_, KeyCode::Enter) => self
                .selected_prompt()
                .map(|prompt| HistorySearchDialogEvent::Selected(prompt.to_string()))
                .unwrap_or(HistorySearchDialogEvent::None),
            (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                HistorySearchDialogEvent::Cancelled
            }
            (_, KeyCode::Backspace) => {
                self.query.pop();
                self.selected = 0;
                HistorySearchDialogEvent::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.query.clear();
                self.selected = 0;
                HistorySearchDialogEvent::None
            }
            (modifiers, KeyCode::Char(ch))
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                self.query.push(ch);
                self.selected = 0;
                HistorySearchDialogEvent::None
            }
            _ => HistorySearchDialogEvent::None,
        }
    }

    pub fn render(&self, width: usize, height: usize) -> String {
        let width = width.max(20);
        let height = height.max(4);

        let visible = self.visible_indices();
        let empty_message = self.empty_message();
        let detail_lines = if let Some(message) = empty_message {
            vec![message.to_string()]
        } else {
            let mut rows = self.list_rows(&visible, width / 2, height.saturating_sub(7));
            rows.push(String::new());
            rows.push("Preview".to_string());
            rows.extend(self.preview_rows(width.saturating_sub(28).max(20)));
            rows.push(String::new());
            if let Some(prompt) = self.selected_prompt() {
                rows.push(format!(
                    "Fill preview: prompt: {}",
                    truncate_to_width(prompt, width.saturating_sub(28).max(20))
                ));
            }
            if let Some(source) = self.selected_source() {
                rows.push(format!(
                    "Source: {}",
                    truncate_to_width(&source, width.saturating_sub(28).max(20))
                ));
            }
            rows
        };
        BetterViewPanel::new("History search")
            .summary(format!("filter={} matches={}", self.query, visible.len()))
            .sections_title("History")
            .sections(vec!["Prompts".to_string(), "Preview".to_string()], 0)
            .detail_title("Preview")
            .detail_lines(detail_lines)
            .footer("Type filter | Up/Down history | Enter fill prompt | Esc close")
            .render_lines()
            .into_iter()
            .take(height)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn list_rows(&self, visible: &[usize], width: usize, limit: usize) -> Vec<String> {
        let Some(items) = &self.items else {
            return Vec::new();
        };
        let row_width = width.saturating_sub(AGE_WIDTH + 3).max(10);
        visible
            .iter()
            .take(limit)
            .enumerate()
            .filter_map(|(visible_idx, item_idx)| {
                let item = items.get(*item_idx)?;
                let marker = if visible_idx == self.selected {
                    ">"
                } else {
                    " "
                };
                let source = if item.source.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", truncate_to_width(&item.source, 24))
                };
                Some(format!(
                    "{marker} {} {}{}",
                    item.age,
                    truncate_to_width(&item.first_line, row_width),
                    source
                ))
            })
            .collect()
    }

    fn preview_rows(&self, width: usize) -> Vec<String> {
        let Some(prompt) = self.selected_prompt() else {
            return vec!["No prompt selected".to_string()];
        };

        let mut wrapped = Vec::new();
        for line in prompt.lines().filter(|line| !line.trim().is_empty()) {
            for row in textwrap::wrap(line, width.max(1)) {
                wrapped.push(row.into_owned());
            }
        }
        if wrapped.is_empty() {
            wrapped.push("(empty prompt)".to_string());
        }

        let overflow = wrapped.len() > PREVIEW_ROWS;
        let shown_count = if overflow {
            PREVIEW_ROWS.saturating_sub(1)
        } else {
            PREVIEW_ROWS
        };
        let mut lines = wrapped
            .iter()
            .take(shown_count)
            .map(|row| truncate_to_width(row, width))
            .collect::<Vec<_>>();
        if overflow {
            lines.push(format!(
                "+{} more lines",
                wrapped.len().saturating_sub(shown_count)
            ));
        }
        lines
    }

    fn empty_message(&self) -> Option<&'static str> {
        match &self.items {
            None => Some("Loading..."),
            Some(items) if items.is_empty() => Some("No saved prompts for this workspace yet"),
            Some(_) if self.visible_indices().is_empty() => Some("No matching prompts"),
            Some(_) => None,
        }
    }

    fn selected_source(&self) -> Option<String> {
        let idx = self.selected_item_index()?;
        self.items
            .as_ref()
            .and_then(|items| items.get(idx))
            .and_then(|item| (!item.source.is_empty()).then(|| item.source.clone()))
    }

    fn move_next(&mut self) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = (self.selected + 1) % len;
        }
    }

    fn move_prev(&mut self) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected == 0 {
            self.selected = len - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(len - 1);
        }
    }

    fn selected_item_index(&self) -> Option<usize> {
        self.visible_indices().get(self.selected).copied()
    }

    fn visible_indices(&self) -> Vec<usize> {
        let Some(items) = &self.items else {
            return Vec::new();
        };
        let query = self.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..items.len()).collect();
        }

        let mut exact = Vec::new();
        let mut fuzzy = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            if item.lower.contains(&query) {
                exact.push(idx);
            } else if fuzzy_match(&item.lower, &query).is_some() {
                fuzzy.push(idx);
            }
        }
        exact.extend(fuzzy);
        exact
    }
}

impl HistorySearchItem {
    fn new(entry: HistorySearchEntry, now_secs: i64) -> Self {
        let first_line = entry
            .display
            .lines()
            .next()
            .unwrap_or(entry.display.as_str())
            .to_string();
        let lower = entry.display.to_ascii_lowercase();
        let age = pad_to_width(&format_age(entry.timestamp_secs, now_secs), AGE_WIDTH);
        let source = format_source(&entry);
        Self {
            entry,
            lower,
            first_line,
            age,
            source,
        }
    }
}

fn format_source(entry: &HistorySearchEntry) -> String {
    entry
        .source_title
        .as_deref()
        .or(entry.source_cwd.as_deref())
        .or(entry.source_session_id.as_deref())
        .unwrap_or("")
        .to_string()
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn format_age(timestamp_secs: i64, now_secs: i64) -> String {
    let delta = now_secs.saturating_sub(timestamp_secs).max(0);
    if delta < 60 {
        "now".to_string()
    } else if delta < 3_600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3_600)
    } else {
        format!("{}d", delta / 86_400)
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut width = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }

    if out.len() < text.len() && max_width > 3 {
        while display_width(&out) + 3 > max_width {
            out.pop();
        }
        out.push_str("...");
    }
    out
}

fn pad_to_width(text: &str, width: usize) -> String {
    let current = display_width(text);
    if current >= width {
        truncate_to_width(text, width)
    } else {
        format!("{}{}", text, " ".repeat(width - current))
    }
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, KeyModifiers};
    use insta::assert_snapshot;

    const NOW: i64 = 1_700_000_000;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ctrl_key(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn entries() -> Vec<HistorySearchEntry> {
        vec![
            HistorySearchEntry::new("git status --short", NOW - 45),
            HistorySearchEntry::new("cargo test -p allthecodes history_search", NOW - 240),
            HistorySearchEntry::new("open model context protocol docs", NOW - 7_200),
            HistorySearchEntry::new(
                "write a multi-line prompt\nwith enough detail to preview\nand extra rows\nthat should overflow\nline five\nline six\nline seven",
                NOW - 172_800,
            ),
        ]
    }

    fn long_entries() -> Vec<HistorySearchEntry> {
        vec![
            HistorySearchEntry::new(
                "run a very long prompt that asks the model to inspect every permission overlay, every command surface, and every snapshot truncation case",
                NOW - 45,
            ),
            HistorySearchEntry::new(
                "summarize /tmp/very/long/project/path/with/many/nested/directories/and/a/file-name-that-should-not-fit.rs",
                NOW - 240,
            ),
        ]
    }

    #[test]
    fn filtering_keeps_contains_before_subsequence() {
        let mut dialog = HistorySearchDialog::from_entries(entries(), "mcp", NOW);

        assert_eq!(
            dialog.selected_prompt(),
            Some("open model context protocol docs")
        );

        dialog.handle_key(ctrl_key('u'));
        dialog.handle_key(key(KeyCode::Char('z')));
        dialog.handle_key(key(KeyCode::Char('z')));
        dialog.handle_key(key(KeyCode::Char('z')));
        assert_eq!(dialog.query(), "zzz");
        assert_eq!(dialog.selected_prompt(), None);
    }

    #[test]
    fn key_handling_edits_and_selects() {
        let mut dialog = HistorySearchDialog::from_entries(entries(), "", NOW);

        dialog.handle_key(key(KeyCode::Char('c')));
        dialog.handle_key(key(KeyCode::Char('a')));
        assert!(dialog
            .selected_prompt()
            .is_some_and(|prompt| prompt.starts_with("cargo test")));

        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.query(), "c");

        dialog.handle_key(ctrl_key('u'));
        assert_eq!(dialog.query(), "");

        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            HistorySearchDialogEvent::Selected("git status --short".to_string())
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            HistorySearchDialogEvent::Cancelled
        );
    }

    #[test]
    fn snapshot_history_search_states() {
        let wide = HistorySearchDialog::from_entries(entries(), "", NOW).render(120, 12);
        let narrow = HistorySearchDialog::from_entries(entries(), "write", NOW).render(64, 14);
        let truncated =
            HistorySearchDialog::from_entries(long_entries(), "permission", NOW).render(42, 8);
        let loading = HistorySearchDialog::loading("", NOW).render(64, 6);
        let no_history = HistorySearchDialog::from_entries(Vec::new(), "", NOW).render(64, 6);
        let no_matches = HistorySearchDialog::from_entries(entries(), "zzz", NOW).render(64, 6);

        assert_snapshot!(format!(
            "## wide\n{wide}\n\n## narrow\n{narrow}\n\n## truncated\n{truncated}\n\n## loading\n{loading}\n\n## no history\n{no_history}\n\n## no matches\n{no_matches}"
        ));
    }
}
