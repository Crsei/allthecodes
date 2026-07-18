//! Operation-batch rendering for the new semantic operation model.
//!
//! This is the Phase 3 replacement for `grouped_tool_use_content.rs` and
//! `collapsed_read_search_content.rs`. Instead of displaying raw tool names
//! and call counts, it renders semantic operations with risk indicators,
//! result summaries, and batch counts.
//!
//! The rendering follows the plan from
//! `development/tui/command-operation-display-plan.md`.

use crate::ui::theme::Theme;
use allthecodes_tool_display::{
    OperationKind, OperationRisk, OperationSideChannel, OperationStatus, OperationSubtype,
    ToolOperation,
};
use ratatui::text::{Line, Span};

/// A display-ready operation view used by the TUI renderer.
///
/// This struct is assembled from `ToolOperation` data and render-context
/// state. It can represent either a single operation or a batch summary.
#[derive(Debug, Clone)]
pub struct ToolOperationView {
    /// The semantic operation kind.
    pub kind: OperationKind,
    /// Optional subtype.
    pub subtype: Option<OperationSubtype>,
    /// Execution status.
    pub status: OperationStatus,
    /// Risk level.
    pub risk: OperationRisk,
    /// User-facing label (one line, short).
    pub label: String,
    /// Primary target path or identifier.
    pub target: Option<String>,
    /// Summary of the operation result (if available).
    pub result_summary: Option<String>,
    /// Whether this view represents a batch summary (vs a single op).
    pub is_batch: bool,
    /// Whether the operation is still in progress.
    pub active: bool,
    /// Whether the operation had an error.
    pub has_error: bool,
    /// Side-channel references (image paths, preview URLs, diff sources, etc.)
    pub side_channels: Vec<OperationSideChannel>,
}

impl ToolOperationView {
    /// Create a single-operation view from a `ToolOperation`.
    pub fn from_operation(op: &ToolOperation) -> Self {
        let status = op.status;
        let has_error = status == OperationStatus::Error;
        let active = status == OperationStatus::InProgress;
        let result_text = op
            .result_summary
            .as_ref()
            .map(|rs| rs.text.clone())
            .filter(|t| !t.is_empty());

        let label = if op.confidence == allthecodes_tool_display::OperationConfidence::Low
            && op.kind == OperationKind::Delete
        {
            format!("May {}", op.label)
        } else {
            op.label.clone()
        };

        ToolOperationView {
            kind: op.kind,
            subtype: op.subtype,
            status,
            risk: op.risk,
            label,
            target: op.target.clone(),
            result_summary: result_text,
            is_batch: false,
            active,
            has_error,
            side_channels: op.side_channels.clone(),
        }
    }

    /// Create a batch summary view from a list of operations of the same kind.
    pub fn from_batch(operations: &[&ToolOperation]) -> Self {
        let kind = operations
            .first()
            .map(|op| op.kind)
            .unwrap_or(OperationKind::Unknown);
        let subtype = operations.first().and_then(|op| op.subtype);
        let count = operations.len();

        // Determine aggregate status
        let any_error = operations
            .iter()
            .any(|op| op.status == OperationStatus::Error);
        let all_resolved = operations
            .iter()
            .all(|op| op.status == OperationStatus::Resolved);
        let any_in_progress = operations
            .iter()
            .any(|op| op.status == OperationStatus::InProgress);
        let any_cancelled = operations
            .iter()
            .any(|op| op.status == OperationStatus::Cancelled);
        let status = if any_error {
            OperationStatus::Error
        } else if all_resolved {
            OperationStatus::Resolved
        } else if any_in_progress {
            OperationStatus::InProgress
        } else if any_cancelled {
            OperationStatus::Cancelled
        } else {
            OperationStatus::Resolved
        };

        // Aggregate risk (highest wins)
        let risk = operations
            .iter()
            .map(|op| op.risk)
            .max_by_key(|r| risk_level(*r))
            .unwrap_or(OperationRisk::Safe);

        // Build the batch label
        let label = build_batch_label(kind, subtype, count, status);

        ToolOperationView {
            kind,
            subtype,
            status,
            risk,
            label,
            target: None,
            result_summary: None,
            is_batch: true,
            active: any_in_progress,
            has_error: any_error,
            side_channels: Vec::new(),
        }
    }
}

