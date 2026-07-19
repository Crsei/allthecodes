use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use futures::Stream;
use serde_json::Value;

use allthecodes_types::agent_events::AgentEvent;
use allthecodes_types::agent_runtime_record::AgentRuntimePermissionDecision;
use allthecodes_types::brief::BriefMessagePayload;

use crate::effort::ResolvedEffort;
use crate::types::app_state::AppState;
use crate::types::message::{AssistantMessage, Message, StreamEvent, Usage};
use crate::types::state::AutoCompactTracking;
use crate::types::tool::{ToolProgress, ToolResult, Tools};

#[derive(Clone)]
pub enum ToolRefreshOutcome {
    Fresh {
        tools: Tools,
        elapsed_ms: u64,
    },
    CachedBusy {
        tools: Tools,
        elapsed_ms: u64,
        reason: String,
    },
    CachedError {
        tools: Tools,
        elapsed_ms: u64,
        reason: String,
    },
}

impl ToolRefreshOutcome {
    pub fn fresh(tools: Tools, elapsed_ms: u64) -> Self {
        Self::Fresh { tools, elapsed_ms }
    }

    pub fn cached_busy(tools: Tools, elapsed_ms: u64, reason: impl Into<String>) -> Self {
        Self::CachedBusy {
            tools,
            elapsed_ms,
            reason: reason.into(),
        }
    }

    pub fn cached_error(tools: Tools, elapsed_ms: u64, reason: impl Into<String>) -> Self {
        Self::CachedError {
            tools,
            elapsed_ms,
            reason: reason.into(),
        }
    }

    pub fn tools(&self) -> &Tools {
        match self {
            Self::Fresh { tools, .. }
            | Self::CachedBusy { tools, .. }
            | Self::CachedError { tools, .. } => tools,
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        match self {
            Self::Fresh { elapsed_ms, .. }
            | Self::CachedBusy { elapsed_ms, .. }
            | Self::CachedError { elapsed_ms, .. } => *elapsed_ms,
        }
    }

    pub fn cached_reason(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Fresh { .. } => None,
            Self::CachedBusy { reason, .. } => Some(("cached_busy", reason)),
            Self::CachedError { reason, .. } => Some(("cached_error", reason)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub assistant_message: AssistantMessage,
    pub stream_events: Vec<StreamEvent>,
    pub usage: Usage,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<Message>,
    pub tracking: AutoCompactTracking,
}

#[derive(Debug, Clone)]
pub struct ToolExecRequest {
    pub tool_use_id: String,
    pub tool_name: String,
    pub input: Value,
    pub langfuse_batch_span: Option<crate::services::langfuse::LangfuseSpan>,
}

#[derive(Debug, Clone)]
pub struct ToolExecResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub effective_input: Value,
    pub result: ToolResult,
    pub is_error: bool,
    pub hook_stopped_continuation: bool,
    pub duration_ms: Option<u64>,
    pub permission_decision: Option<AgentRuntimePermissionDecision>,
    pub brief_message: Option<BriefMessagePayload>,
}

impl ToolExecResult {
    pub fn with_runtime_metadata(
        mut self,
        effective_input: Value,
        duration_ms: Option<u64>,
        permission_decision: Option<AgentRuntimePermissionDecision>,
    ) -> Self {
        self.effective_input = effective_input;
        self.duration_ms = duration_ms;
        self.permission_decision = permission_decision;
        self
    }
}

#[derive(Clone)]
pub struct ModelCallParams {
    pub messages: Vec<Message>,
    pub system_prompt: Vec<String>,
    pub tools: Tools,
    pub model: Option<String>,
    pub max_output_tokens: Option<usize>,
    pub skip_cache_write: Option<bool>,
    pub thinking_enabled: Option<bool>,
    pub effort_value: Option<String>,
    pub output_config: Option<Value>,
    pub model_reasoning_effort: Option<String>,
    /// Provider-aware resolved effort. When `Some`, the wire builder uses
    /// this verbatim and ignores `effort_value`/`output_config`/
    /// `model_reasoning_effort` for effort shaping. When `None`, callers
    /// fall back to the legacy scalar fields (kept for compatibility while
    /// migration is in progress).
    pub resolved_effort: Option<ResolvedEffort>,
    pub advisor_model: Option<String>,
}

impl std::fmt::Debug for ModelCallParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelCallParams")
            .field("messages_count", &self.messages.len())
            .field("system_prompt", &self.system_prompt)
            .field("tools_count", &self.tools.len())
            .field("model", &self.model)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("skip_cache_write", &self.skip_cache_write)
            .field("thinking_enabled", &self.thinking_enabled)
            .field("effort_value", &self.effort_value)
            .field("output_config", &self.output_config)
            .field("model_reasoning_effort", &self.model_reasoning_effort)
            .field("resolved_effort", &self.resolved_effort)
            .field("advisor_model", &self.advisor_model)
            .finish()
    }
}

