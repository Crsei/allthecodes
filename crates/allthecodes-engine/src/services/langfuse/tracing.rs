use opentelemetry::trace::Status;
use serde_json::{json, Value};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use allthecodes_types::message::Usage;

use super::sanitize::{
    metadata_json, sanitize_global, sanitize_global_string, sanitize_tool_input,
    sanitize_tool_output, serialize_sanitized_value,
};

const TRACE_SESSION_ID_ATTR: &str = "langfuse.session.id";
const TRACE_USER_ID_ATTR: &str = "langfuse.user.id";
const TRACE_NAME_ATTR: &str = "langfuse.trace.name";
const TRACE_METADATA_ATTR: &str = "langfuse.trace.metadata";
const TRACE_TAGS_ATTR: &str = "langfuse.trace.tags";
const OBSERVATION_TYPE_ATTR: &str = "langfuse.observation.type";
const OBSERVATION_METADATA_ATTR: &str = "langfuse.observation.metadata";
const INPUT_VALUE_ATTR: &str = "input.value";
const OUTPUT_VALUE_ATTR: &str = "output.value";
const LLM_MODEL_ATTR: &str = "llm.model_name";
const LLM_PROMPT_TOKENS_ATTR: &str = "llm.token_count.prompt";
const LLM_COMPLETION_TOKENS_ATTR: &str = "llm.token_count.completion";
const LLM_TOTAL_TOKENS_ATTR: &str = "llm.token_count.total";

