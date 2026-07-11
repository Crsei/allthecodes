//! Engine-owned runtime support for Agent executions.
//!
//! These types bridge the Agent runtime event loop and the query lifecycle:
//! the event loop pushes completed background Agent results, and the engine
//! drains them at turn boundaries for injection into the conversation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::types::tool::Tool;
use allthecodes_tasks::{
    AgentRuntimeActivity, TaskCreateOptions, TaskEntry, TaskRuntimeHandle, TaskStatus,
};
use allthecodes_types::agent_events::AgentCompletionStatus;
use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
use allthecodes_types::agent_types::AgentNode;
use allthecodes_types::mcp::McpBindingContext;
use allthecodes_types::output::{EventSeq, OutputReadBatch};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Result from a completed background Agent.
#[derive(Debug, Clone)]
pub struct CompletedBackgroundAgent {
    pub agent_id: String,
    pub description: String,
    pub agent_type: Option<String>,
    pub result_text: String,
    pub had_error: bool,
    pub completion_status: AgentCompletionStatus,
    pub duration: Duration,
    pub total_tokens: Option<u64>,
    pub tool_uses: Option<u64>,
}

/// Shared buffer of completed agents waiting to be injected into the query loop.
///
/// Internal locking keeps clones connected to the same queue without requiring
/// external synchronization at the event-loop/query-loop boundary.
#[derive(Debug, Clone, Default)]
pub struct PendingBackgroundResults {
    inner: Arc<Mutex<Vec<CompletedBackgroundAgent>>>,
}

impl PendingBackgroundResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a completed agent result from the runtime event loop.
    pub fn push(&self, agent: CompletedBackgroundAgent) {
        self.inner.lock().push(agent);
    }

    /// Drain all pending results at a query turn boundary.
    pub fn drain_all(&self) -> Vec<CompletedBackgroundAgent> {
        let mut guard = self.inner.lock();
        std::mem::take(&mut *guard)
    }
}

#[derive(Clone)]
pub struct AgentRuntimeAdapters {
    pub dashboard: Arc<dyn DashboardEmitter>,
    pub builtin_agents: Arc<dyn BuiltinAgentRegistry>,
    pub tools: Arc<dyn AgentToolRegistry>,
    pub teammate_spawner: Arc<dyn TeammateSpawner>,
    pub task_store: Arc<dyn AgentTaskStore>,
    pub agent_tree: Arc<dyn AgentTreeRuntime>,
}

impl Default for AgentRuntimeAdapters {
    fn default() -> Self {
        Self {
            dashboard: Arc::new(NoopDashboardEmitter),
            builtin_agents: Arc::new(BuiltinAgentRegistryImpl),
            tools: Arc::new(NoopAgentToolRegistry),
            teammate_spawner: Arc::new(NoopTeammateSpawner),
            task_store: Arc::new(NoopAgentTaskStore),
            agent_tree: Arc::new(InMemoryAgentTreeRuntime::default()),
        }
    }
}

static AGENT_RUNTIME_ADAPTERS: std::sync::OnceLock<parking_lot::RwLock<AgentRuntimeAdapters>> =
    std::sync::OnceLock::new();

fn adapters() -> &'static parking_lot::RwLock<AgentRuntimeAdapters> {
    AGENT_RUNTIME_ADAPTERS.get_or_init(|| parking_lot::RwLock::new(AgentRuntimeAdapters::default()))
}

pub fn set_agent_runtime_adapters(adapters_value: AgentRuntimeAdapters) {
    *adapters().write() = adapters_value;
}

pub fn set_agent_tree_runtime(agent_tree: Arc<dyn AgentTreeRuntime>) {
    adapters().write().agent_tree = agent_tree;
}

pub fn agent_runtime_adapters() -> AgentRuntimeAdapters {
    adapters().read().clone()
}

