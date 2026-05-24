//! Rust-side background task UI surfaces.
pub mod async_agent_detail_dialog;
pub mod background_task;
pub mod background_task_status;
pub mod background_tasks_dialog;
pub mod dream_detail_dialog;
pub mod in_process_teammate_detail_dialog;
pub mod monitor_mcp_detail_dialog;
pub mod remote_session_detail_dialog;
pub mod remote_session_progress;
pub mod render_tool_activity;
pub mod shell_detail_dialog;
pub mod shell_progress;
pub mod task_status_utils;
pub mod workflow_detail_dialog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Shell,
    RemoteSession,
    AsyncAgent,
    InProcessTeammate,
    MonitorMcp,
    Dream,
    Workflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatus {
    pub id: String,
    pub title: String,
    pub kind: TaskKind,
    pub state: TaskState,
    pub progress: Option<(usize, usize)>,
    pub summary: String,
    pub elapsed_ms: u64,
    pub output_lines: Vec<String>,
}

impl TaskStatus {
    pub fn new(id: impl Into<String>, title: impl Into<String>, kind: TaskKind) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            kind,
            state: TaskState::Pending,
            progress: None,
            summary: String::new(),
            elapsed_ms: 0,
            output_lines: Vec::new(),
        }
    }
}

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    let _ = TaskStatus::new("task", "Task", TaskKind::Shell);
}

#[cfg(test)]
mod tests {
    use super::async_agent_detail_dialog::render_async_agent_detail_dialog;
    use super::background_task::render_background_task;
    use super::background_task_status::render_background_task_status;
    use super::background_tasks_dialog::render_background_tasks_dialog;
    use super::dream_detail_dialog::render_dream_detail_dialog;
    use super::in_process_teammate_detail_dialog::render_in_process_teammate_detail_dialog;
    use super::monitor_mcp_detail_dialog::render_monitor_mcp_detail_dialog;
    use super::remote_session_detail_dialog::render_remote_session_detail_dialog;
    use super::remote_session_progress::render_remote_session_progress;
    use super::render_tool_activity::render_task_tool_activity;
    use super::shell_detail_dialog::render_shell_detail_dialog;
    use super::shell_progress::render_shell_progress;
    use super::task_status_utils::progress_bar_styled;
    use super::workflow_detail_dialog::render_workflow_detail_dialog;
    use super::{TaskKind, TaskState, TaskStatus};
    use crate::ui::theme::{get_theme, ThemeName};

    #[test]
    fn snapshot_task_surfaces() {
        let mut shell = TaskStatus::new("task-1", "cargo test", TaskKind::Shell);
        shell.state = TaskState::Running;
        shell.progress = Some((3, 5));
        shell.summary = "running tests".to_string();
        shell.elapsed_ms = 12_400;
        shell.output_lines = vec![
            "running 12 tests".to_string(),
            "test ui::tasks ... ok".to_string(),
        ];

        let mut remote = TaskStatus::new("task-2", "remote deploy", TaskKind::RemoteSession);
        remote.state = TaskState::Failed;
        remote.summary = "connection lost".to_string();
        remote.output_lines = vec!["ssh exited with 255".to_string()];

        let mut agent = TaskStatus::new("task-3", "review worker", TaskKind::AsyncAgent);
        agent.state = TaskState::Succeeded;
        agent.summary = "reported findings".to_string();
        agent.output_lines = vec!["no blocking issues".to_string()];

        let tasks = vec![shell.clone(), remote.clone(), agent.clone()];
        let rendered = [
            section("row", render_background_task(&shell, true)),
            section("status", render_background_task_status(&tasks, true)),
            section("dialog", render_background_tasks_dialog(&tasks, 1)),
            section("shell-progress", render_shell_progress(&shell)),
            section("shell-detail", render_shell_detail_dialog(&shell)),
            section("remote-progress", render_remote_session_progress(&remote)),
            section(
                "remote-detail",
                render_remote_session_detail_dialog(&remote),
            ),
            section("async-agent", render_async_agent_detail_dialog(&agent)),
            section(
                "in-process",
                render_in_process_teammate_detail_dialog(&agent, "builder"),
            ),
            section(
                "monitor-mcp",
                render_monitor_mcp_detail_dialog(&remote, "docs"),
            ),
            section("dream", render_dream_detail_dialog(&agent)),
            section("workflow", render_workflow_detail_dialog(&tasks, "ui-port")),
            section("activity", render_task_tool_activity(&tasks)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("task_surfaces", rendered);
    }

    #[test]
    fn styled_progress_bar_uses_task_progress() {
        let mut task = TaskStatus::new("task-1", "cargo test", TaskKind::Shell);
        task.progress = Some((1, 4));
        let colors = get_theme(&ThemeName::Dark);

        let line = progress_bar_styled(&task, 4, colors);
        let plain = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(plain.chars().count(), 4);
        assert!(plain.starts_with("█"));
    }

    fn section(name: &str, body: impl AsRef<str>) -> String {
        format!("## {name}\n{}", body.as_ref())
    }
}