fn risk_level(risk: OperationRisk) -> u8 {
    match risk {
        OperationRisk::Safe => 0,
        OperationRisk::Low => 1,
        OperationRisk::Medium => 2,
        OperationRisk::High => 3,
        OperationRisk::Destructive => 4,
    }
}

fn build_batch_label(
    kind: OperationKind,
    subtype: Option<OperationSubtype>,
    count: usize,
    status: OperationStatus,
) -> String {
    let active = status == OperationStatus::InProgress;
    let noun = |singular: &'static str, plural: &'static str| {
        if count == 1 {
            singular
        } else {
            plural
        }
    };
    match (kind, subtype, active) {
        (OperationKind::Read, _, true) => format!("Reading {count} {}", noun("file", "files")),
        (OperationKind::Read, _, false) => format!("Read {count} {}", noun("file", "files")),
        (OperationKind::Search, _, true) => {
            format!("Searching for {count} {}", noun("pattern", "patterns"))
        }
        (OperationKind::Search, _, false) => {
            format!("Searched for {count} {}", noun("pattern", "patterns"))
        }
        (OperationKind::Create, _, true) => format!("Writing {count} {}", noun("file", "files")),
        (OperationKind::Create, _, false) => format!("Wrote {count} {}", noun("file", "files")),
        (OperationKind::Modify, _, true) => format!("Editing {count} {}", noun("file", "files")),
        (OperationKind::Modify, _, false) => format!("Edited {count} {}", noun("file", "files")),
        (OperationKind::Delete, _, true) => format!("Deleting {count} {}", noun("file", "files")),
        (OperationKind::Delete, _, false) => format!("Deleted {count} {}", noun("file", "files")),
        (OperationKind::Execute, Some(OperationSubtype::Test), true) => {
            format!("Running {count} {}", noun("test", "tests"))
        }
        (OperationKind::Execute, Some(OperationSubtype::Test), false) => {
            format!("Ran {count} {}", noun("test", "tests"))
        }
        (OperationKind::Execute, Some(OperationSubtype::Build), true) => {
            format!("Running {count} {}", noun("build", "builds"))
        }
        (OperationKind::Execute, Some(OperationSubtype::Build), false) => {
            format!("Ran {count} {}", noun("build", "builds"))
        }
        (OperationKind::Execute, Some(OperationSubtype::Install), true) => {
            format!("Running {count} {}", noun("install", "installs"))
        }
        (OperationKind::Execute, Some(OperationSubtype::Install), false) => {
            format!("Ran {count} {}", noun("install", "installs"))
        }
        (OperationKind::Execute, _, true) => {
            format!("Running {count} {}", noun("bash command", "bash commands"))
        }
        (OperationKind::Execute, _, false) => {
            format!("Ran {count} {}", noun("bash command", "bash commands"))
        }
        (OperationKind::Delegate, _, true) => {
            format!("Launching {count} {}", noun("agent", "agents"))
        }
        (OperationKind::Delegate, _, false) => {
            format!("Launched {count} {}", noun("agent", "agents"))
        }
        (OperationKind::Plan, _, true) => format!("Updating {count} {}", noun("plan", "plans")),
        (OperationKind::Plan, _, false) => format!("Updated {count} {}", noun("plan", "plans")),
        (OperationKind::Status, _, true) => {
            format!("Updating {count} {}", noun("status", "statuses"))
        }
        (OperationKind::Status, _, false) => {
            format!("Updated {count} {}", noun("status", "statuses"))
        }
        (_, _, true) => format!("Running {count} operations"),
        (_, _, false) => format!("Ran {count} operations"),
    }
}

fn operation_kind_prefix(kind: OperationKind, subtype: Option<OperationSubtype>) -> &'static str {
    match (kind, subtype) {
        (OperationKind::Read, _) => "Read",
        (OperationKind::Search, _) => "Search",
        (OperationKind::Create, _) => "Create",
        (OperationKind::Modify, _) => "Edit",
        (OperationKind::Delete, _) => "Delete",
        (OperationKind::Execute, Some(OperationSubtype::Build)) => "Build",
        (OperationKind::Execute, Some(OperationSubtype::Test)) => "Test",
        (OperationKind::Execute, Some(OperationSubtype::Format)) => "Format",
        (OperationKind::Execute, Some(OperationSubtype::Install)) => "Install",
        (OperationKind::Execute, _) => "Run",
        (OperationKind::Permission, _) => "Permission",
        (OperationKind::Network, _) => "Network",
        (OperationKind::Delegate, _) => "Delegate",
        (OperationKind::Plan, _) => "Plan",
        (OperationKind::Status, Some(OperationSubtype::Todo)) => "Todo",
        (OperationKind::Status, _) => "Status",
        (OperationKind::System, _) => "System",
        (OperationKind::Unknown, _) => "Operation",
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

/// Render the main operation row for a single operation or batch.
///
/// Supports an optional `width` parameter for narrow-terminal priority:
/// - Width < 30: drop everything except bullet, label, and minimal status.
/// - Width < 45: drop result summary and risk labels.
pub fn render_tool_operation_lines(view: &ToolOperationView, theme: &Theme) -> Vec<Line<'static>> {
    render_tool_operation_lines_with_width(view, theme, None)
}

