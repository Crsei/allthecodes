//! Tool activity and progress rendering.

use ratatui::text::{Line, Span};

use super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ToolState {
    pub fn label(self) -> &'static str {
        match self {
            ToolState::Queued => "queued",
            ToolState::Running => "running",
            ToolState::Succeeded => "succeeded",
            ToolState::Failed => "failed",
            ToolState::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivity {
    pub name: String,
    pub user_facing_name: Option<String>,
    pub arguments_summary: Option<String>,
    pub state: ToolState,
    pub summary: String,
    pub error_summary: Option<String>,
    pub elapsed_ms: u64,
    pub progress: Option<(usize, usize)>,
    pub output_lines: usize,
    pub output_preview: Vec<String>,
}

impl ToolActivity {
    pub fn new(name: impl Into<String>, state: ToolState) -> Self {
        Self {
            name: name.into(),
            user_facing_name: None,
            arguments_summary: None,
            state,
            summary: String::new(),
            error_summary: None,
            elapsed_ms: 0,
            progress: None,
            output_lines: 0,
            output_preview: Vec::new(),
        }
    }

    pub fn from_tool_use(tool_name: &str, input: &str, state: ToolState) -> Self {
        let (user_facing_name, arguments_summary) = tool_label_and_args(tool_name, input);
        Self {
            name: tool_name.to_string(),
            user_facing_name: Some(user_facing_name),
            arguments_summary,
            state,
            summary: String::new(),
            error_summary: None,
            elapsed_ms: 0,
            progress: None,
            output_lines: 0,
            output_preview: Vec::new(),
        }
    }

    pub fn compact_line(&self) -> String {
        let mut parts = vec![
            format!("  ● {}", self.display_call()),
            format!("[{}]", self.state.label()),
            format!("worked for {}", format_elapsed(self.elapsed_ms)),
        ];

        if let Some(progress) = self.progress_text() {
            parts.push(progress);
        }

        if let Some(error) = self
            .error_summary
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            parts.push(format!("error: {error}"));
        } else if !self.summary.is_empty() {
            parts.push(self.summary.clone());
        }

        if self.output_lines > 0 {
            parts.push(format!("{} output lines", self.output_lines));
        }

        parts.join(" | ")
    }

    /// Render a theme-styled compact line for display in ratatui buffers.
    ///
    /// Uses theme colors for status, tool name, errors, and progress.
    pub fn compact_styled_line(&self, theme: &Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();

        let status_style = match self.state {
            ToolState::Queued => theme.dim,
            ToolState::Running => theme.info,
            ToolState::Succeeded => theme.diff_add,
            ToolState::Failed => theme.error,
            ToolState::Cancelled => theme.warning,
        };
        let name_style = match self.state {
            ToolState::Succeeded => theme.diff_add,
            _ => theme.tool_name,
        };
        let display = self.display_call();
        spans.push(Span::styled("  ● ", theme.dim));
        spans.push(Span::styled(display, name_style));
        spans.push(Span::raw(" | "));

        spans.push(Span::styled(
            format!("[{}]", self.state.label()),
            status_style,
        ));
        spans.push(Span::raw(" | "));

        spans.push(Span::styled(
            format!("worked for {}", format_elapsed(self.elapsed_ms)),
            theme.dim,
        ));

        if let Some((done, total)) = self.progress {
            spans.push(Span::raw(" | "));
            let ratio = if total == 0 {
                0.0
            } else {
                done.min(total) as f64 / total as f64
            };
            spans.push(Span::styled(
                format!("{}/{} [", done.min(total), total),
                theme.dim,
            ));
            spans.extend(render_styled_progress_bar(ratio, 10, theme).spans);
            spans.push(Span::styled("]".to_string(), theme.dim));
        }

        let has_error = self
            .error_summary
            .as_ref()
            .is_some_and(|value| !value.is_empty());
        if has_error {
            if let Some(error) = &self.error_summary {
                spans.push(Span::raw(" | "));
                spans.push(Span::styled(format!("error: {error}"), theme.error));
            }
        } else if !self.summary.is_empty() {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(self.summary.clone(), theme.dim));
        }

        if self.output_lines > 0 {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                format!("{} output lines", self.output_lines),
                theme.dim,
            ));
        }

        Line::from(spans)
    }

    #[cfg(test)]
    pub fn transcript_block(&self) -> String {
        let mut lines = vec![
            format!("tool: {}", self.name),
            format!("display: {}", self.display_call()),
            format!("state: {}", self.state.label()),
            format!("elapsed: {}", format_elapsed(self.elapsed_ms)),
        ];
        if let Some(progress) = self.progress_text() {
            lines.push(format!("progress: {progress}"));
        }
        if !self.summary.is_empty() {
            lines.push(format!("summary: {}", self.summary));
        }
        if let Some(error) = self
            .error_summary
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("error: {error}"));
        }
        lines.extend([
            format!("output lines: {}", self.output_lines),
            format!(
                "output preview: {}",
                if self.output_preview.is_empty() {
                    "none".to_string()
                } else {
                    self.output_preview.join(" | ")
                }
            ),
        ]);
        lines.join("\n")
    }

    pub fn display_call(&self) -> String {
        let name = self
            .user_facing_name
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.name);
        match self
            .arguments_summary
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            Some(args) => format!("{name}({args})"),
            None => name.to_string(),
        }
    }

    fn progress_text(&self) -> Option<String> {
        let (done, total) = self.progress?;
        let ratio = if total == 0 {
            0.0
        } else {
            done.min(total) as f64 / total as f64
        };
        Some(format!(
            "{}/{} [{}]",
            done.min(total),
            total,
            render_plain_progress_bar(ratio, 10)
        ))
    }
}

