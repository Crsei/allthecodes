//! QueryEngineDeps -- QueryDeps implementation for the QueryEngine.
//!
//! Provides the query loop with access to the engine's shared state
//! (abort flag, app state, tools) and, optionally, a real `ApiClient`
//! for making Anthropic API calls.

use parking_lot::{Mutex, RwLock};
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use futures::Stream;
use uuid::Uuid;

use allthecodes_types::callbacks::PermissionEventPayload;
use allthecodes_types::permission_events::{
    HookPermissionDecisionEvent, PermissionDecisionDebugEvent,
};

use crate::compact::compaction::build_post_compact_messages_with_boundary;
use crate::permissions::decision::{
    AutoClassifierDecision, AutoClassifierStage, DenialTracker, PermissionDecision,
    PermissionDecisionReason,
};
use crate::tool_runtime::execution::{
    find_tool, is_plan_mode_plan_file_write, sandbox_allowed_command_applies, security_validate,
    ToolExecutionResult,
};
use crate::types::app_state::AppState;
use crate::types::config::QuerySource;
use crate::types::message::{Message, MessageContent, StreamEvent, UserMessage};
use crate::types::state::AutoCompactTracking;
use crate::types::tool::{
    PermissionMode, PermissionRequestPayload, PermissionResponsePayload, ToolProgress, Tools,
    ValidationResult,
};
use allthecodes_engine::query::deps::{
    CompactionResult, ModelCallParams, ModelResponse, QueryDeps, ToolExecRequest, ToolExecResult,
};

use super::helpers::{build_messages_request, format_conversation_for_summary};
use super::{ActiveSteerState, AutoClassifierFn, QueryEngineState};

mod autocompact;
mod execute;
mod model_call;
mod permission;
pub(crate) use model_call::{model_for_autocompact, tool_execution_result_to_exec_result};
pub(crate) use permission::{
    auto_classifier_needed, central_permission_decision_for_tool, emit_hook_permission_decision,
    emit_permission_decision_debug, hook_error_is_critical, permission_denied_message,
    permission_feedback_message, permission_result_from_decision,
};

/// Dependency injection bridge: provides the query loop with access to the
/// engine's shared state (abort flag, app state, tools) and, optionally, a
/// real `ApiClient` for making Anthropic API calls.
#[derive(Clone)]
pub(crate) struct QueryEngineDeps {
    pub(crate) aborted: Arc<AtomicBool>,
    pub(crate) state: Arc<RwLock<QueryEngineState>>,
    pub(crate) cwd: String,
    pub(crate) session_id: String,
    pub(crate) query_source: QuerySource,
    /// Audit context for this submit — carries correlation IDs.
    pub(crate) audit_ctx: crate::observability::AuditContext,
    pub(crate) langfuse_trace: Option<crate::services::langfuse::LangfuseTrace>,
    /// When `Some`, the deps will use this client for `call_model` /
    /// `call_model_streaming`. When `None`, those methods bail with a
    /// descriptive error.
    pub(crate) api_client: Option<Arc<allthecodes_api::api::client::ApiClient>>,
    /// Sub-agent context -- propagated into `ToolUseContext` so that
    /// nested Agent tool calls can enforce recursion depth limits.
    pub(crate) agent_context: Option<crate::types::config::AgentContext>,
    /// Async callback for interactive permission prompts.
    /// Propagated into `ToolUseContext` for headless/TUI permission flow.
    pub(crate) permission_callback: Option<crate::types::tool::PermissionCallback>,
    /// Background agent sender — forwarded into ToolUseContext.
    pub(crate) bg_agent_tx: Option<allthecodes_types::agent_channel::AgentSender>,
    /// Callback for non-blocking permission-side UI events.
    pub(crate) permission_event_callback: Option<crate::types::tool::PermissionEventCallback>,
    /// Optional callback that receives every `ToolProgress` emitted by a
    /// tool. The query loop pulls this via `tool_progress_callback()` and
    /// hands it to `execute_tool_calls`. Tests can leave it unset; in
    /// headless mode the engine installs a closure that forwards each
    /// progress event to `FrontendSink`.
    pub(crate) tool_progress_callback: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    /// Shared buffer of completed background agents.
    pub(crate) pending_bg_results: crate::agent_runtime::PendingBackgroundResults,
    /// Active-turn steer queue.
    pub(crate) active_steer_state: Arc<Mutex<ActiveSteerState>>,
    /// Hook runner — used via the `HookRunner` trait from `cc-types::hooks` so
    /// the engine has no direct dependency on the concrete shell-hook runner.
    pub(crate) hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    /// Command dispatcher — forwarded into `ToolUseContext` for tools that
    /// spawn child engines (e.g. Agent).
    pub(crate) command_dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
    /// Async callback for computing auto-mode classifier decisions.
    /// Called with (tool_name, tool_input, classifier_input, messages, cwd)
    /// when mode is Auto.
    /// Returns `None` if the classifier is unavailable or skipped.
    pub(crate) auto_classifier_fn: Option<AutoClassifierFn>,
}