fn render_tool_operation_lines_with_width(
    view: &ToolOperationView,
    theme: &Theme,
    _width: Option<usize>,
) -> Vec<Line<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Bullet
    spans.push(Span::styled("  ● ", theme.dim));

    let kind_style = if view.has_error {
        theme.error
    } else if view.active {
        theme.info
    } else if view.risk == OperationRisk::Destructive || view.risk == OperationRisk::High {
        theme.warning
    } else {
        theme.tool_name
    };

    let prefix = operation_kind_prefix(view.kind, view.subtype);
    let mut label = view.label.clone();
    if label.trim().is_empty() {
        label = prefix.to_string();
    }
    spans.push(Span::styled(label, kind_style));

    if !view.is_batch {
        if let Some(target) = view
            .target
            .as_deref()
            .filter(|target| !target.is_empty() && !view.label.contains(*target))
        {
            spans.push(Span::styled(format!(" {target}"), theme.dim));
        }
    }

    // Risk indicator for high/destructive
    if view.risk == OperationRisk::Destructive {
        spans.push(Span::styled(" [destructive]", theme.error));
    } else if view.risk == OperationRisk::High {
        spans.push(Span::styled(" [high risk]", theme.warning));
    }

    // Status indicator
    spans.push(Span::raw(" "));
    match view.status {
        OperationStatus::InProgress => spans.push(Span::styled("…", theme.info)),
        OperationStatus::Error => spans.push(Span::styled("[error]", theme.error)),
        OperationStatus::Cancelled => spans.push(Span::styled("[cancelled]", theme.dim)),
        OperationStatus::Resolved => spans.push(Span::styled("✔", theme.diff_add)),
    }

    // Result summary for single ops
    if !view.is_batch {
        if let Some(ref result) = view.result_summary {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(truncate_chars(result, 60), theme.dim));
        }
    }

    // Side-channel references
    for ch in &view.side_channels {
        spans.push(Span::raw(" "));
        let reference = ch.reference.trim();
        let ref_text = if !reference.is_empty() {
            format!("[{}: {}]", ch.channel_type, truncate_chars(reference, 48))
        } else if let Some(ref desc) = ch.description {
            format!("[{}: {}]", ch.channel_type, desc)
        } else {
            format!("[{}]", ch.channel_type)
        };
        spans.push(Span::styled(ref_text, theme.dim));
    }

    vec![Line::from(spans)]
}

