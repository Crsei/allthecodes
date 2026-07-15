use std::collections::BTreeMap;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextTone {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextLayerKey {
    Repo,
    Branch,
    Model,
    Plan,
    CurrentAgent,
    CurrentTool,
    ContextUsage,
    PendingPermission,
    LastError,
    /// Catch-all sticky slot for transient system notices (info / warning /
    /// error) that do not match one of the well-known state prefixes above.
    /// Holds only the most recent such notice; a new notice replaces the
    /// previous one.
    Notices,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLayerItem {
    pub key: ContextLayerKey,
    pub tone: ContextTone,
    pub label: String,
    pub value: String,
}

impl ContextLayerItem {
    pub fn new(tone: ContextTone, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: ContextLayerKey::Repo,
            tone,
            label: label.into(),
            value: value.into(),
        }
    }

    pub fn keyed(
        key: ContextLayerKey,
        tone: ContextTone,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key,
            tone,
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ContextLayerState {
    items: BTreeMap<ContextLayerKey, ContextLayerItem>,
}

impl ContextLayerState {
    pub fn upsert(&mut self, item: ContextLayerItem) {
        self.items.insert(item.key.clone(), item);
    }

    pub fn remove(&mut self, key: &ContextLayerKey) {
        self.items.remove(key);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn items(&self) -> Vec<ContextLayerItem> {
        self.items.values().cloned().collect()
    }
}

pub fn render_context_layer(
    items: &[ContextLayerItem],
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if width == 0 || items.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    spans.push(Span::styled(" info ", theme.context_info_label));
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" | ", theme.dim));
        }
        spans.push(Span::styled(
            format!("{}: ", item.label),
            style_for_label(item.tone, theme),
        ));
        spans.push(Span::styled(
            item.value.clone(),
            style_for_body(item.tone, theme),
        ));
    }

    vec![Line::from(truncate_spans(spans, width))]
}

fn style_for_label(tone: ContextTone, theme: &Theme) -> Style {
    match tone {
        ContextTone::Info => theme.context_info_label,
        ContextTone::Warning => theme.context_warning,
        ContextTone::Error => theme.context_error,
    }
}

fn style_for_body(tone: ContextTone, theme: &Theme) -> Style {
    match tone {
        ContextTone::Info => theme.context_info_text,
        ContextTone::Warning => theme.context_warning,
        ContextTone::Error => theme.context_error,
    }
}

fn truncate_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut used_width = 0usize;
    let mut out = Vec::new();
    for span in spans {
        if used_width >= max_width {
            break;
        }

        let mut kept = String::new();
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used_width + ch_width > max_width {
                break;
            }
            used_width += ch_width;
            kept.push(ch);
        }

        if !kept.is_empty() {
            out.push(Span::styled(kept, span.style));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_info_label_is_blue_but_body_is_not() {
        let theme = Theme::default();
        let lines = render_context_layer(
            &[ContextLayerItem::new(
                ContextTone::Info,
                "branch",
                "feature/agent-loop",
            )],
            80,
            &theme,
        );
        let label = lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("branch"))
            .unwrap();
        let body = lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("feature/agent-loop"))
            .unwrap();

        assert_eq!(label.style.fg, theme.context_info_label.fg);
        assert_eq!(body.style.fg, theme.context_info_text.fg);
        assert_ne!(body.style.fg, theme.context_info_label.fg);
    }

    #[test]
    fn context_layer_truncates_to_width() {
        let theme = Theme::default();
        let lines = render_context_layer(
            &[ContextLayerItem::new(
                ContextTone::Info,
                "repo",
                "a-very-long-repository-name-that-must-fit",
            )],
            24,
            &theme,
        );
        assert!(lines[0].width() <= 24);
    }
}
