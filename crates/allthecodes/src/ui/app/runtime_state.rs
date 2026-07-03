use crate::ui::app::agent_navigation::{AgentNavigationState, AgentThreadEntry};
use crate::ui::command_surface::{TaskSurfaceItem, TasksSurface};
#[cfg(test)]
use crate::ui::tasks::TaskStatus;
use allthecodes_ipc_protocol::BackendMessage;

#[derive(Debug)]
pub(super) struct RuntimeViewState {
    agent_nav: AgentNavigationState,
    current_agent_thread_id: Option<String>,
    live_task_items: Vec<TaskSurfaceItem>,
    backend_task_items: Vec<TaskSurfaceItem>,
    task_items: Vec<TaskSurfaceItem>,
}

impl Default for RuntimeViewState {
    fn default() -> Self {
        let live_task_items = TasksSurface::new().items;
        Self {
            agent_nav: AgentNavigationState::default(),
            current_agent_thread_id: None,
            task_items: live_task_items.clone(),
            live_task_items,
            backend_task_items: Vec::new(),
        }
    }
}

impl RuntimeViewState {
    pub(super) fn agent_nav(&self) -> &AgentNavigationState {
        &self.agent_nav
    }

    pub(super) fn agent_nav_mut(&mut self) -> &mut AgentNavigationState {
        &mut self.agent_nav
    }

    pub(super) fn upsert_agent(&mut self, entry: AgentThreadEntry) {
        self.agent_nav.upsert(entry);
    }

    pub(super) fn set_current_agent_thread(&mut self, thread_id: Option<String>) {
        self.current_agent_thread_id = thread_id;
    }

    pub(super) fn current_agent_thread_id(&self) -> Option<&String> {
        self.current_agent_thread_id.as_ref()
    }

    pub(super) fn task_items(&self) -> &[TaskSurfaceItem] {
        &self.task_items
    }

    pub(super) fn refresh_task_items(&mut self, items: &[TaskSurfaceItem]) {
        self.live_task_items = items.to_vec();
        self.rebuild_task_items();
    }

    #[cfg(test)]
    pub(super) fn tasks(&self) -> Vec<TaskStatus> {
        self.task_items
            .iter()
            .map(|item| item.task.clone())
            .collect()
    }

    pub(super) fn apply_task_event(&mut self, message: &BackendMessage) {
        let mut surface = TasksSurface::from_items(std::mem::take(&mut self.backend_task_items));
        surface.handle_event(message);
        self.backend_task_items = surface.items;
        self.rebuild_task_items();
    }

    fn rebuild_task_items(&mut self) {
        let mut items = self.live_task_items.clone();
        for item in &self.backend_task_items {
            if !items
                .iter()
                .any(|existing| existing.task.id == item.task.id)
            {
                items.push(item.clone());
            }
        }
        self.task_items = items;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::agent_navigation::AgentThreadEntry;
    use crate::ui::tasks::{TaskKind, TaskStatus};

    #[test]
    fn runtime_view_state_tracks_current_agent_thread() {
        let mut state = RuntimeViewState::default();
        state.upsert_agent(AgentThreadEntry {
            thread_id: "worker-1".to_string(),
            agent_nickname: Some("Build worker".to_string()),
            agent_role: Some("builder".to_string()),
            is_primary: false,
            is_closed: false,
        });
        state.set_current_agent_thread(Some("worker-1".to_string()));

        assert_eq!(
            state.current_agent_thread_id().map(String::as_str),
            Some("worker-1")
        );
        assert_eq!(state.agent_nav().thread_count(), 1);

        state.task_items.push(TaskSurfaceItem {
            task: TaskStatus::new("task-1", "cargo test", TaskKind::Shell),
            source: crate::ui::command_surface::TaskSurfaceSource::Tool,
        });
        assert_eq!(state.tasks().len(), 1);
    }
}
