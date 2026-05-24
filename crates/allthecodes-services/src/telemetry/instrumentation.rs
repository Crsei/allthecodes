//! Span types for telemetry instrumentation.
//!
//! Each span tracks a lifecycle phase (interaction, model call, tool execution,
//! hook execution) with a `start()` constructor, `finish()` to record success,
//! and `record_error()` to record failure.
//!
//! All spans support:
//! - Automatic duration computation from start to finish/error.
//! - Internal event emission through the `TelemetryHandle`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{TelemetryEvent, TelemetryHandle};

/// Type of input for `emit_input_event`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    UserMessage,
    SystemPrompt,
    ToolResult,
    ToolInput,
    Other,
}

// ---------------------------------------------------------------------------
// InteractionSpan
// ---------------------------------------------------------------------------

/// Tracks a single user submit -> model response interaction.
#[derive(Debug, Clone)]
pub struct InteractionSpan {
    pub(crate) interaction_id: String,
    pub(crate) session_id: String,
    pub(crate) submit_id: String,
    pub(crate) start_time: DateTime<Utc>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) model: Option<String>,
    pub(crate) tools_used: Vec<String>,
    pub(crate) tokens_in: Option<u32>,
    pub(crate) tokens_out: Option<u32>,
    pub(crate) error: Option<String>,
    pub(crate) handle: Option<TelemetryHandle>,
}

impl InteractionSpan {
    /// Start a new interaction span.
    pub fn start(session_id: String, submit_id: String, handle: TelemetryHandle) -> Self {
        Self {
            interaction_id: format!("int_{}", Uuid::new_v4().as_simple()),
            session_id,
            submit_id,
            start_time: Utc::now(),
            duration_ms: None,
            model: None,
            tools_used: Vec::new(),
            tokens_in: None,
            tokens_out: None,
            error: None,
            handle: Some(handle),
        }
    }

    /// The interaction ID.
    pub fn interaction_id(&self) -> &str {
        &self.interaction_id
    }

    /// The session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The submit ID.
    pub fn submit_id(&self) -> &str {
        &self.submit_id
    }

    /// Duration in milliseconds (set after finish/error).
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// The model used (set after finish).
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Error message (set after `record_error`).
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Add a tool name to the tools_used list.
    pub fn add_tool(&mut self, tool_name: &str) {
        self.tools_used.push(tool_name.to_string());
    }

    /// Finish the span, recording success with model and token counts.
    pub fn finish(&mut self, model: &str, tokens_in: u32, tokens_out: u32) {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
        self.duration_ms = Some(elapsed);
        self.model = Some(model.to_string());
        self.tokens_in = Some(tokens_in);
        self.tokens_out = Some(tokens_out);
        self.emit_event();
    }

    /// Record an error on the span.
    pub fn record_error(&mut self, error: &str) {
        self.error = Some(error.to_string());
        if self.duration_ms.is_none() {
            let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
            self.duration_ms = Some(elapsed);
        }
        self.emit_event();
    }

