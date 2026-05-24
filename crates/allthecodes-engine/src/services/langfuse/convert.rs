use std::collections::HashMap;

use serde_json::{json, Value};

use crate::types::tool::Tools;
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, ToolResultContent,
};

use super::sanitize::{
    sanitize_global_string, sanitize_global_value, sanitize_tool_output, serialize_sanitized_value,
};

pub fn convert_generation_input(
    messages: &[Message],
    system_prompt: &[String],
    tools: &Tools,
) -> Value {
    json!({
        "messages": convert_messages(messages, system_prompt),
        "tools": convert_tools(tools),
    })
}

pub fn convert_assistant_output(message: &AssistantMessage) -> Value {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &message.content {
        match block {
            ContentBlock::Text { text } => text_parts.push(sanitize_global_string(text)),
            ContentBlock::ConnectorText { connector_text, .. } => {
                text_parts.push(sanitize_global_string(connector_text));
            }
            ContentBlock::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serialize_sanitized_value(&sanitize_global_value(input)),
                },
            })),
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                text_parts.push("[thinking redacted]".to_string());
            }
            ContentBlock::Image { .. } => text_parts.push("[image omitted]".to_string()),
            ContentBlock::ServerToolUse { .. } => {
                text_parts.push("[server tool use omitted]".to_string());
            }
            ContentBlock::ToolResult { .. } => {}
        }
    }

    let mut result = json!({
        "role": "assistant",
        "content": text_parts.join("\n\n"),
    });

    if !tool_calls.is_empty() {
        result["tool_calls"] = Value::Array(tool_calls);
    }

    result
}

pub fn convert_tools(tools: &Tools) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "parameters": tool.input_json_schema(),
                    }
                })
            })
            .collect(),
    )
}

fn convert_messages(messages: &[Message], system_prompt: &[String]) -> Value {
    let mut converted = Vec::new();
    let mut tool_names_by_id = HashMap::new();

    if !system_prompt.is_empty() {
        converted.push(json!({
            "role": "system",
            "content": sanitize_global_string(&system_prompt.join("\n\n")),
        }));
    }

    for message in messages {
        match message {
            Message::User(user) => match &user.content {
                MessageContent::Text(text) => converted.push(json!({
                    "role": "user",
                    "content": sanitize_global_string(text),
                })),
                MessageContent::Blocks(blocks) => {
                    let mut text_parts = Vec::new();
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                text_parts.push(sanitize_global_string(text));
                            }
                            ContentBlock::ConnectorText { connector_text, .. } => {
                                text_parts.push(sanitize_global_string(connector_text));
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error: _,
                            } => {
                                let tool_name = tool_names_by_id
                                    .get(tool_use_id)
                                    .map(String::as_str)
                                    .unwrap_or("unknown");
                                let output = match content {
                                    ToolResultContent::Text(text) => {
                                        sanitize_tool_output(tool_name, text)
                                    }
                                    ToolResultContent::Blocks(_) => {
                                        "[complex tool result omitted]".to_string()
                                    }
                                };
                                converted.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "name": tool_name,
                                    "content": output,
                                }));
                            }
                            ContentBlock::Thinking { .. }
                            | ContentBlock::RedactedThinking { .. } => {
                                text_parts.push("[thinking redacted]".to_string());
                            }
                            ContentBlock::Image { .. } => {
                                text_parts.push("[image omitted]".to_string());
                            }
                            ContentBlock::ServerToolUse { .. } => {
                                text_parts.push("[server tool use omitted]".to_string());
                            }
                            ContentBlock::ToolUse { .. } => {}
                        }
                    }
                    if !text_parts.is_empty() {
                        converted.push(json!({
                            "role": "user",
                            "content": text_parts.join("\n\n"),
                        }));
                    }
                }
            },
            Message::Assistant(assistant) => {
                let mut text_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for block in &assistant.content {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(sanitize_global_string(text));
                        }
                        ContentBlock::ConnectorText { connector_text, .. } => {
                            text_parts.push(sanitize_global_string(connector_text));
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_names_by_id.insert(id.clone(), name.clone());
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serialize_sanitized_value(
                                        &sanitize_global_value(input),
                                    ),
                                },
                            }));
                        }
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                            text_parts.push("[thinking redacted]".to_string());
                        }
                        ContentBlock::Image { .. } => {
                            text_parts.push("[image omitted]".to_string());
                        }
                        ContentBlock::ServerToolUse { .. } => {
                            text_parts.push("[server tool use omitted]".to_string());
                        }
                        ContentBlock::ToolResult { .. } => {}
                    }
                }

                let mut message = json!({
                    "role": "assistant",
                    "content": text_parts.join("\n\n"),
                });
                if !tool_calls.is_empty() {
                    message["tool_calls"] = Value::Array(tool_calls);
                }
                converted.push(message);
            }
            _ => {}
        }
    }

    Value::Array(converted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, UserMessage};
    use uuid::Uuid;

    #[test]
    fn convert_generation_input_maps_tool_use_and_result() {
        let tool_id = "tool-1".to_string();
        let assistant = Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: tool_id.clone(),
                name: "Bash".to_string(),
                input: json!({"command": "pwd"}),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        });
        let user = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 2,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: ToolResultContent::Text("output".to_string()),
                is_error: false,
            }]),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        let converted =
            convert_generation_input(&[assistant, user], &["system".to_string()], &vec![]);
        let messages = converted["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "tool");
    }
}
