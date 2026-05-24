//! No-op Langfuse surface used when the `telemetry` feature is disabled.

use serde_json::Value;

use allthecodes_types::message::Usage;

#[derive(Clone, Debug, Default)]
pub struct LangfuseTrace {
    pub session_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct LangfuseSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStatus {
    Error,
}

pub fn shutdown_langfuse() {}

pub fn create_trace(
    _session_id: &str,
    _model: &str,
    _provider: &str,
    _input: &str,
    _query_source: Option<&str>,
) -> Option<LangfuseTrace> {
    None
}

pub fn create_subagent_trace(
    _session_id: &str,
    _agent_type: &str,
    _agent_id: &str,
    _model: &str,
    _provider: &str,
    _input: &str,
) -> Option<LangfuseTrace> {
    None
}

pub fn create_generation_span(
    _root: &LangfuseTrace,
    _model: &str,
    _provider: &str,
    _input: Value,
) -> Option<LangfuseSpan> {
    None
}

pub fn finish_generation_span(
    _span: Option<LangfuseSpan>,
    _output: Option<Value>,
    _usage: Option<&Usage>,
    _ttft_ms: Option<u64>,
    _error: Option<&str>,
) {
}

pub fn create_tool_span(
    _root: &LangfuseTrace,
    _tool_name: &str,
    _tool_use_id: &str,
    _input: &Value,
    _parent_batch_span: Option<&LangfuseSpan>,
) -> Option<LangfuseSpan> {
    None
}

pub fn finish_tool_span(
    _span: Option<LangfuseSpan>,
    _tool_name: &str,
    _output: &str,
    _is_error: bool,
) {
}

pub fn create_tool_batch_span(
    _root: &LangfuseTrace,
    _tool_names: &[String],
    _batch_index: usize,
) -> Option<LangfuseSpan> {
    None
}

pub fn end_span(_span: Option<LangfuseSpan>) {}

pub fn end_trace(
    _trace: Option<LangfuseTrace>,
    _output: Option<&str>,
    _status: Option<TraceStatus>,
) {
}

/// Stub: no-op when telemetry feature is disabled.
pub fn bridge_from_telemetry() -> Option<LangfuseTrace> {
    None
}

/// Stub: no-op when telemetry feature is disabled.
pub fn flush_telemetry_to_langfuse() {}