#[derive(Clone, Debug)]
pub struct LangfuseTrace {
    pub(crate) span: tracing::Span,
    pub session_id: String,
    pub user_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LangfuseSpan {
    pub(crate) span: tracing::Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStatus {
    Error,
}

pub fn create_trace(
    session_id: &str,
    model: &str,
    provider: &str,
    input: &str,
    query_source: Option<&str>,
) -> Option<LangfuseTrace> {
    let trace_name = match query_source {
        Some(source) if !source.is_empty() => format!("agent-run:{}", source),
        _ => "agent-run".to_string(),
    };
    let user_id = resolve_langfuse_user_id();
    let span = tracing::info_span!("langfuse.trace", otel.name = trace_name.as_str());
    apply_trace_identity(&span, session_id, user_id.as_deref());
    span.set_attribute(OBSERVATION_TYPE_ATTR, "agent");
    span.set_attribute(TRACE_NAME_ATTR, trace_name.clone());
    span.set_attribute(INPUT_VALUE_ATTR, sanitize_global_string(input));
    span.set_attribute(
        TRACE_METADATA_ATTR,
        metadata_json(vec![
            ("provider", Value::String(provider.to_string())),
            ("model", Value::String(model.to_string())),
            ("agentType", Value::String("main".to_string())),
            (
                "querySource",
                query_source
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            ),
        ]),
    );
    span.set_attribute(
        TRACE_TAGS_ATTR,
        json!(["allthecodes", query_source.unwrap_or("submit")]).to_string(),
    );

    Some(LangfuseTrace {
        span,
        session_id: session_id.to_string(),
        user_id,
    })
}

pub fn create_subagent_trace(
    session_id: &str,
    agent_type: &str,
    agent_id: &str,
    model: &str,
    provider: &str,
    input: &str,
) -> Option<LangfuseTrace> {
    let trace_name = format!("agent:{}", agent_type);
    let user_id = resolve_langfuse_user_id();
    let span = tracing::info_span!("langfuse.subagent", otel.name = trace_name.as_str());
    apply_trace_identity(&span, session_id, user_id.as_deref());
    span.set_attribute(OBSERVATION_TYPE_ATTR, "agent");
    span.set_attribute(TRACE_NAME_ATTR, trace_name.clone());
    span.set_attribute(INPUT_VALUE_ATTR, sanitize_global_string(input));
    span.set_attribute(
        TRACE_METADATA_ATTR,
        metadata_json(vec![
            ("provider", Value::String(provider.to_string())),
            ("model", Value::String(model.to_string())),
            ("agentType", Value::String(agent_type.to_string())),
            ("agentId", Value::String(agent_id.to_string())),
        ]),
    );
    span.set_attribute(
        TRACE_TAGS_ATTR,
        json!(["allthecodes", "subagent", agent_type]).to_string(),
    );

    Some(LangfuseTrace {
        span,
        session_id: session_id.to_string(),
        user_id,
    })
}

pub fn create_generation_span(
    root: &LangfuseTrace,
    model: &str,
    provider: &str,
    input: Value,
) -> Option<LangfuseSpan> {
    let display_name = generation_name(provider);
    let span = tracing::info_span!(
        parent: &root.span,
        "langfuse.generation",
        otel.name = display_name
    );
    apply_trace_identity(&span, &root.session_id, root.user_id.as_deref());
    span.set_attribute(OBSERVATION_TYPE_ATTR, "generation");
    span.set_attribute(LLM_MODEL_ATTR, model.to_string());
    span.set_attribute(
        OBSERVATION_METADATA_ATTR,
        metadata_json(vec![
            ("provider", Value::String(provider.to_string())),
            ("model", Value::String(model.to_string())),
        ]),
    );
    span.set_attribute(
        INPUT_VALUE_ATTR,
        serialize_sanitized_value(&sanitize_global(&input)),
    );

    Some(LangfuseSpan { span })
}

pub fn finish_generation_span(
    span: Option<LangfuseSpan>,
    output: Option<Value>,
    usage: Option<&Usage>,
    ttft_ms: Option<u64>,
    error: Option<&str>,
) {
    let Some(span) = span else {
        return;
    };

    if let Some(output) = output {
        span.span.set_attribute(
            OUTPUT_VALUE_ATTR,
            serialize_sanitized_value(&sanitize_global(&output)),
        );
    }

    if let Some(ttft_ms) = ttft_ms {
        record_completion_start_event(&span.span, ttft_ms);
    }

    if let Some(usage) = usage {
        let total = usage
            .input_tokens
            .saturating_add(usage.output_tokens)
            .saturating_add(usage.cache_creation_input_tokens)
            .saturating_add(usage.cache_read_input_tokens);
        span.span
            .set_attribute(LLM_PROMPT_TOKENS_ATTR, usage.input_tokens as i64);
        span.span
            .set_attribute(LLM_COMPLETION_TOKENS_ATTR, usage.output_tokens as i64);
        span.span.set_attribute(LLM_TOTAL_TOKENS_ATTR, total as i64);
        span.span.set_attribute(
            OBSERVATION_METADATA_ATTR,
            metadata_json(vec![
                (
                    "ttftMs",
                    ttft_ms.map(|value| json!(value)).unwrap_or(Value::Null),
                ),
                ("cacheReadInputTokens", json!(usage.cache_read_input_tokens)),
                (
                    "cacheCreationInputTokens",
                    json!(usage.cache_creation_input_tokens),
                ),
            ]),
        );
    } else if let Some(ttft_ms) = ttft_ms {
        span.span.set_attribute(
            OBSERVATION_METADATA_ATTR,
            metadata_json(vec![("ttftMs", json!(ttft_ms))]),
        );
    }

    if let Some(error) = error {
        span.span
            .set_status(Status::error(sanitize_global_string(error)));
        span.span.set_attribute(
            OBSERVATION_METADATA_ATTR,
            metadata_json(vec![
                ("error", Value::String(sanitize_global_string(error))),
                (
                    "ttftMs",
                    ttft_ms.map(|value| json!(value)).unwrap_or(Value::Null),
                ),
            ]),
        );
    }
}

pub fn create_tool_span(
    root: &LangfuseTrace,
    tool_name: &str,
    tool_use_id: &str,
    input: &Value,
    parent_batch_span: Option<&LangfuseSpan>,
) -> Option<LangfuseSpan> {
    let parent = parent_batch_span
        .map(|span| &span.span)
        .unwrap_or(&root.span);
    let span = tracing::info_span!(parent: parent, "langfuse.tool", otel.name = tool_name);
    apply_trace_identity(&span, &root.session_id, root.user_id.as_deref());
    span.set_attribute(OBSERVATION_TYPE_ATTR, "tool");
    span.set_attribute(
        OBSERVATION_METADATA_ATTR,
        metadata_json(vec![("toolUseId", Value::String(tool_use_id.to_string()))]),
    );
    span.set_attribute(
        INPUT_VALUE_ATTR,
        serialize_sanitized_value(&sanitize_tool_input(tool_name, input)),
    );

    Some(LangfuseSpan { span })
}

pub fn finish_tool_span(span: Option<LangfuseSpan>, tool_name: &str, output: &str, is_error: bool) {
    let Some(span) = span else {
        return;
    };

    span.span
        .set_attribute(OUTPUT_VALUE_ATTR, sanitize_tool_output(tool_name, output));
    span.span.set_attribute(
        OBSERVATION_METADATA_ATTR,
        metadata_json(vec![("isError", Value::Bool(is_error))]),
    );
    if is_error {
        span.span
            .set_status(Status::error(sanitize_tool_output(tool_name, output)));
    }
}

pub fn create_tool_batch_span(
    root: &LangfuseTrace,
    tool_names: &[String],
    batch_index: usize,
) -> Option<LangfuseSpan> {
    let span = tracing::info_span!(parent: &root.span, "langfuse.tool_batch", otel.name = "tools");
    apply_trace_identity(&span, &root.session_id, root.user_id.as_deref());
    span.set_attribute(OBSERVATION_TYPE_ATTR, "span");
    span.set_attribute(
        OBSERVATION_METADATA_ATTR,
        metadata_json(vec![
            ("toolNames", json!(tool_names)),
            ("toolCount", json!(tool_names.len())),
            ("batchIndex", json!(batch_index)),
        ]),
    );

    Some(LangfuseSpan { span })
}

pub fn end_span(span: Option<LangfuseSpan>) {
    drop(span);
}

pub fn end_trace(trace: Option<LangfuseTrace>, output: Option<&str>, status: Option<TraceStatus>) {
    let Some(trace) = trace else {
        return;
    };

    if let Some(output) = output {
        trace
            .span
            .set_attribute(OUTPUT_VALUE_ATTR, sanitize_global_string(output));
    }

    match status {
        Some(TraceStatus::Error) => trace.span.set_status(Status::error("error".to_string())),
        None => {}
    }
}

fn apply_trace_identity(span: &tracing::Span, session_id: &str, user_id: Option<&str>) {
    span.set_attribute(TRACE_SESSION_ID_ATTR, session_id.to_string());
    if let Some(user_id) = user_id {
        span.set_attribute(TRACE_USER_ID_ATTR, user_id.to_string());
    }
}

fn resolve_langfuse_user_id() -> Option<String> {
    for candidate in ["LANGFUSE_USER_ID", "USERNAME", "USER"] {
        let Ok(value) = std::env::var(candidate) else {
            continue;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn generation_name(provider: &str) -> &str {
    match provider {
        "anthropic" => "ChatAnthropic",
        "bedrock" => "ChatBedrockAnthropic",
        "vertex" => "ChatVertexAnthropic",
        "azure" | "azure-foundry" | "microsoft-foundry" | "foundry" => "ChatAzureOpenAI",
        "google" | "gemini" => "ChatGoogleGenerativeAI",
        "openai" | "openai-codex" => "ChatOpenAI",
        _ => "ChatModel",
    }
}

fn record_completion_start_event(span: &tracing::Span, ttft_ms: u64) {
    let ttft_ms = ttft_ms as i64;
    span.in_scope(|| {
        tracing::info!(ttft_ms, "completion_start");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_name_maps_known_providers() {
        assert_eq!(generation_name("anthropic"), "ChatAnthropic");
        assert_eq!(generation_name("bedrock"), "ChatBedrockAnthropic");
        assert_eq!(generation_name("openai"), "ChatOpenAI");
        assert_eq!(generation_name("azure-foundry"), "ChatAzureOpenAI");
    }
}
