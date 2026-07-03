use crate::ui::app::agent_navigation::{AgentNavigationState, AgentThreadEntry};
use crate::ui::tasks::TaskStatus;

#[derive(Debug, Default)]
pub(super) struct RuntimeViewState {
    agent_nav: AgentNavigationState,
    current_agent_thread_id: Option<String>,
    tasks: Vec<TaskStatus>,
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

    pub(super) fn tasks(&self) -> &[TaskStatus] {
        &self.tasks
    }

    pub(super) fn set_tasks(&mut self, tasks: Vec<TaskStatus>) {
        self.tasks = tasks;
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

        state.set_tasks(vec![TaskStatus::new(
            "task-1",
            "cargo test",
            TaskKind::Shell,
        )]);
        assert_eq!(state.tasks().len(), 1);
    }
}