pub fn render_grouped_activity(activities: &[ToolActivity]) -> String {
    render_grouped_styled_activity(activities, &Theme::default())
        .into_iter()
        .map(line_to_plain)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render grouped tool activities as styled ratatui lines.
pub fn render_grouped_styled_activity(
    activities: &[ToolActivity],
    theme: &Theme,
) -> Vec<Line<'static>> {
    if activities.is_empty() {
        return vec![Line::from(Span::styled("No tool activity", theme.dim))];
    }

    activities
        .iter()
        .map(|a| a.compact_styled_line(theme))
        .collect()
}

fn line_to_plain(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

fn render_styled_progress_bar(ratio: f64, width: usize, theme: &Theme) -> Line<'static> {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    Line::from(vec![
        Span::styled("█".repeat(filled), theme.progress_fill),
        Span::styled(" ".repeat(empty), theme.progress_empty),
    ])
}

fn tool_label_and_args(tool_name: &str, input: &str) -> (String, Option<String>) {
    let name = user_facing_tool_name(tool_name);
    let args = summarize_tool_input(input);
    (name, args)
}

pub fn user_facing_tool_name(tool_name: &str) -> String {
    match tool_name {
        "read_file" | "Read" => "Read".to_string(),
        "edit_file" | "file_edit" | "FileEdit" | "Edit" | "MultiEdit" | "NotebookEdit"
        | "file_write" | "FileWrite" | "Write" => "Edit".to_string(),
        "bash" | "Bash" => "Bash".to_string(),
        "powershell" | "PowerShell" => "PowerShell".to_string(),
        "grep" | "Grep" => "Search".to_string(),
        "glob" | "Glob" => "Glob".to_string(),
        "web_fetch" | "WebFetch" => "Fetch".to_string(),
        "todo_write" | "TodoWrite" => "Todo".to_string(),
        "Task" | "Agent" => "Agent".to_string(),
        _ => tool_name.to_string(),
    }
}

fn summarize_tool_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return summarize_json_input(&value);
    }

    Some(truncate(trimmed, 96))
}

fn summarize_json_input(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    let preferred = [
        "path",
        "file_path",
        "command",
        "pattern",
        "url",
        "query",
        "description",
    ];
    let mut parts = Vec::new();
    for key in preferred {
        if let Some(value) = object.get(key).and_then(json_scalar) {
            parts.push(format!("{key}={}", truncate(&value, 48)));
        }
    }
    if parts.is_empty() {
        for (key, value) in object.iter().take(3) {
            if let Some(value) = json_scalar(value) {
                parts.push(format!("{key}={}", truncate(&value, 48)));
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn format_elapsed(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn render_plain_progress_bar(ratio: f64, width: usize) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), " ".repeat(empty))
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else if max_chars <= 3 {
        ".".repeat(max_chars)
    } else {
        format!(
            "{}...",
            value.chars().take(max_chars - 3).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    #[test]
    fn snapshot_grouped_activity_states() {
        let mut running = ToolActivity::from_tool_use(
            "bash",
            r#"{"command":"cargo test -p allthecodes"}"#,
            ToolState::Running,
        );
        running.summary = "running tests".to_string();
        running.elapsed_ms = 12_400;
        running.progress = Some((3, 5));
        running.output_lines = 18;
        running.output_preview = vec!["running 18 tests".to_string()];

        let mut failed = ToolActivity::from_tool_use(
            "read_file",
            r#"{"path":"src/ui/app.rs"}"#,
            ToolState::Failed,
        );
        failed.elapsed_ms = 900;
        failed.error_summary = Some("permission denied".to_string());

        let mut cancelled = ToolActivity::new("unknown_tool", ToolState::Cancelled);
        cancelled.summary = "user interrupted".to_string();

        let queued = ToolActivity::from_tool_use("todo_write", "", ToolState::Queued);

        assert_snapshot!(format!(
            "## grouped\n{}\n\n## transcript\n{}\n\n## empty\n{}",
            render_grouped_activity(&[running.clone(), failed.clone(), cancelled, queued]),
            running.transcript_block(),
            render_grouped_activity(&[])
        ));
    }

    #[test]
    fn json_inputs_are_summarized_with_preferred_keys() {
        let activity = ToolActivity::from_tool_use(
            "grep",
            r#"{"pattern":"HistorySearch","path":"src/ui","extra":"ignored"}"#,
            ToolState::Running,
        );

        assert_eq!(
            activity.display_call(),
            "Search(path=src/ui, pattern=HistorySearch)"
        );
    }

    #[test]
    fn compact_line_includes_progress_summary_and_output_count() {
        let mut activity =
            ToolActivity::from_tool_use("bash", r#"{"command":"cargo check"}"#, ToolState::Running);
        activity.elapsed_ms = 1_250;
        activity.progress = Some((2, 4));
        activity.output_lines = 7;

        let line = activity.compact_line();

        assert!(line.contains("[running]"));
        assert!(line.contains("2/4"));
        assert!(line.contains("7 output lines"));
    }
}