pub trait DashboardEmitter: Send + Sync {
    #[expect(
        clippy::too_many_arguments,
        reason = "dashboard event ABI mirrors the structured AgentEvent payload"
    )]
    fn emit_subagent_event(
        &self,
        kind: &str,
        agent_id: &str,
        parent_agent_id: Option<&str>,
        description: Option<&str>,
        model: Option<&str>,
        depth: usize,
        background: bool,
        payload: Option<Value>,
    ) -> Result<()>;

    fn emit_execution_record(&self, record: &AgentRuntimeExecutionRecord) -> Result<()>;
}

struct NoopDashboardEmitter;

impl DashboardEmitter for NoopDashboardEmitter {
    fn emit_subagent_event(
        &self,
        _kind: &str,
        _agent_id: &str,
        _parent_agent_id: Option<&str>,
        _description: Option<&str>,
        _model: Option<&str>,
        _depth: usize,
        _background: bool,
        _payload: Option<Value>,
    ) -> Result<()> {
        Ok(())
    }

    fn emit_execution_record(&self, _record: &AgentRuntimeExecutionRecord) -> Result<()> {
        Ok(())
    }
}

pub trait AgentTreeRuntime: Send + Sync {
    fn register(&self, node: AgentNode);
    fn update_state(
        &self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    );
    fn snapshot(&self) -> Vec<AgentNode>;
    fn active_count(&self) -> usize;
}

#[derive(Default)]
struct InMemoryAgentTreeRuntime {
    inner: Mutex<AgentTreeState>,
}

#[derive(Default)]
struct AgentTreeState {
    nodes: HashMap<String, AgentNode>,
    roots: Vec<String>,
}

impl AgentTreeState {
    fn register(&mut self, node: AgentNode) {
        let id = node.agent_id.clone();
        let parent_id = node.parent_agent_id.clone();
        self.nodes.insert(id.clone(), node);
        if parent_id.is_none() && !self.roots.contains(&id) {
            self.roots.push(id);
        }
    }

    fn update_state(
        &mut self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    ) {
        if let Some(node) = self.nodes.get_mut(agent_id) {
            node.state = state.to_string();
            node.had_error = had_error;
            if let Some(result_preview) = result_preview {
                node.result_preview = Some(result_preview);
            }
            if let Some(duration_ms) = duration_ms {
                node.duration_ms = Some(duration_ms);
                node.completed_at = Some(chrono::Utc::now().timestamp());
            }
        }
    }

    fn snapshot(&self) -> Vec<AgentNode> {
        self.roots
            .iter()
            .filter_map(|id| self.build_subtree(id))
            .collect()
    }

    fn build_subtree(&self, id: &str) -> Option<AgentNode> {
        let node = self.nodes.get(id)?;
        let mut cloned = node.clone();
        cloned.children = self
            .nodes
            .values()
            .filter(|candidate| candidate.parent_agent_id.as_deref() == Some(id))
            .filter_map(|candidate| self.build_subtree(&candidate.agent_id))
            .collect();
        Some(cloned)
    }

    fn active_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.state == "running")
            .count()
    }
}

impl AgentTreeRuntime for InMemoryAgentTreeRuntime {
    fn register(&self, node: AgentNode) {
        self.inner.lock().register(node);
    }

    fn update_state(
        &self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    ) {
        self.inner
            .lock()
            .update_state(agent_id, state, result_preview, duration_ms, had_error);
    }

    fn snapshot(&self) -> Vec<AgentNode> {
        self.inner.lock().snapshot()
    }

    fn active_count(&self) -> usize {
        self.inner.lock().active_count()
    }
}

pub trait BuiltinAgentRegistry: Send + Sync {
    fn builtin_agent_entries(
        &self,
    ) -> Vec<allthecodes_ipc_protocol::subsystem_types::AgentDefinitionEntry>;
    fn builtin_agent_prompt(&self, name: &str) -> Option<String>;
}

#[derive(Default)]
struct BuiltinAgentRegistryImpl;

