use crate::ui::app::agent_navigation::{AgentNavigationState, AgentThreadEntry};
use crate::ui::command_surface::{task_list_items, TaskSurfaceItem, TasksSurface};
use crate::ui::context_layer::ContextLayerState;
use crate::ui::messages::task_list_content::TaskListItem;
#[cfg(test)]
use crate::ui::tasks::TaskStatus;
use allthecodes_ipc_protocol::BackendMessage;

#[derive(Debug)]
pub(super) struct RuntimeViewState {
    agent_nav: AgentNavigationState,
    current_agent_thread_id: Option<String>,
    context_layer: ContextLayerState,
    live_task_items: Vec<TaskSurfaceItem>,
    backend_task_items: Vec<TaskSurfaceItem>,
    task_items: Vec<TaskSurfaceItem>,
    task_list_items: Vec<TaskListItem>,
}

impl Default for RuntimeViewState {
    fn default() -> Self {
        let live_task_items = TasksSurface::new().items;
        Self {
            agent_nav: AgentNavigationState::default(),
            current_agent_thread_id: None,
            context_layer: ContextLayerState::default(),
            task_items: live_task_items.clone(),
            live_task_items,
            backend_task_items: Vec::new(),
            task_list_items: task_list_items(),
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

    pub(super) fn context_layer(&self) -> &ContextLayerState {
        &self.context_layer
    }

    pub(super) fn context_layer_mut(&mut self) -> &mut ContextLayerState {
        &mut self.context_layer
    }

    pub(super) fn task_items(&self) -> &[TaskSurfaceItem] {
        &self.task_items
    }

    pub(super) fn task_list_items(&self) -> &[TaskListItem] {
        &self.task_list_items
    }

    /// Refresh the list that drives the expanded Spinner task view.
    ///
    /// The task adapter is the single conversion point for the in-process
    /// store, so this refresh has the same source-of-truth as `/tasks`.
    pub(super) fn refresh_task_list_items(&mut self) -> bool {
        let next = task_list_items();
        if self.task_list_items == next {
            return false;
        }
        self.task_list_items = next;
        true
    }

    #[cfg(test)]
    pub(super) fn set_task_list_items_for_tests(&mut self, items: Vec<TaskListItem>) {
        self.task_list_items = items;
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
        use crate::ui::context_layer::{ContextLayerItem, ContextLayerKey, ContextTone};

        let mut state = RuntimeViewState::default();
        state.refresh_task_items(&[]);
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

        state.context_layer_mut().upsert(ContextLayerItem::keyed(
            ContextLayerKey::Repo,
            ContextTone::Info,
            "repo",
            "allthecodes",
        ));
        assert_eq!(state.context_layer().items().len(), 1);
    }
}
