//! Binary-only adapters for engine-owned Agent runtime hooks.

use std::sync::{Arc, Once};

use allthecodes_engine::agent_runtime::{
    AgentTaskRef, AgentTaskStore, AgentToolRegistry, DashboardEmitter, TeammateSpawner,
};
use allthecodes_tasks::{
    AgentRuntimeActivity, TaskCreateOptions, TaskEntry, TaskRuntimeHandle, TaskStatus,
};
use allthecodes_types::output::{EventSeq, OutputReadBatch};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use allthecodes_engine::types::tool::Tool;

static INSTALL: Once = Once::new();

pub fn install(dashboard: Arc<dyn DashboardEmitter>, tools: Arc<dyn AgentToolRegistry>) {
    INSTALL.call_once(|| {
        let mut adapters = allthecodes_engine::agent_runtime::agent_runtime_adapters();
        adapters.dashboard = dashboard;
        adapters.tools = tools;
        adapters.teammate_spawner = Arc::new(RootTeammateSpawner);
        adapters.task_store = Arc::new(RootAgentTaskStore);
        allthecodes_engine::agent_runtime::set_agent_runtime_adapters(adapters);
    });
}

struct RootTeammateSpawner;

#[async_trait]
impl TeammateSpawner for RootTeammateSpawner {
    async fn spawn(
        &self,
        input: Value,
        ctx: &allthecodes_engine::types::tool::ToolUseContext,
        parent: &allthecodes_types::message::AssistantMessage,
        on_progress: Option<
            Box<dyn Fn(allthecodes_engine::types::tool::ToolProgress) + Send + Sync>,
        >,
    ) -> Result<allthecodes_engine::types::tool::ToolResult> {
        allthecodes_teams::team_spawn::TeamSpawnTool
            .call(input, ctx, parent, on_progress)
            .await
    }
}

struct RootAgentTaskStore;

impl AgentTaskStore for RootAgentTaskStore {
    fn try_create_with_options(
        &self,
        task_list_id: &str,
        subject: &str,
        description: &str,
        options: TaskCreateOptions,
    ) -> Result<TaskEntry> {
        allthecodes_tasks::store_for_task_list_id(task_list_id).try_create_with_options(
            subject,
            description,
            options,
        )
    }

