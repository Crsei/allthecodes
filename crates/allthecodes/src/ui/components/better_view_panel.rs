//! Shared text renderer for Better View command and approval panels.

use crate::ui::panel_layout::PanelSizePreset;

pub(crate) const PANEL_WIDTH: usize = PanelSizePreset::BetterViewPanel.spec().min_width as usize;
pub(crate) const NAV_WIDTH: usize = 40;
const DETAIL_WIDTH: usize = PANEL_WIDTH - NAV_WIDTH - 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetterViewPanel {
    pub title: String,
    pub summary: String,
    pub sections_title: String,
    pub detail_title: String,
    pub sections: Vec<String>,
    pub selected_section: usize,
    pub detail_lines: Vec<String>,
    pub footer: String,
}

impl BetterViewPanel {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            summary: String::new(),
            sections_title: "Sections".to_string(),
            detail_title: "Detail".to_string(),
            sections: Vec::new(),
            selected_section: 0,
            detail_lines: Vec::new(),
            footer: String::new(),
        }
    }

    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn sections_title(mut self, title: impl Into<String>) -> Self {
        self.sections_title = title.into();
        self
    }

    pub fn detail_title(mut self, title: impl Into<String>) -> Self {
        self.detail_title = title.into();
        self
    }

    pub fn sections(mut self, sections: Vec<String>, selected: usize) -> Self {
        self.selected_section = selected.min(sections.len().saturating_sub(1));
        self.sections = sections;
        self
    }

    pub fn detail_lines(mut self, lines: Vec<String>) -> Self {
        self.detail_lines = lines;
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn render(&self) -> String {
        self.render_lines().join("\n")
    }

    pub fn render_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(top_border(&self.title));
        if !self.summary.trim().is_empty() {
            lines.push(panel_line(&fit(&self.summary, PANEL_WIDTH - 4)));
        }
        lines.push(separator());
        lines.push(two_col_line(&self.sections_title, &self.detail_title));

        let body_len = self.sections.len().max(self.detail_lines.len()).max(1);
        for idx in 0..body_len {
            let section = self
                .sections
                .get(idx)
                .map(|label| {
                    let marker = if idx == self.selected_section {
                        ">"
                    } else {
                        " "
                    };
                    format!("{marker} {label}")
                })
                .unwrap_or_default();
            let detail = self.detail_lines.get(idx).cloned().unwrap_or_default();
            lines.push(two_col_line(&section, &detail));
        }

        lines.push(separator());
        if !self.footer.trim().is_empty() {
            lines.push(panel_line(&fit(&self.footer, PANEL_WIDTH - 4)));
        }
        lines.push(bottom_border());
        lines
    }
}

pub fn selected_row(label: impl AsRef<str>, detail: impl AsRef<str>, selected: bool) -> String {
    let marker = if selected { ">" } else { " " };
    if detail.as_ref().is_empty() {
        format!("{marker} {}", label.as_ref())
    } else {
        format!("{marker} {:<28} {}", label.as_ref(), detail.as_ref())
    }
}

pub fn plain_row(label: impl AsRef<str>, detail: impl AsRef<str>) -> String {
    if detail.as_ref().is_empty() {
        label.as_ref().to_string()
    } else {
        format!("{:<30} {}", label.as_ref(), detail.as_ref())
    }
}

pub fn key_value_row(key: impl AsRef<str>, value: impl AsRef<str>) -> String {
    format!("{:<24} {}", key.as_ref(), value.as_ref())
}

fn top_border(title: &str) -> String {
    let title = format!(" {title} ");
    let dashes = PANEL_WIDTH.saturating_sub(title.len() + 2);
    format!("+{title}{}+", "-".repeat(dashes))
}

fn separator() -> String {
    format!("|{}|", "-".repeat(PANEL_WIDTH - 2))
}

fn bottom_border() -> String {
    format!("+{}+", "-".repeat(PANEL_WIDTH - 2))
}

fn panel_line(text: &str) -> String {
    format!("| {:<width$} |", text, width = PANEL_WIDTH - 4)
}

fn two_col_line(left: &str, right: &str) -> String {
    let left = fit(left, NAV_WIDTH);
    let right = fit(right, DETAIL_WIDTH);
    format!(
        "| {:<nav$} {:<detail$} |",
        left,
        right,
        nav = NAV_WIDTH,
        detail = DETAIL_WIDTH
    )
}

fn fit(input: &str, max: usize) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    if chars.len() <= max {
        return input.to_string();
    }
    if max <= 3 {
        return ".".repeat(max);
    }
    format!("{}...", chars[..max - 3].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    #[test]
    fn renders_sectioned_panel_with_single_footer() {
        let rendered = BetterViewPanel::new("Config")
            .summary("model=custom backend=native source=user")
            .detail_title("Status")
            .sections(vec!["Status".into(), "Model".into()], 0)
            .detail_lines(vec![
                plain_row("model", "custom current"),
                selected_row("Show effective config", "/config show", true),
            ])
            .footer("Left/Right section | Up/Down navigate | Enter select | Esc close")
            .render();

        assert!(rendered.contains("+ Config"));
        assert!(rendered.contains("> Status"));
        assert!(rendered.matches("Esc close").count() == 1);
    }

    #[test]
    fn key_value_row_formats_detail_pairs() {
        assert_eq!(
            key_value_row("model", "sonnet"),
            "model                    sonnet"
        );
    }

    #[test]
    fn snapshot_truncates_long_panel_fields() {
        let rendered = BetterViewPanel::new("Configuration With A Very Long Title That Should Fit")
            .summary("This summary is intentionally long so the panel has to abbreviate it before it reaches the fixed panel edge.")
            .sections_title("Very Long Section Heading")
            .detail_title("Very Long Detail Heading")
            .sections(
                vec![
                    "status section with a very long label".into(),
                    "model settings with a very long label".into(),
                ],
                0,
            )
            .detail_lines(vec![
                plain_row(
                    "active model",
                    "claude-sonnet-with-a-very-long-provider-and-routing-name",
                ),
                selected_row(
                    "Open effective configuration with a long action label",
                    "/config show --include-sources --include-defaults",
                    true,
                ),
            ])
            .footer("Left/Right section | Up/Down navigate | Enter select | Esc close | This tail should be truncated")
            .render();

        assert_snapshot!("better_view_panel_truncates_long_fields", rendered);
    }
}
