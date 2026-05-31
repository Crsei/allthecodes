use serde_json::Value;

use crate::types::tool::Tools;
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
