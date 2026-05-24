//! Binary-only adapters for engine-owned Agent runtime hooks.

use std::sync::{Arc, Once};

use allthecodes_engine::agent_runtime::{
    AgentTaskStore, AgentToolRegistry, DashboardEmitter, TeammateSpawner,
};
use allthecodes_tasks::{TaskCreateOptions, TaskEntry, TaskRuntimeHandle, TaskStatus};
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
        subject: &str,
        description: &str,
        options: TaskCreateOptions,
    ) -> Result<TaskEntry> {
        allthecodes_tasks::global_store().try_create_with_options(subject, description, options)
    }

    fn try_update_status(&self, id: &str, status: TaskStatus) -> Result<Option<TaskEntry>> {
        allthecodes_tasks::global_store().try_update_status(id, status)
    }

    fn register_runtime_handle(&self, id: &str, cancellation_token: CancellationToken) -> bool {
        allthecodes_tasks::global_store().register_runtime_handle(id, cancellation_token)
    }

    fn append_output(&self, id: &str, output: &str) -> Option<TaskEntry> {
        allthecodes_tasks::global_store().append_output(id, output)
    }

    fn try_stop(&self, id: &str) -> Result<Option<TaskEntry>> {
        allthecodes_tasks::global_store().try_stop(id)
    }

    fn get_by_agent_id(&self, agent_id: &str) -> Option<TaskEntry> {
        allthecodes_tasks::global_store().get_by_agent_id(agent_id)
    }

    fn unregister_runtime_handle(&self, id: &str) -> Option<TaskRuntimeHandle> {
        allthecodes_tasks::global_store().unregister_runtime_handle(id)
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
