use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::command_surface::adapters::tasks::task_surface_items;
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::tasks::async_agent_detail_dialog::render_async_agent_detail_dialog;
use crate::ui::tasks::background_task_status::render_background_task_status;
use crate::ui::tasks::background_tasks_dialog::render_background_tasks_dialog;
use crate::ui::tasks::dream_detail_dialog::render_dream_detail_dialog;
use crate::ui::tasks::in_process_teammate_detail_dialog::render_in_process_teammate_detail_dialog;
use crate::ui::tasks::monitor_mcp_detail_dialog::render_monitor_mcp_detail_dialog;
use crate::ui::tasks::remote_session_detail_dialog::render_remote_session_detail_dialog;
use crate::ui::tasks::remote_session_progress::render_remote_session_progress;
use crate::ui::tasks::render_tool_activity::render_task_tool_activity;
use crate::ui::tasks::shell_detail_dialog::render_shell_detail_dialog;
use crate::ui::tasks::shell_progress::render_shell_progress;
use crate::ui::tasks::workflow_detail_dialog::render_workflow_detail_dialog;
use crate::ui::tasks::TaskState as UiTaskState;
use crate::ui::tasks::TaskStatus as UiTaskStatus;
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_types::agent_events::{AgentEvent, TeamEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasksSurface {
    pub(crate) items: Vec<TaskSurfaceItem>,
    pub(crate) selected_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskSurfaceItem {
    pub(crate) task: UiTaskStatus,
    pub(crate) source: TaskSurfaceSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskSurfaceSource {
    Tool,
    Team { teammate_name: String },
}

impl TasksSurface {
    pub(crate) fn new() -> Self {
        Self::from_items(task_surface_items())
    }

    pub(crate) fn from_items(items: Vec<TaskSurfaceItem>) -> Self {
        Self {
            items,
            selected_index: 0,
        }
    }

    pub(crate) fn sync_items(&mut self, items: &[TaskSurfaceItem]) {
        self.items = items.to_vec();
        if self.items.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self
                .selected_index
                .min(self.items.len().saturating_mul(2) - 1);
        }
    }

    pub(crate) fn render(&self) -> String {
        let tasks = self
            .items
            .iter()
            .map(|item| item.task.clone())
            .collect::<Vec<_>>();
        if self.detail_active() {
            let Some(item) = self.selected_item() else {
                return render_background_tasks_dialog(&tasks, 0);
            };
            let mut lines = vec![
                render_background_task_status(&tasks, true),
                String::new(),
                render_task_detail(item, &tasks),
            ];
            if matches!(item.task.kind, crate::ui::tasks::TaskKind::Shell) {
                lines.push(String::new());
                lines.push(render_shell_progress(&item.task));
            } else if matches!(item.task.kind, crate::ui::tasks::TaskKind::RemoteSession) {
                lines.push(String::new());
                lines.push(render_remote_session_progress(&item.task));
            }
            lines.push(String::new());
            lines.push(
                "Left list | Enter show | k/s stop | d delete tool | r refresh | Esc close"
                    .to_string(),
            );
            lines.join("\n")
        } else {
            [
                render_background_task_status(&tasks, true),
                String::new(),
                render_background_tasks_dialog(&tasks, self.list_index()),
                String::new(),
                render_task_tool_activity(&tasks),
                String::new(),
                "o/Right details | Enter show | k/s stop | d delete tool | r refresh | Esc close"
                    .to_string(),
            ]
            .join("\n")
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up => {
                self.set_list_index(cycle_index(self.list_index(), self.items.len(), -1));
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.set_list_index(cycle_index(self.list_index(), self.items.len(), 1));
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char('o') => {
                self.open_detail();
                CommandSurfaceOutcome::None
            }
            KeyCode::Left | KeyCode::Backspace => {
                self.close_detail();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self
                .selected_item()
                .map(|item| CommandSurfaceOutcome::Submit(format!("/tasks show {}", item.task.id)))
                .unwrap_or(CommandSurfaceOutcome::None),
            KeyCode::Char('k') | KeyCode::Char('s') => self.selected_stop_command(),
            KeyCode::Char('d') => self.selected_delete_command(),
            KeyCode::Char('r') => CommandSurfaceOutcome::Submit("/tasks".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn selected_item(&self) -> Option<&TaskSurfaceItem> {
        self.items.get(self.list_index())
    }

    pub(crate) fn selected_stop_command(&self) -> CommandSurfaceOutcome {
        let Some(item) = self.selected_item() else {
            return CommandSurfaceOutcome::None;
        };
        match &item.source {
            TaskSurfaceSource::Tool => {
                CommandSurfaceOutcome::Submit(format!("/tasks stop {}", item.task.id))
            }
            TaskSurfaceSource::Team { teammate_name } => {
                CommandSurfaceOutcome::Submit(format!("/team kill {}", teammate_name))
            }
        }
    }

    pub(crate) fn selected_delete_command(&self) -> CommandSurfaceOutcome {
        let Some(item) = self.selected_item() else {
            return CommandSurfaceOutcome::None;
        };
        match item.source {
            TaskSurfaceSource::Tool => {
                CommandSurfaceOutcome::Submit(format!("/tasks delete {}", item.task.id))
            }
            TaskSurfaceSource::Team { .. } => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn handle_event(&mut self, message: &BackendMessage) {
        match message {
            BackendMessage::ToolProgress {
                tool_use_id,
                tool,
                output,
                elapsed_seconds,
                total_lines,
                timeout_ms,
                ..
            } => {
                let mut task = UiTaskStatus::new(
                    tool_use_id.clone(),
                    tool.clone(),
                    crate::ui::tasks::TaskKind::Shell,
                );
                task.state = UiTaskState::Running;
                task.elapsed_ms = elapsed_seconds.saturating_mul(1000);
                task.output_lines = output.lines().map(ToString::to_string).collect();
                task.progress = total_lines
                    .and_then(|total| usize::try_from(total).ok())
                    .map(|total| (task.output_lines.len().min(total), total));
                task.summary = match timeout_ms {
                    Some(timeout) => format!(
                        "{} output line(s), timeout {}ms",
                        total_lines.unwrap_or(task.output_lines.len() as u64),
                        timeout
                    ),
                    None => format!(
                        "{} output line(s)",
                        total_lines.unwrap_or(task.output_lines.len() as u64)
                    ),
                };
                self.upsert_item(TaskSurfaceItem {
                    task,
                    source: TaskSurfaceSource::Tool,
                });
            }
            BackendMessage::BackgroundAgentComplete {
                agent_id,
                description,
                result_preview,
                had_error,
                duration_ms,
            } => {
                let mut task = UiTaskStatus::new(
                    agent_id.clone(),
                    description.clone(),
                    crate::ui::tasks::TaskKind::AsyncAgent,
                );
                task.state = if *had_error {
                    UiTaskState::Failed
                } else {
                    UiTaskState::Succeeded
                };
                task.elapsed_ms = *duration_ms;
                task.summary = result_preview.clone();
                task.output_lines = vec![result_preview.clone()];
                self.upsert_item(TaskSurfaceItem {
                    task,
                    source: TaskSurfaceSource::Tool,
                });
            }
            BackendMessage::AgentEvent { event } => self.apply_agent_event(event),
            BackendMessage::TeamEvent { event } => self.apply_team_event(event),
            _ => {}
        }
    }

    fn list_index(&self) -> usize {
        if self.items.is_empty() {
            0
        } else {
            self.selected_index % self.items.len()
        }
    }

    fn detail_active(&self) -> bool {
        !self.items.is_empty() && self.selected_index >= self.items.len()
    }

    fn set_list_index(&mut self, index: usize) {
        let detail_offset = if self.detail_active() {
            self.items.len()
        } else {
            0
        };
        self.selected_index = index.saturating_add(detail_offset);
    }

    fn open_detail(&mut self) {
        if !self.items.is_empty() && !self.detail_active() {
            self.selected_index = self.list_index() + self.items.len();
        }
    }

    fn close_detail(&mut self) {
        self.selected_index = self.list_index();
    }

    fn upsert_item(&mut self, item: TaskSurfaceItem) {
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|existing| existing.task.id == item.task.id)
        {
            *existing = item;
        } else {
            self.items.push(item);
        }
        if self.items.is_empty() {
            self.selected_index = 0;
        } else {
            self.selected_index = self
                .selected_index
                .min(self.items.len().saturating_mul(2) - 1);
        }
    }

    fn apply_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Spawned {
                agent_id,
                description,
                ..
            } => {
                let mut task = UiTaskStatus::new(
                    agent_id,
                    description,
                    crate::ui::tasks::TaskKind::AsyncAgent,
                );
                task.state = UiTaskState::Running;
                task.summary = "agent running".to_string();
                self.upsert_item(TaskSurfaceItem {
                    task,
                    source: TaskSurfaceSource::Tool,
                });
            }
            AgentEvent::Completed {
                agent_id,
                result_preview,
                had_error,
                duration_ms,
                ..
            } => {
                self.update_task(agent_id, |task| {
                    task.state = if *had_error {
                        UiTaskState::Failed
                    } else {
                        UiTaskState::Succeeded
                    };
                    task.elapsed_ms = *duration_ms;
                    task.summary = result_preview.clone();
                    task.output_lines.push(result_preview.clone());
                });
            }
            AgentEvent::Error {
                agent_id,
                error,
                duration_ms,
            } => {
                self.update_task(agent_id, |task| {
                    task.state = UiTaskState::Failed;
                    task.elapsed_ms = *duration_ms;
                    task.summary = error.clone();
                    task.output_lines.push(error.clone());
                });
            }
            AgentEvent::Aborted { agent_id } => {
                self.update_task(agent_id, |task| {
                    task.state = UiTaskState::Canceled;
                    task.summary = "agent aborted".to_string();
                });
            }
            AgentEvent::StreamDelta { agent_id, text }
            | AgentEvent::ThinkingDelta {
                agent_id,
                thinking: text,
            } => {
                self.update_task(agent_id, |task| {
                    task.output_lines.push(text.clone());
                    task.summary = text.lines().last().unwrap_or(text.as_str()).to_string();
                });
            }
            AgentEvent::ToolUse {
                agent_id,
                tool_name,
                input,
                ..
            } => {
                self.update_task(agent_id, |task| {
                    task.summary = format!("{tool_name} {}", input);
                });
            }
            AgentEvent::ToolResult {
                agent_id,
                output,
                is_error,
                ..
            } => {
                self.update_task(agent_id, |task| {
                    if *is_error {
                        task.state = UiTaskState::Failed;
                    }
                    task.output_lines.push(output.clone());
                    task.summary = output.lines().last().unwrap_or(output.as_str()).to_string();
                });
            }
            AgentEvent::OutputBatch {
                agent_id, output, ..
            } => {
                self.update_task(agent_id, |task| {
                    for event in &output.events {
                        if !event.chunk.is_empty() {
                            task.output_lines.push(event.chunk.clone());
                        }
                    }
                    if let Some(last_chunk) =
                        output.events.iter().rev().find_map(|event| {
                            (!event.chunk.is_empty()).then_some(event.chunk.as_str())
                        })
                    {
                        task.summary = last_chunk.lines().last().unwrap_or(last_chunk).to_string();
                    }
                });
            }
            AgentEvent::PermissionQueued { .. }
            | AgentEvent::PermissionResolved { .. }
            | AgentEvent::TreeSnapshot { .. }
            | AgentEvent::ExecutionRecord { .. } => {}
        }
    }

    fn apply_team_event(&mut self, event: &TeamEvent) {
        match event {
            TeamEvent::MemberJoined {
                team_name,
                agent_name,
                ..
            } => {
                let mut task = UiTaskStatus::new(
                    format!("team:{team_name}:{agent_name}"),
                    format!("{agent_name} ({team_name})"),
                    crate::ui::tasks::TaskKind::InProcessTeammate,
                );
                task.state = UiTaskState::Running;
                task.summary = "teammate joined".to_string();
                self.upsert_item(TaskSurfaceItem {
                    task,
                    source: TaskSurfaceSource::Team {
                        teammate_name: agent_name.clone(),
                    },
                });
            }
            TeamEvent::MemberLeft {
                team_name,
                agent_name,
                ..
            } => {
                let id = format!("team:{team_name}:{agent_name}");
                self.update_task(&id, |task| {
                    task.state = UiTaskState::Canceled;
                    task.summary = "teammate left".to_string();
                });
            }
            TeamEvent::MessageRouted {
                team_name,
                to,
                text,
                summary,
                ..
            } => {
                let id = format!("team:{team_name}:{to}");
                self.update_task(&id, |task| {
                    task.output_lines.push(text.clone());
                    task.summary = summary.clone().unwrap_or_else(|| text.clone());
                });
            }
            TeamEvent::StatusSnapshot {
                team_name, members, ..
            } => {
                for member in members {
                    let mut task = UiTaskStatus::new(
                        format!("team:{team_name}:{}", member.agent_name),
                        format!("{} ({team_name})", member.agent_name),
                        crate::ui::tasks::TaskKind::InProcessTeammate,
                    );
                    task.state = if member.is_active {
                        UiTaskState::Running
                    } else {
                        UiTaskState::Canceled
                    };
                    task.summary = format!("{} unread message(s)", member.unread_messages);
                    self.upsert_item(TaskSurfaceItem {
                        task,
                        source: TaskSurfaceSource::Team {
                            teammate_name: member.agent_name.clone(),
                        },
                    });
                }
            }
        }
    }

    fn update_task(&mut self, id: &str, update: impl FnOnce(&mut UiTaskStatus)) {
        if let Some(item) = self.items.iter_mut().find(|item| item.task.id == id) {
            update(&mut item.task);
        }
    }
}

const _: fn(&mut TasksSurface, &BackendMessage) = TasksSurface::handle_event;

fn render_task_detail(item: &TaskSurfaceItem, tasks: &[UiTaskStatus]) -> String {
    match item.task.kind {
        crate::ui::tasks::TaskKind::Shell => render_shell_detail_dialog(&item.task),
        crate::ui::tasks::TaskKind::RemoteSession => {
            render_remote_session_detail_dialog(&item.task)
        }
        crate::ui::tasks::TaskKind::AsyncAgent => render_async_agent_detail_dialog(&item.task),
        crate::ui::tasks::TaskKind::InProcessTeammate => {
            let teammate_name = match &item.source {
                TaskSurfaceSource::Team { teammate_name } => teammate_name.as_str(),
                TaskSurfaceSource::Tool => "teammate",
            };
            render_in_process_teammate_detail_dialog(&item.task, teammate_name)
        }
        crate::ui::tasks::TaskKind::MonitorMcp => {
            render_monitor_mcp_detail_dialog(&item.task, &item.task.title)
        }
        crate::ui::tasks::TaskKind::Dream => render_dream_detail_dialog(&item.task),
        crate::ui::tasks::TaskKind::Workflow => {
            render_workflow_detail_dialog(tasks, &item.task.title)
        }
    }
}
