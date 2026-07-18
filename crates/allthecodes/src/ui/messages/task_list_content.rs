//! Task-list rendering shared by the prompt spinner and task surfaces.
//!
//! This is the Rust counterpart to Claude Code's `TaskListV2`: it keeps the
//! task state out of the chat transcript and renders an expanded task view
//! using compact status glyphs instead.

use std::cmp::Reverse;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::tasks::TaskState;
use crate::ui::theme::Theme;

/// A task snapshot suitable for the compact expanded task list.
///
/// The command-surface adapter owns conversion from the task-store domain
/// type. Keeping this rendering record small means the renderer remains
/// independent from task-store locking and persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskListItem {
    pub(crate) title: String,
    pub(crate) state: TaskState,
    pub(crate) blocked_by: Vec<String>,
    /// Unix timestamp in seconds of the most recent task mutation.
    pub(crate) updated_at: i64,
}

const MAX_DISPLAYED_TASKS: usize = 8;
const RECENT_COMPLETION_SECONDS: i64 = 30;

/// Render a TaskListV2-style expanded task list.
///
/// Completed tasks updated in the last thirty seconds stay visible first,
/// followed by in-progress work, open work, then older completions. The
/// resulting list is bounded so a large plan cannot consume the message pane.
pub(crate) fn render_task_list_lines(
    tasks: &[TaskListItem],
    theme: &Theme,
    now_unix_secs: i64,
) -> Vec<Line<'static>> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let done = tasks
        .iter()
        .filter(|task| matches!(task.state, TaskState::Succeeded))
        .count();
    let in_progress = tasks
        .iter()
        .filter(|task| matches!(task.state, TaskState::Running))
        .count();
    let open = tasks
        .iter()
        .filter(|task| matches!(task.state, TaskState::Pending))
        .count();

    let mut ordered = tasks.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|task| task_sort_key(task, now_unix_secs));

    let shown = ordered.len().min(MAX_DISPLAYED_TASKS);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "  {} tasks ({} done, {} in progress, {} open)",
            tasks.len(),
            done,
            in_progress,
            open
        ),
        theme.dim,
    ))];
    lines.extend(
        ordered
            .iter()
            .take(shown)
            .map(|task| render_task_line(task, theme)),
    );

    if shown < ordered.len() {
        let hidden = &ordered[shown..];
        let hidden_in_progress = hidden
            .iter()
            .filter(|task| matches!(task.state, TaskState::Running))
            .count();
        let hidden_pending = hidden
            .iter()
            .filter(|task| matches!(task.state, TaskState::Pending))
            .count();
        let hidden_completed = hidden.iter().filter(|task| is_terminal(task.state)).count();
        lines.push(Line::from(Span::styled(
            format!(
                "  … +{hidden_in_progress} in progress, {hidden_pending} pending, {hidden_completed} completed"
            ),
            theme.dim,
        )));
    }

    lines
}

fn task_sort_key(task: &TaskListItem, now_unix_secs: i64) -> (u8, Reverse<i64>, String) {
    let recently_completed = matches!(task.state, TaskState::Succeeded)
        && now_unix_secs.saturating_sub(task.updated_at) <= RECENT_COMPLETION_SECONDS;
    let priority: u8 = if recently_completed {
        0
    } else {
        match task.state {
            TaskState::Running => 1,
            TaskState::Pending => 2,
            TaskState::Succeeded | TaskState::Failed | TaskState::Canceled => 3,
        }
    };
    let blocked_rank: u8 = if task.blocked_by.is_empty() { 0 } else { 1 };
    (
        priority.saturating_mul(2).saturating_add(blocked_rank),
        Reverse(task.updated_at),
        task.title.clone(),
    )
}

fn render_task_line(task: &TaskListItem, theme: &Theme) -> Line<'static> {
    let (marker, marker_style, content_style) = match task.state {
        TaskState::Succeeded => (
            "✔",
            theme.diff_add,
            theme
                .dim
                .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT),
        ),
        TaskState::Running => (
            "◼",
            theme.info.add_modifier(Modifier::BOLD),
            theme.tool_name,
        ),
        TaskState::Pending => ("◻", Style::default(), Style::default()),
        TaskState::Failed => ("◻", theme.error, theme.error),
        TaskState::Canceled => ("◻", theme.dim, theme.dim.add_modifier(Modifier::DIM)),
    };

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(marker, marker_style),
        Span::raw(" "),
        Span::styled(task.title.clone(), content_style),
    ];
    if !task.blocked_by.is_empty() {
        let dependencies = task
            .blocked_by
            .iter()
            .map(|dependency| {
                if dependency.starts_with('#') {
                    dependency.clone()
                } else {
                    format!("#{dependency}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        spans.push(Span::styled(
            format!(" › blocked by {dependencies}"),
            theme.dim,
        ));
    }
    Line::from(spans)
}

fn is_terminal(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Succeeded | TaskState::Failed | TaskState::Canceled
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, state: TaskState, updated_at: i64) -> TaskListItem {
        TaskListItem {
            title: title.to_string(),
            state,
            blocked_by: Vec::new(),
            updated_at,
        }
    }

    fn plain(lines: Vec<Line<'static>>) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn maps_task_statuses_to_tasklistv2_glyphs_and_blocked_suffix() {
        let mut pending = item("Publish release", TaskState::Pending, 90);
        pending.blocked_by = vec!["2".to_string(), "#3".to_string()];
        let rendered = plain(render_task_list_lines(
            &[
                item("Write tests", TaskState::Succeeded, 100),
                item("Build package", TaskState::Running, 99),
                pending,
            ],
            &Theme::default(),
            110,
        ));

        assert!(rendered.iter().any(|line| line.contains("✔ Write tests")));
        assert!(rendered.iter().any(|line| line.contains("◼ Build package")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("◻ Publish release › blocked by #2, #3")));
    }

    #[test]
    fn prioritizes_recent_completion_then_running_then_open_work() {
        let rendered = plain(render_task_list_lines(
            &[
                item("older complete", TaskState::Succeeded, 1),
                item("open", TaskState::Pending, 100),
                item("active", TaskState::Running, 101),
                item("recent complete", TaskState::Succeeded, 105),
            ],
            &Theme::default(),
            110,
        ));

        assert!(rendered[1].contains("recent complete"));
        assert!(rendered[2].contains("active"));
        assert!(rendered[3].contains("open"));
        assert!(rendered[4].contains("older complete"));
    }

    #[test]
    fn truncates_large_lists_with_hidden_status_summary() {
        let tasks = (0..10)
            .map(|index| item(&format!("task-{index}"), TaskState::Pending, index))
            .collect::<Vec<_>>();
        let rendered = plain(render_task_list_lines(&tasks, &Theme::default(), 100));

        assert_eq!(rendered.len(), 10);
        assert!(rendered
            .last()
            .unwrap()
            .contains("… +0 in progress, 2 pending, 0 completed"));
    }
}