impl BuiltinAgentRegistry for BuiltinAgentRegistryImpl {
    fn builtin_agent_entries(
        &self,
    ) -> Vec<allthecodes_ipc_protocol::subsystem_types::AgentDefinitionEntry> {
        crate::agent::builtin_agents::builtin_agent_entries()
    }

    fn builtin_agent_prompt(&self, name: &str) -> Option<String> {
        crate::agent::builtin_agents::builtin_agent_prompt(name).map(ToOwned::to_owned)
    }
}

pub trait AgentToolRegistry: Send + Sync {
    fn get_all_tools(&self) -> Vec<Arc<dyn Tool>>;

    fn get_tools_for_mcp_context(&self, _ctx: &McpBindingContext) -> Vec<Arc<dyn Tool>> {
        self.get_all_tools()
    }
}

struct NoopAgentToolRegistry;

impl AgentToolRegistry for NoopAgentToolRegistry {
    fn get_all_tools(&self) -> Vec<Arc<dyn Tool>> {
        Vec::new()
    }
}

#[async_trait]
pub trait TeammateSpawner: Send + Sync {
    async fn spawn(
        &self,
        input: serde_json::Value,
        ctx: &crate::types::tool::ToolUseContext,
        parent: &crate::types::message::AssistantMessage,
        on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> Result<crate::types::tool::ToolResult>;
}

struct NoopTeammateSpawner;

#[async_trait]
impl TeammateSpawner for NoopTeammateSpawner {
    async fn spawn(
        &self,
        _input: serde_json::Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent: &crate::types::message::AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> Result<crate::types::tool::ToolResult> {
        Err(anyhow!("team spawn runtime is unavailable"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentTaskRef {
    pub task_list_id: String,
    pub task_id: String,
}

pub trait AgentTaskStore: Send + Sync {
    fn try_create_with_options(
        &self,
        task_list_id: &str,
        subject: &str,
        description: &str,
        options: TaskCreateOptions,
    ) -> Result<TaskEntry>;
    fn adopt_pending_agent_task(
        &self,
        task_ref: &AgentTaskRef,
        agent_id: &str,
        child_session_id: &str,
        worktree_path: Option<String>,
        worktree_branch: Option<String>,
    ) -> Result<TaskEntry>;
    fn get(&self, task_ref: &AgentTaskRef) -> Option<TaskEntry>;
    fn try_update_status(
        &self,
        task_ref: &AgentTaskRef,
        status: TaskStatus,
    ) -> Result<Option<TaskEntry>>;
    fn update_runtime_activity(
        &self,
        task_ref: &AgentTaskRef,
        activity: AgentRuntimeActivity,
    ) -> Result<Option<TaskEntry>>;
    fn register_runtime_handle(
        &self,
        task_ref: &AgentTaskRef,
        cancellation_token: CancellationToken,
    ) -> bool;
    fn append_output(&self, task_ref: &AgentTaskRef, output: &str) -> Option<TaskEntry>;
    fn read_output_events(
        &self,
        task_ref: &AgentTaskRef,
        after_seq: Option<EventSeq>,
        limit_bytes: usize,
    ) -> Result<Option<OutputReadBatch>>;
    fn try_stop(&self, task_ref: &AgentTaskRef) -> Result<Option<TaskEntry>>;
    fn get_by_agent_id(&self, task_list_id: &str, agent_id: &str) -> Option<TaskEntry>;
    fn unregister_runtime_handle(&self, task_ref: &AgentTaskRef) -> Option<TaskRuntimeHandle>;
    fn unassign_teammate_tasks(
        &self,
        team_name: &str,
        teammate_id: &str,
        teammate_name: &str,
        reason: allthecodes_tasks::TeammateTaskExitReason,
    ) -> allthecodes_tasks::UnassignTeammateTasksResult;
}

struct NoopAgentTaskStore;

impl AgentTaskStore for NoopAgentTaskStore {
    fn try_create_with_options(
        &self,
        _task_list_id: &str,
        _subject: &str,
        _description: &str,
        _options: TaskCreateOptions,
    ) -> Result<TaskEntry> {
        Err(anyhow!("task store runtime is unavailable"))
    }

    fn adopt_pending_agent_task(
        &self,
        _task_ref: &AgentTaskRef,
        _agent_id: &str,
        _child_session_id: &str,
        _worktree_path: Option<String>,
        _worktree_branch: Option<String>,
    ) -> Result<TaskEntry> {
        Err(anyhow!("task store runtime is unavailable"))
    }

    fn get(&self, _task_ref: &AgentTaskRef) -> Option<TaskEntry> {
        None
    }

    fn try_update_status(
        &self,
        _task_ref: &AgentTaskRef,
        _status: TaskStatus,
    ) -> Result<Option<TaskEntry>> {
        Ok(None)
    }

    fn update_runtime_activity(
        &self,
        _task_ref: &AgentTaskRef,
        _activity: AgentRuntimeActivity,
    ) -> Result<Option<TaskEntry>> {
        Ok(None)
    }

    fn register_runtime_handle(
        &self,
        _task_ref: &AgentTaskRef,
        _cancellation_token: CancellationToken,
    ) -> bool {
        false
    }

    fn append_output(&self, _task_ref: &AgentTaskRef, _output: &str) -> Option<TaskEntry> {
        None
    }

    fn read_output_events(
        &self,
        _task_ref: &AgentTaskRef,
        _after_seq: Option<EventSeq>,
        _limit_bytes: usize,
    ) -> Result<Option<OutputReadBatch>> {
        Ok(None)
    }

    fn try_stop(&self, _task_ref: &AgentTaskRef) -> Result<Option<TaskEntry>> {
        Ok(None)
    }

    fn get_by_agent_id(&self, _task_list_id: &str, _agent_id: &str) -> Option<TaskEntry> {
        None
    }

    fn unregister_runtime_handle(&self, _task_ref: &AgentTaskRef) -> Option<TaskRuntimeHandle> {
        None
    }

    fn unassign_teammate_tasks(
        &self,
        _team_name: &str,
        _teammate_id: &str,
        _teammate_name: &str,
        _reason: allthecodes_tasks::TeammateTaskExitReason,
    ) -> allthecodes_tasks::UnassignTeammateTasksResult {
        allthecodes_tasks::UnassignTeammateTasksResult {
            unassigned_tasks: Vec::new(),
            notification_message: String::new(),
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "public adapter shim preserves existing dashboard event call sites"
)]
pub fn emit_subagent_event(
    kind: &str,
    agent_id: &str,
    parent_agent_id: Option<&str>,
    description: Option<&str>,
    model: Option<&str>,
    depth: usize,
    background: bool,
    payload: Option<Value>,
) -> Result<()> {
    adapters().read().dashboard.emit_subagent_event(
        kind,
        agent_id,
        parent_agent_id,
        description,
        model,
        depth,
        background,
        payload,
    )
}

pub fn emit_execution_record(record: &AgentRuntimeExecutionRecord) -> Result<()> {
    adapters().read().dashboard.emit_execution_record(record)
}

pub fn register_agent_node(node: AgentNode) {
    adapters().read().agent_tree.register(node);
}

pub fn update_agent_state(
    agent_id: &str,
    state: &str,
    result_preview: Option<String>,
    duration_ms: Option<u64>,
    had_error: bool,
) {
    adapters().read().agent_tree.update_state(
        agent_id,
        state,
        result_preview,
        duration_ms,
        had_error,
    );
}

pub fn agent_tree_snapshot() -> Vec<AgentNode> {
    adapters().read().agent_tree.snapshot()
}

pub fn active_agent_count() -> usize {
    adapters().read().agent_tree.active_count()
}

pub fn builtin_agent_entries(
) -> Vec<allthecodes_ipc_protocol::subsystem_types::AgentDefinitionEntry> {
    adapters().read().builtin_agents.builtin_agent_entries()
}

pub fn builtin_agent_prompt(name: &str) -> Option<String> {
    adapters().read().builtin_agents.builtin_agent_prompt(name)
}

pub fn all_tools() -> Vec<Arc<dyn Tool>> {
    adapters().read().tools.get_all_tools()
}

pub fn tools_for_mcp_context(ctx: &McpBindingContext) -> Vec<Arc<dyn Tool>> {
    adapters().read().tools.get_tools_for_mcp_context(ctx)
}

pub async fn spawn_teammate(
    input: serde_json::Value,
    ctx: &crate::types::tool::ToolUseContext,
    parent: &crate::types::message::AssistantMessage,
    on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
) -> Result<crate::types::tool::ToolResult> {
    let spawner = adapters().read().teammate_spawner.clone();
    spawner.spawn(input, ctx, parent, on_progress).await
}

pub fn global_task_store() -> Arc<dyn AgentTaskStore> {
    adapters().read().task_store.clone()
}

pub fn unassign_teammate_tasks(
    team_name: &str,
    teammate_id: &str,
    teammate_name: &str,
    reason: allthecodes_tasks::TeammateTaskExitReason,
) -> allthecodes_tasks::UnassignTeammateTasksResult {
    adapters().read().task_store.unassign_teammate_tasks(
        team_name,
        teammate_id,
        teammate_name,
        reason,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_completed(id: &str, desc: &str) -> CompletedBackgroundAgent {
        CompletedBackgroundAgent {
            agent_id: id.to_string(),
            description: desc.to_string(),
            agent_type: Some("worker".to_string()),
            result_text: format!("Result from {}", desc),
            had_error: false,
            completion_status: AgentCompletionStatus::Completed,
            duration: Duration::from_secs(1),
            total_tokens: None,
            tool_uses: None,
        }
    }

    fn make_node(id: &str, parent: Option<&str>) -> AgentNode {
        AgentNode {
            agent_id: id.to_string(),
            parent_agent_id: parent.map(ToOwned::to_owned),
            description: format!("agent {id}"),
            agent_type: None,
            model: None,
            state: "running".to_string(),
            is_background: false,
            depth: if parent.is_some() { 2 } else { 1 },
            chain_id: "chain".to_string(),
            spawned_at: 100,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            children: vec![],
        }
    }

    #[test]
    fn pending_results_push_and_drain() {
        let pending = PendingBackgroundResults::new();
        assert!(pending.drain_all().is_empty());

        pending.push(make_completed("a1", "task one"));
        pending.push(make_completed("a2", "task two"));

        let drained = pending.drain_all();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].agent_id, "a1");
        assert_eq!(drained[1].agent_id, "a2");

        assert!(pending.drain_all().is_empty());
    }

    #[test]
    fn pending_results_clone_shares_state() {
        let pending1 = PendingBackgroundResults::new();
        let pending2 = pending1.clone();

        pending1.push(make_completed("a1", "task"));
        let drained = pending2.drain_all();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].agent_id, "a1");
    }

    #[test]
    fn in_memory_agent_tree_tracks_snapshot_and_active_count() {
        let tree = InMemoryAgentTreeRuntime::default();
        tree.register(make_node("root", None));
        tree.register(make_node("child", Some("root")));

        let snapshot = tree.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].agent_id, "root");
        assert_eq!(snapshot[0].children.len(), 1);
        assert_eq!(snapshot[0].children[0].agent_id, "child");
        assert_eq!(tree.active_count(), 2);

        tree.update_state(
            "child",
            "completed",
            Some("done".to_string()),
            Some(42),
            false,
        );
        assert_eq!(tree.active_count(), 1);

        let snapshot = tree.snapshot();
        assert_eq!(snapshot[0].children[0].state, "completed");
        assert_eq!(
            snapshot[0].children[0].result_preview.as_deref(),
            Some("done")
        );
        assert_eq!(snapshot[0].children[0].duration_ms, Some(42));
        assert!(snapshot[0].children[0].completed_at.is_some());
    }
}
