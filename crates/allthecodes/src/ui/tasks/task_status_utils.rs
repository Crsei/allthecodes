//! Shared task status formatting helpers.

use super::TaskStatus;
use super::{TaskKind, TaskState};
use crate::ui::theme::ThemeColors;

pub fn state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "pending",
        TaskState::Running => "running",
        TaskState::Succeeded => "succeeded",
        TaskState::Failed => "failed",
        TaskState::Canceled => "canceled",
    }
}

pub fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Shell => "shell",
        TaskKind::RemoteSession => "remote",
        TaskKind::AsyncAgent => "agent",
        TaskKind::InProcessTeammate => "teammate",
        TaskKind::MonitorMcp => "mcp",
        TaskKind::Dream => "dream",
        TaskKind::Workflow => "workflow",
    }
}

pub fn format_elapsed(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

pub fn progress_bar(task: &TaskStatus, width: usize) -> String {
    let Some((done, total)) = task.progress else {
        return render_plain_progress_bar(0.0, width);
    };

    let ratio = if total == 0 {
        0.0
    } else {
        done.min(total) as f64 / total as f64
    };
    render_plain_progress_bar(ratio, width)
}

/// Theme-aware variant returning styled progress-bar spans.
///
/// The progress bar uses accent color for the filled portion.
pub fn progress_bar_styled(
    task: &TaskStatus,
    width: usize,
    colors: &ThemeColors,
) -> ratatui::text::Line<'static> {
    let ratio = task.progress.map_or(0.0, |(done, total)| {
        if total == 0 {
            0.0
        } else {
            done.min(total) as f64 / total as f64
        }
    });
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            "█".repeat(filled),
            ratatui::style::Style::default().fg(colors.accent),
        ),
        ratatui::text::Span::styled(
            "░".repeat(empty),
            ratatui::style::Style::default().fg(colors.inactive),
        ),
    ])
}

const _: fn(&TaskStatus, usize, &ThemeColors) -> ratatui::text::Line<'static> = progress_bar_styled;

pub fn progress_detail(task: &TaskStatus) -> String {
    let Some((done, total)) = task.progress else {
        return "no progress reported".to_string();
    };

    if total == 0 {
        return format!("{done}/0 steps");
    }

    let capped = done.min(total);
    let percent = (capped as f64 / total as f64) * 100.0;
    format!("{capped}/{total} steps ({percent:.0}%)")
}

pub fn task_header(task: &TaskStatus) -> String {
    format!(
        "● {} [{}] {} worked for {}",
        task.title,
        kind_label(task.kind),
        state_label(task.state),
        format_elapsed(task.elapsed_ms)
    )
}

fn render_plain_progress_bar(ratio: f64, width: usize) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), " ".repeat(empty))
}
