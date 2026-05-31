use serde_json::Value;

use allthecodes_engine::types::tool::Tools;
use allthecodes_types::message::{AssistantMessage, Message};

pub use allthecodes_langfuse::convert::LangfuseToolDefinition;

pub fn convert_generation_input(
    messages: &[Message],
    system_prompt: &[String],
    tools: &Tools,
) -> Value {
    let tool_definitions = tool_definitions(tools);
    allthecodes_langfuse::convert::convert_generation_input(
        messages,
        system_prompt,
        &tool_definitions,
    )
}

pub fn convert_assistant_output(message: &AssistantMessage) -> Value {
    allthecodes_langfuse::convert::convert_assistant_output(message)
}

pub fn convert_tools(tools: &Tools) -> Value {
    let tool_definitions = tool_definitions(tools);
    allthecodes_langfuse::convert::convert_tools(&tool_definitions)
}

fn tool_definitions(tools: &Tools) -> Vec<LangfuseToolDefinition> {
    tools
        .iter()
        .map(|tool| LangfuseToolDefinition::new(tool.name(), tool.input_json_schema()))
        .collect()
}

/// Convert an `InteractionSpan` to a Langfuse-compatible JSON value.
#[cfg(feature = "telemetry")]
impl From<&crate::telemetry::instrumentation::InteractionSpan> for serde_json::Value {
    fn from(span: &crate::telemetry::instrumentation::InteractionSpan) -> Self {
        serde_json::json!({
            "interaction_id": span.interaction_id(),
            "session_id": span.session_id(),
            "submit_id": span.submit_id(),
            "model": span.model(),
            "duration_ms": span.duration_ms(),
            "error": span.error(),
        })
    }
}