    fn adopt_pending_agent_task(
        &self,
        task_ref: &AgentTaskRef,
        agent_id: &str,
        child_session_id: &str,
        worktree_path: Option<String>,
        worktree_branch: Option<String>,
    ) -> Result<TaskEntry> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id).adopt_pending_agent_task(
            &task_ref.task_id,
            agent_id,
            child_session_id,
            worktree_path,
            worktree_branch,
        )
    }

    fn get(&self, task_ref: &AgentTaskRef) -> Option<TaskEntry> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id).get(&task_ref.task_id)
    }

    fn try_update_status(
        &self,
        task_ref: &AgentTaskRef,
        status: TaskStatus,
    ) -> Result<Option<TaskEntry>> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .try_update_status(&task_ref.task_id, status)
    }

    fn update_runtime_activity(
        &self,
        task_ref: &AgentTaskRef,
        activity: AgentRuntimeActivity,
    ) -> Result<Option<TaskEntry>> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .update_runtime_activity(&task_ref.task_id, activity)
    }

    fn register_runtime_handle(
        &self,
        task_ref: &AgentTaskRef,
        cancellation_token: CancellationToken,
    ) -> bool {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .register_runtime_handle(&task_ref.task_id, cancellation_token)
    }

    fn append_output(&self, task_ref: &AgentTaskRef, output: &str) -> Option<TaskEntry> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .append_output(&task_ref.task_id, output)
    }

    fn read_output_events(
        &self,
        task_ref: &AgentTaskRef,
        after_seq: Option<EventSeq>,
        limit_bytes: usize,
    ) -> Result<Option<OutputReadBatch>> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id).read_output_events(
            &task_ref.task_id,
            after_seq,
            limit_bytes,
        )
    }

    fn try_stop(&self, task_ref: &AgentTaskRef) -> Result<Option<TaskEntry>> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .try_stop(&task_ref.task_id)
    }

    fn get_by_agent_id(&self, task_list_id: &str, agent_id: &str) -> Option<TaskEntry> {
        allthecodes_tasks::store_for_task_list_id(task_list_id).get_by_agent_id(agent_id)
    }

    fn unregister_runtime_handle(&self, task_ref: &AgentTaskRef) -> Option<TaskRuntimeHandle> {
        allthecodes_tasks::store_for_task_list_id(&task_ref.task_list_id)
            .unregister_runtime_handle(&task_ref.task_id)
    }

    fn unassign_teammate_tasks(
        &self,
        team_name: &str,
        teammate_id: &str,
        teammate_name: &str,
        reason: allthecodes_tasks::TeammateTaskExitReason,
    ) -> allthecodes_tasks::UnassignTeammateTasksResult {
        allthecodes_tasks::unassign_teammate_tasks(team_name, teammate_id, teammate_name, reason)
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn safe_task_file_stem(id: &str) -> String {
        id.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    #[test]
    #[serial]
    fn root_agent_task_store_uses_sqlite_default_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let _storage = EnvGuard::unset("ALLTHECODES_TASK_STORAGE");
        let _sqlite_path = EnvGuard::unset("ALLTHECODES_TASK_SQLITE_PATH");
        let _task_list = EnvGuard::unset("ALLTHECODES_TASK_LIST_ID");
        let _cc_task_list = EnvGuard::unset("CC_RUST_TASK_LIST_ID");
        let _claude_task_list = EnvGuard::unset("CLAUDE_CODE_TASK_LIST_ID");
        let _team = EnvGuard::unset("ALLTHECODES_TEAM_NAME");
        let _claude_team = EnvGuard::unset("CLAUDE_CODE_TEAM_NAME");

        let store = RootAgentTaskStore;
        let created = store
            .try_create_with_options(
                allthecodes_tasks::DEFAULT_TASK_LIST_ID,
                "Runtime adapter SQLite task",
                "verify root agent adapter storage",
                TaskCreateOptions {
                    agent_id: Some("runtime-agent".to_string()),
                    ..TaskCreateOptions::default()
                },
            )
            .expect("create task through root adapter");
        let task_ref = AgentTaskRef {
            task_list_id: allthecodes_tasks::DEFAULT_TASK_LIST_ID.to_string(),
            task_id: created.id.clone(),
        };
        let appended = store
            .append_output(&task_ref, "runtime adapter output")
            .expect("append output through root adapter");
        let updated = store
            .try_update_status(&task_ref, TaskStatus::Completed)
            .expect("update task through root adapter")
            .expect("updated task");

        assert_eq!(appended.output_summary, "runtime adapter output");
        assert_eq!(updated.status, TaskStatus::Completed);

        let task_dir = tmp
            .path()
            .join("tasks")
            .join(allthecodes_tasks::DEFAULT_TASK_LIST_ID);
        let file_stem = safe_task_file_stem(&created.id);
        assert!(tmp.path().join("state").join("state_5.sqlite").exists());
        assert!(!task_dir.join(format!("{file_stem}.json")).exists());
        assert!(task_dir.join(format!("{file_stem}.output.log")).exists());
    }

    #[test]
    #[serial]
    fn root_agent_task_store_routes_delegation_to_explicit_scope() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let scope = "parent-session-scope";
        let scoped_store = allthecodes_tasks::store_for_task_list_id(scope);
        let pending = scoped_store.try_create("delegated", "prompt").unwrap();
        let task_ref = AgentTaskRef {
            task_list_id: scope.to_string(),
            task_id: pending.id.clone(),
        };

        let adopted = RootAgentTaskStore
            .adopt_pending_agent_task(&task_ref, "child-session", "child-session", None, None)
            .unwrap();
        assert_eq!(adopted.status, TaskStatus::InProgress);
        assert_eq!(
            scoped_store.get(&pending.id).unwrap().agent_id.as_deref(),
            Some("child-session")
        );
        assert!(allthecodes_tasks::global_store().list().is_empty());
    }
}