#[async_trait::async_trait]
pub trait QueryDeps: Send + Sync {
    async fn call_model(&self, params: ModelCallParams) -> Result<ModelResponse>;

    async fn call_model_streaming(
        &self,
        params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>;

    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>>;

    async fn autocompact(
        &self,
        params: ModelCallParams,
        tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>>;

    async fn reactive_compact(&self, messages: Vec<Message>) -> Result<Option<CompactionResult>>;

    async fn collapse_drain(
        &self,
        _messages: Vec<Message>,
        _tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        Ok(None)
    }

    async fn execute_tool(
        &self,
        request: ToolExecRequest,
        tools: &Tools,
        parent_message: &AssistantMessage,
        on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult>;

    /// Persist canonical verification lifecycle facts. Test dependencies may
    /// keep the default no-op implementation.
    async fn record_verification_items(
        &self,
        _items: Vec<allthecodes_session::record_replay::RecordItem>,
    ) {
    }

    /// Persist query lifecycle facts that must survive process interruption.
    async fn record_query_items(
        &self,
        _items: Vec<allthecodes_session::record_replay::RecordItem>,
    ) {
    }

    /// Append the assistant tool-call message and its tool-result messages to
    /// the canonical rollout, then flush before another model turn.
    async fn persist_tool_results(&self, _messages: Vec<Message>) -> Result<()> {
        Ok(())
    }

    /// Mark this query as terminally unverified so the lifecycle cannot
    /// translate the last assistant text into a successful SDK result.
    fn mark_verification_incomplete(&self, _summary: String) {}

    fn tool_progress_callback(&self) -> Option<Arc<dyn Fn(ToolProgress) + Send + Sync>> {
        None
    }

    fn get_app_state(&self) -> AppState;

    fn uuid(&self) -> String;

    fn is_aborted(&self) -> bool;

    fn get_tools(&self) -> Tools;

    async fn refresh_tools(&self) -> ToolRefreshOutcome;

    fn drain_background_results(&self) -> Vec<crate::agent_runtime::CompletedBackgroundAgent> {
        vec![]
    }

    fn drain_steer_messages(&self) -> Vec<String> {
        vec![]
    }

    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        Arc::new(allthecodes_types::hooks::NoopHookRunner)
    }

    fn audit_context(&self) -> allthecodes_observability::AuditContext {
        allthecodes_observability::AuditContext::noop("unknown")
    }

    fn langfuse_trace(&self) -> Option<crate::services::langfuse::LangfuseTrace> {
        None
    }

    fn langfuse_provider_name(&self) -> Option<String> {
        None
    }

    fn provider_recovery_policy(
        &self,
    ) -> Option<allthecodes_api::api::client::ProviderRecoveryPolicy> {
        None
    }

    fn uses_codex_responses(&self) -> bool {
        false
    }

    /// Session identifier shared across all events in a session.
    fn session_id(&self) -> &str {
        ""
    }

    /// Agent identifier for the current tool execution context, if any.
    fn agent_id(&self) -> Option<&str> {
        None
    }

    /// Agent type/role label, if any.
    fn agent_type(&self) -> Option<&str> {
        None
    }

    /// Parent agent identifier for the current agent context, if any.
    fn parent_agent_id(&self) -> Option<&str> {
        None
    }

    /// Send an agent event to the frontend (headless/TUI).
    ///
    /// The default implementation is a no-op so test mocks don't need to
    /// override this method.
    fn send_agent_event(&self, _event: AgentEvent) {}
}