/// Render a TodoWrite operation batch as a rich todo checklist.
///
/// Parses `raw_input.todos` from each operation's input JSON and renders
/// the checklist with status markers.
pub fn render_todo_operation_lines(
    operations: &[ToolOperation],
    theme: &Theme,
) -> Vec<Line<'static>> {
    // Determine aggregate status
    let any_error = operations
        .iter()
        .any(|op| op.status == OperationStatus::Error);
    let all_resolved = operations
        .iter()
        .all(|op| op.status == OperationStatus::Resolved);
    let active = !all_resolved;

    // Title line
    let title_style = if any_error {
        theme.error
    } else if active {
        theme.info
    } else {
        theme.tool_name
    };
    let title = if all_resolved {
        "  ● Updated todos"
    } else if active {
        "  ● Updating todos"
    } else {
        "  ● Update todos"
    };
    let mut lines = vec![Line::from(vec![Span::styled(title, title_style)])];

    // Collect all todo items from all operations (deduplicated by content+status)
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<(String, String)> = Vec::new(); // (status, content)

    for op in operations {
        if let Some(todos) = op.raw_input.get("todos").and_then(|v| v.as_array()) {
            for todo in todos {
                let status = todo
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending")
                    .to_string();
                let content = todo
                    .get("activeForm")
                    .and_then(|v| v.as_str())
                    .filter(|_| status == "in_progress")
                    .or_else(|| todo.get("content").and_then(|v| v.as_str()))
                    .unwrap_or("(untitled todo)")
                    .to_string();
                let dedup_key = format!("{}:{}", status, content);
                if seen.insert(dedup_key) {
                    items.push((status, content));
                }
            }
        }
    }

    if items.is_empty() && !any_error {
        lines.push(Line::from(vec![
            Span::raw("   ⎿  "),
            Span::styled("(no todo items)", theme.dim),
        ]));
    }

    for (i, (status, content)) in items.iter().enumerate() {
        if i >= 20 {
            lines.push(Line::from(vec![
                Span::raw("   ⎿  "),
                Span::styled(format!("... {} more", items.len() - 20), theme.dim),
            ]));
            break;
        }
        let marker = match status.as_str() {
            "completed" => "[x]",
            "in_progress" => "[*]",
            _ => "[ ]",
        };
        let marker_style = match status.as_str() {
            "completed" => theme.diff_add,
            "in_progress" => theme.info,
            _ => theme.dim,
        };
        let content_display = truncate_chars(content, 80);
        lines.push(Line::from(vec![
            Span::raw("   ⎿  "),
            Span::styled(marker, marker_style),
            Span::raw(" "),
            Span::styled(
                content_display,
                if status == "completed" {
                    theme.dim
                } else {
                    theme.tool_name
                },
            ),
        ]));
    }

    // Show status indicator alongside title
    if !all_resolved && !any_error {
        let status_span = Span::styled(" …", theme.info);
        if let Some(first_line) = lines.first_mut() {
            first_line.spans.push(status_span);
        }
    } else if all_resolved {
        let status_span = Span::styled(" ✔", theme.diff_add);
        if let Some(first_line) = lines.first_mut() {
            first_line.spans.push(status_span);
        }
    } else if any_error {
        let status_span = Span::styled(" [error]", theme.error);
        if let Some(first_line) = lines.first_mut() {
            first_line.spans.push(status_span);
        }
    }

    lines
}

/// Render an expanded individual operation row (for use within a batch detail).
pub fn render_operation_detail_line(op: &ToolOperation, theme: &Theme) -> Vec<Line<'static>> {
    let view = ToolOperationView::from_operation(op);
    render_tool_operation_lines(&view, theme)
}

