use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use futures::Stream;
use serde_json::Value;

use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::message::{AssistantMessage, Message, StreamEvent};
use allthecodes_engine::types::state::AutoCompactTracking;
use allthecodes_engine::types::tool::{ToolProgress, ToolResult, Tools};

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub assistant_message: AssistantMessage,
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
    pub langfuse_batch_span: Option<allthecodes_services::langfuse::LangfuseSpan>,
}

#[derive(Debug, Clone)]
pub struct ToolExecResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: ToolResult,
    pub is_error: bool,
    pub hook_stopped_continuation: bool,
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

    fn tool_progress_callback(&self) -> Option<Arc<dyn Fn(ToolProgress) + Send + Sync>> {
        None
    }

    fn get_app_state(&self) -> AppState;

    fn uuid(&self) -> String;

    fn is_aborted(&self) -> bool;

    fn get_tools(&self) -> Tools;

    async fn refresh_tools(&self) -> Result<Tools>;

    fn drain_background_results(
        &self,
    ) -> Vec<allthecodes_engine::agent_runtime::CompletedBackgroundAgent> {
        vec![]
    }

    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        Arc::new(allthecodes_types::hooks::NoopHookRunner)
    }

    fn audit_context(&self) -> allthecodes_observability::AuditContext {
        allthecodes_observability::AuditContext::noop("unknown")
    }

    fn langfuse_trace(&self) -> Option<allthecodes_services::langfuse::LangfuseTrace> {
        None
    }

    fn langfuse_provider_name(&self) -> Option<String> {
        None
    }
}