/// Tools that are always allowed in Auto mode without classifier classification.
const AUTO_MODE_ALLOWLISTED_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "LSP",
    "Sleep",
    "TaskCreate",
    "TaskUpdate",
    "TaskGet",
    "TaskList",
    "Plan",
    "WebSearch",
    "WebFetch",
];

impl QueryEngineDeps {
    /// Compute an auto-mode classifier decision if the classifier is configured
    /// and the mode is Auto. Returns `None` for non-Auto modes or when no
    /// classifier callback is installed.
    async fn compute_auto_classifier(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_classifier_input: &serde_json::Value,
    ) -> Option<AutoClassifierDecision> {
        let fn_ref = self.auto_classifier_fn.as_ref()?;

        // Scope the lock guard so it's dropped before the async call.
        let (cwd, messages) = {
            let state = self.state.read();
            if state.app_state.tool_permission_context.mode != PermissionMode::Auto {
                return None;
            }
            // Allowlisted tools bypass the classifier entirely in Auto mode.
            if AUTO_MODE_ALLOWLISTED_TOOLS.contains(&tool_name) {
                return Some(AutoClassifierDecision::allow(
                    "allowlisted-tool",
                    AutoClassifierStage::Fast,
                ));
            }
            (self.cwd.clone(), state.messages.clone())
        };

        fn_ref(
            tool_name.to_string(),
            tool_input.clone(),
            tool_classifier_input.clone(),
            messages,
            cwd,
        )
        .await
    }
}

#[async_trait::async_trait]
impl QueryDeps for QueryEngineDeps {
    fn tool_progress_callback(&self) -> Option<Arc<dyn Fn(ToolProgress) + Send + Sync>> {
        self.tool_progress_callback.clone()
    }

    async fn call_model(&self, params: ModelCallParams) -> Result<ModelResponse> {
        self.call_model_impl(params).await
    }

    async fn call_model_streaming(
        &self,
        params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.call_model_streaming_impl(params).await
    }

    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        self.microcompact_impl(messages).await
    }

    async fn autocompact(
        &self,
        params: ModelCallParams,
        tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        self.autocompact_impl(params, tracking).await
    }

    async fn reactive_compact(&self, messages: Vec<Message>) -> Result<Option<CompactionResult>> {
        self.reactive_compact_impl(messages).await
    }

    async fn collapse_drain(
        &self,
        messages: Vec<Message>,
        tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        self.collapse_drain_impl(messages, tracking).await
    }
    async fn execute_tool(
        &self,
        request: ToolExecRequest,
        tools: &Tools,
        parent_message: &crate::types::message::AssistantMessage,
        on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult> {
        self.execute_tool_impl(request, tools, parent_message, on_progress)
            .await
    }

    fn get_app_state(&self) -> AppState {
        self.state.read().app_state.clone()
    }

    fn uuid(&self) -> String {
        Uuid::new_v4().to_string()
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    fn get_tools(&self) -> Tools {
        self.state.read().tools.clone()
    }

    async fn refresh_tools(&self) -> Result<Tools> {
        self.refresh_tools_impl().await
    }

    fn drain_background_results(&self) -> Vec<crate::agent_runtime::CompletedBackgroundAgent> {
        self.pending_bg_results.drain_all()
    }

    fn drain_steer_messages(&self) -> Vec<String> {
        let mut state = self.active_steer_state.lock();
        state.pending.drain(..).collect()
    }

    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        self.hook_runner.clone()
    }

    fn audit_context(&self) -> crate::observability::AuditContext {
        self.audit_ctx.clone()
    }

    fn langfuse_trace(&self) -> Option<crate::services::langfuse::LangfuseTrace> {
        self.langfuse_trace.clone()
    }

    fn langfuse_provider_name(&self) -> Option<String> {
        self.api_client
            .as_ref()
            .map(|client| client.langfuse_provider_name().to_string())
    }
}