    fn emit_event(&self) {
        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::Interaction {
                interaction_id: self.interaction_id.clone(),
                session_id: self.session_id.clone(),
                submit_id: self.submit_id.clone(),
                duration_ms: self.duration_ms,
                model: self.model.clone(),
                tools_used: self.tools_used.clone(),
                tokens_in: self.tokens_in,
                tokens_out: self.tokens_out,
                error: self.error.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// ModelSpan
// ---------------------------------------------------------------------------

/// Tracks a single model API call.
#[derive(Debug, Clone)]
pub struct ModelSpan {
    pub(crate) span_id: String,
    pub(crate) parent_span_id: Option<String>,
    pub(crate) model: String,
    pub(crate) start_time: DateTime<Utc>,
    pub(crate) request_tokens: Option<u32>,
    pub(crate) response_tokens: Option<u32>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) cache_hit: bool,
    pub(crate) retry_count: u32,
    pub(crate) error: Option<String>,
    pub(crate) handle: Option<TelemetryHandle>,
}

impl ModelSpan {
    /// Start a new model call span.
    pub fn start(
        model: impl Into<String>,
        parent_span_id: Option<String>,
        handle: TelemetryHandle,
    ) -> Self {
        Self {
            span_id: format!("mdl_{}", Uuid::new_v4().as_simple()),
            parent_span_id,
            model: model.into(),
            start_time: Utc::now(),
            request_tokens: None,
            response_tokens: None,
            duration_ms: None,
            cache_hit: false,
            retry_count: 0,
            error: None,
            handle: Some(handle),
        }
    }

    /// The span ID.
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// Finish the span with token counts.
    pub fn finish(
        &mut self,
        request_tokens: u32,
        response_tokens: u32,
        cache_hit: bool,
        retry_count: u32,
    ) {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
        self.duration_ms = Some(elapsed);
        self.request_tokens = Some(request_tokens);
        self.response_tokens = Some(response_tokens);
        self.cache_hit = cache_hit;
        self.retry_count = retry_count;
        self.emit_event();
    }

    /// Record an error on the model call.
    pub fn record_error(&mut self, error: &str) {
        self.error = Some(error.to_string());
        if self.duration_ms.is_none() {
            let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
            self.duration_ms = Some(elapsed);
        }
        self.emit_event();
    }

    fn emit_event(&self) {
        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::ModelCall {
                span_id: self.span_id.clone(),
                parent_span_id: self.parent_span_id.clone(),
                model: self.model.clone(),
                request_tokens: self.request_tokens,
                response_tokens: self.response_tokens,
                duration_ms: self.duration_ms,
                cache_hit: self.cache_hit,
                retry_count: self.retry_count,
                error: self.error.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// ToolSpan
// ---------------------------------------------------------------------------

/// Tracks a single tool execution.
#[derive(Debug, Clone)]
pub struct ToolSpan {
    pub(crate) span_id: String,
    pub(crate) parent_span_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) tool_input_summary: Option<String>,
    pub(crate) start_time: DateTime<Utc>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) result: String,
    pub(crate) handle: Option<TelemetryHandle>,
}

/// Result of a tool execution for telemetry purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResult {
    Ok,
    Error,
    Denied,
}

impl ToolResult {
    fn as_str(&self) -> &'static str {
        match self {
            ToolResult::Ok => "ok",
            ToolResult::Error => "error",
            ToolResult::Denied => "denied",
        }
    }
}

impl ToolSpan {
    /// Start a new tool execution span.
    pub fn start(
        tool_name: String,
        tool_input_summary: Option<String>,
        handle: TelemetryHandle,
    ) -> Self {
        Self {
            span_id: format!("tool_{}", Uuid::new_v4().as_simple()),
            parent_span_id: None,
            tool_name,
            tool_input_summary,
            start_time: Utc::now(),
            duration_ms: None,
            result: "pending".to_string(),
            handle: Some(handle),
        }
    }

    /// The span ID.
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// The tool name.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Finish the span with a result.
    pub fn finish(&mut self, result: ToolResult) {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
        self.duration_ms = Some(elapsed);
        self.result = result.as_str().to_string();
        self.emit_event();
    }

    /// Record an error (convenience for `finish(ToolResult::Error)`).
    pub fn record_error(&mut self, _error: &str) {
        self.finish(ToolResult::Error);
    }

    /// Record a denied result (convenience).
    pub fn record_denied(&mut self) {
        self.finish(ToolResult::Denied);
    }

    fn emit_event(&self) {
        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::ToolExecution {
                span_id: self.span_id.clone(),
                parent_span_id: self.parent_span_id.clone(),
                tool_name: self.tool_name.clone(),
                tool_input_summary: self.tool_input_summary.clone(),
                duration_ms: self.duration_ms,
                result: self.result.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// HookSpan
// ---------------------------------------------------------------------------

/// Tracks a hook execution time.
#[derive(Debug, Clone)]
pub struct HookSpan {
    pub(crate) hook_name: String,
    pub(crate) start_time: DateTime<Utc>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) result: String,
    pub(crate) handle: Option<TelemetryHandle>,
}

impl HookSpan {
    /// Start a new hook execution span.
    pub fn start(hook_name: impl Into<String>, handle: TelemetryHandle) -> Self {
        Self {
            hook_name: hook_name.into(),
            start_time: Utc::now(),
            duration_ms: None,
            result: "pending".to_string(),
            handle: Some(handle),
        }
    }

    /// The hook name.
    pub fn hook_name(&self) -> &str {
        &self.hook_name
    }

    /// Finish the span, recording success.
    pub fn finish(&mut self) {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
        self.duration_ms = Some(elapsed);
        self.result = "ok".to_string();
        self.emit_event();
    }

    /// Record an error on the hook.
    pub fn record_error(&mut self, _error: &str) {
        let elapsed = (Utc::now() - self.start_time).num_milliseconds().max(0) as u64;
        self.duration_ms = Some(elapsed);
        self.result = "error".to_string();
        self.emit_event();
    }

    fn emit_event(&self) {
        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::HookExecution {
                hook_name: self.hook_name.clone(),
                duration_ms: self.duration_ms,
                result: self.result.clone(),
            });
        }
    }
}
