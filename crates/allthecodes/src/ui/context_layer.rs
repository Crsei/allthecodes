use std::collections::BTreeMap;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

    let items = prioritize_items_for_width(items, width);
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

/// Keep the semantic status and actionable notices visible when the terminal
/// is narrower than the complete context layer. A long repository path or
/// tool summary is useful decoration, but it must not consume the cells that
/// carry a permission result, error, warning, model, agent, or context value.
///
/// Items are removed only when their complete display representation does not
/// fit. The final surviving item is deliberately retained even when it is
/// wider than the terminal, so the ordinary Unicode-safe truncation path still
/// has something meaningful to show on extremely narrow terminals.
fn prioritize_items_for_width(items: &[ContextLayerItem], width: usize) -> Vec<ContextLayerItem> {
    let mut selected = items.to_vec();
    if context_line_width(&selected) <= width {
        return selected;
    }

    let mut removal_order: Vec<(u8, usize)> = selected
        .iter()
        .enumerate()
        .map(|(index, item)| (context_retention_priority(&item.key), index))
        .collect();
    removal_order.sort_by(
        |(left_priority, left_index), (right_priority, right_index)| {
            left_priority
                .cmp(right_priority)
                .then_with(|| right_index.cmp(left_index))
        },
    );

    for (_, original_index) in removal_order {
        if selected.len() <= 1 {
            break;
        }
        // The source slice is normally the BTreeMap's unique key set. Find by
        // key position indirectly through the original index's current item;
        // this keeps the helper well-defined for test slices containing
        // duplicate keys as well.
        let Some(item) = items.get(original_index) else {
            continue;
        };
        let Some(index) = selected.iter().position(|candidate| candidate == item) else {
            continue;
        };
        selected.remove(index);
        if context_line_width(&selected) <= width {
            break;
        }
    }

    selected
}

fn context_retention_priority(key: &ContextLayerKey) -> u8 {
    match key {
        ContextLayerKey::Notices => 6,
        ContextLayerKey::LastError => 5,
        ContextLayerKey::PendingPermission => 4,
        ContextLayerKey::Model | ContextLayerKey::CurrentAgent | ContextLayerKey::ContextUsage => 3,
        ContextLayerKey::Repo
        | ContextLayerKey::Branch
        | ContextLayerKey::Plan
        | ContextLayerKey::CurrentTool => 1,
    }
}

fn context_line_width(items: &[ContextLayerItem]) -> usize {
    let item_width = items.iter().fold(0usize, |width, item| {
        width
            + UnicodeWidthStr::width(item.label.as_str())
            + 2 // ": "
            + UnicodeWidthStr::width(item.value.as_str())
    });
    UnicodeWidthStr::width(" info ")
        + item_width
        + items.len().saturating_sub(1) * UnicodeWidthStr::width(" | ")
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

    #[test]
    fn context_layer_drops_decorations_before_truncating_actionable_notice() {
        let theme = Theme::default();
        let items = vec![
            ContextLayerItem::keyed(
                ContextLayerKey::Repo,
                ContextTone::Info,
                "repo",
                "/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Branch,
                ContextTone::Info,
                "branch",
                "feature/long-lived-tui-fix",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Plan,
                ContextTone::Info,
                "plan",
                "active ship the release",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::CurrentTool,
                ContextTone::Info,
                "tool",
                "Bash cargo test --workspace",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Model,
                ContextTone::Info,
                "model",
                "gpt-5.6-sol",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::CurrentAgent,
                ContextTone::Info,
                "agent",
                "Primary active",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::ContextUsage,
                ContextTone::Info,
                "context",
                "16.4k/272k (6%)",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Notices,
                ContextTone::Warning,
                "notice",
                "Permission mode set to: default",
            ),
        ];

        let lines = render_context_layer(&items, 120, &theme);
        let rendered: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(rendered.contains("model: gpt-5.6-sol"));
        assert!(rendered.contains("agent: Primary active"));
        assert!(rendered.contains("context: 16.4k/272k (6%)"));
        assert!(rendered.contains("notice: Permission mode set to: default"));
        assert!(!rendered.contains("repo:"));
        assert!(!rendered.contains("branch:"));
        assert!(!rendered.contains("plan:"));
        assert!(!rendered.contains("tool:"));
    }

    #[test]
    fn context_layer_keeps_memory_result_complete_with_a_long_repo_path() {
        let theme = Theme::default();
        let items = vec![
            ContextLayerItem::keyed(
                ContextLayerKey::Repo,
                ContextTone::Info,
                "repo",
                "/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Model,
                ContextTone::Info,
                "model",
                "gpt-5.6-sol",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::CurrentAgent,
                ContextTone::Info,
                "agent",
                "Primary active",
            ),
            ContextLayerItem::keyed(
                ContextLayerKey::Notices,
                ContextTone::Info,
                "notice",
                "Saved project memory 'e2e_test_key': v9Z",
            ),
        ];

        let lines = render_context_layer(&items, 120, &theme);
        let rendered: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(rendered.contains("notice: Saved project memory 'e2e_test_key': v9Z"));
        assert!(!rendered.contains("repo:"));
    }
}