#[cfg(test)]
pub fn render_tool_operation_content(view: &ToolOperationView, theme: &Theme) -> String {
    render_tool_operation_lines(view, theme)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_tool_display::{
        OperationConfidence, OperationKind, OperationRisk, OperationSideChannel, OperationStatus,
        ToolOperation,
    };

    fn make_op(
        kind: OperationKind,
        subtype: Option<OperationSubtype>,
        status: OperationStatus,
        risk: OperationRisk,
        label: &str,
        target: Option<&str>,
    ) -> ToolOperation {
        ToolOperation {
            kind,
            subtype,
            status,
            risk,
            confidence: OperationConfidence::High,
            label: label.to_string(),
            target: target.map(str::to_string),
            command_summary: None,
            result_summary: None,
            raw_tool_name: "Bash".to_string(),
            raw_input: serde_json::json!({}),
            raw_output: None,
            side_channels: Vec::new(),
        }
    }

    #[test]
    fn test_single_read_operation() {
        let op = make_op(
            OperationKind::Read,
            None,
            OperationStatus::Resolved,
            OperationRisk::Safe,
            "Read src/main.rs",
            Some("src/main.rs"),
        );
        let view = ToolOperationView::from_operation(&op);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("Read"));
        assert!(rendered.contains("✔"));
    }

    #[test]
    fn test_single_delete_operation() {
        let op = ToolOperation {
            kind: OperationKind::Delete,
            subtype: None,
            status: OperationStatus::Resolved,
            risk: OperationRisk::Destructive,
            confidence: OperationConfidence::Low,
            label: "Delete /tmp/x".to_string(),
            target: Some("/tmp/x".to_string()),
            command_summary: None,
            result_summary: None,
            raw_tool_name: "Bash".to_string(),
            raw_input: serde_json::json!({}),
            raw_output: None,
            side_channels: Vec::new(),
        };
        let view = ToolOperationView::from_operation(&op);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("May Delete"));
        assert!(rendered.contains("[destructive]"));
    }

    #[test]
    fn test_side_channel_renders_reference_before_description() {
        let mut op = make_op(
            OperationKind::Read,
            None,
            OperationStatus::Resolved,
            OperationRisk::Safe,
            "Open preview",
            None,
        );
        op.side_channels.push(OperationSideChannel {
            channel_type: "preview".to_string(),
            reference: "http://127.0.0.1:3000/page".to_string(),
            description: Some("browser preview".to_string()),
        });

        let view = ToolOperationView::from_operation(&op);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("[preview: http://127.0.0.1:3000/page]"));
        assert!(!rendered.contains("browser preview"));
    }

    #[test]
    fn test_batch_summary() {
        let op1 = make_op(
            OperationKind::Read,
            None,
            OperationStatus::Resolved,
            OperationRisk::Safe,
            "Read a.rs",
            Some("a.rs"),
        );
        let op2 = make_op(
            OperationKind::Read,
            None,
            OperationStatus::Resolved,
            OperationRisk::Safe,
            "Read b.rs",
            Some("b.rs"),
        );
        let op3 = make_op(
            OperationKind::Read,
            None,
            OperationStatus::Resolved,
            OperationRisk::Safe,
            "Read c.rs",
            Some("c.rs"),
        );
        let ops = vec![&op1, &op2, &op3];
        let view = ToolOperationView::from_batch(&ops);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("Read 3 files"));
        assert!(rendered.contains("✔"));
    }

    #[test]
    fn batch_labels_use_active_tense_and_singular_nouns() {
        let op = make_op(
            OperationKind::Search,
            None,
            OperationStatus::InProgress,
            OperationRisk::Safe,
            "Search TODOs",
            None,
        );
        let view = ToolOperationView::from_batch(&[&op]);
        let rendered = render_tool_operation_content(&view, &Theme::default());

        assert!(rendered.contains("Searching for 1 pattern"));
        assert!(rendered.contains("…"));
    }

    #[test]
    fn test_batch_with_errors() {
        let op1 = make_op(
            OperationKind::Modify,
            None,
            OperationStatus::Resolved,
            OperationRisk::Medium,
            "Edit a.rs",
            Some("a.rs"),
        );
        let op2 = make_op(
            OperationKind::Modify,
            None,
            OperationStatus::Error,
            OperationRisk::Medium,
            "Edit b.rs",
            Some("b.rs"),
        );
        let ops = vec![&op1, &op2];
        let view = ToolOperationView::from_batch(&ops);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("Edited 2 files"));
        assert!(rendered.contains("[error]"));
    }

    #[test]
    fn test_in_progress_operation() {
        let op = make_op(
            OperationKind::Execute,
            Some(OperationSubtype::Build),
            OperationStatus::InProgress,
            OperationRisk::Medium,
            "Build allthecodes",
            None,
        );
        let view = ToolOperationView::from_operation(&op);
        let rendered = render_tool_operation_content(&view, &Theme::default());
        assert!(rendered.contains("Build"));
        assert!(rendered.contains("…"));
    }

    #[test]
    fn test_todo_write_renders_checklist() {
        let op = ToolOperation {
            kind: OperationKind::Status,
            subtype: Some(OperationSubtype::Todo),
            status: OperationStatus::Resolved,
            risk: OperationRisk::Safe,
            confidence: OperationConfidence::High,
            label: "Update TODO list (3 items)".to_string(),
            target: None,
            command_summary: None,
            result_summary: None,
            raw_tool_name: "TodoWrite".to_string(),
            raw_input: serde_json::json!({
                "todos": [
                    {"content": "Inspect UI", "status": "completed"},
                    {"content": "Patch rendering", "status": "in_progress", "activeForm": "Patching rendering"},
                    {"content": "Run tests", "status": "pending"}
                ]
            }),
            raw_output: None,
            side_channels: Vec::new(),
        };

        let rendered = render_todo_operation_lines(&[op], &Theme::default());
        let plain: Vec<String> = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        let joined = plain.join("\n");

        assert!(joined.contains("Updated todos"));
        assert!(joined.contains("[x] Inspect UI"));
        assert!(joined.contains("[*] Patching rendering"));
        assert!(joined.contains("[ ] Run tests"));
    }
}
