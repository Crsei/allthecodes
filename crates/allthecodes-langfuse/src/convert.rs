use std::collections::HashMap;

use serde_json::{json, Value};

use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, ToolResultContent,
};

use crate::sanitize::{
    sanitize_global_string, sanitize_global_value, sanitize_tool_output, serialize_sanitized_value,
};

#[derive(Clone, Debug, PartialEq)]
pub struct LangfuseToolDefinition {
    pub name: String,
    pub parameters: Value,
}

impl LangfuseToolDefinition {
    pub fn new(name: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            parameters,
        }
    }
}

pub fn convert_generation_input(
    messages: &[Message],
    system_prompt: &[String],
    tools: &[LangfuseToolDefinition],
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

pub fn convert_tools(tools: &[LangfuseToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "parameters": tool.parameters,
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
    use allthecodes_types::message::{ImageSource, UserMessage};
    use uuid::Uuid;

    fn assistant(content: Vec<ContentBlock>) -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content,
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn user(content: MessageContent) -> UserMessage {
        UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 2,
            role: "user".to_string(),
            content,
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }
    }

    #[test]
    fn full_conversion_roundtrip() {
        let tool_id = "tool-1".to_string();
        let messages = vec![
            Message::User(user(MessageContent::Text("hello".to_string()))),
            Message::Assistant(assistant(vec![ContentBlock::ToolUse {
                id: tool_id.clone(),
                name: "Bash".to_string(),
                input: json!({"command": "pwd"}),
            }])),
            Message::User(user(MessageContent::Blocks(vec![
                ContentBlock::ToolResult {
                    tool_use_id: tool_id,
                    content: ToolResultContent::Text("output".to_string()),
                    is_error: false,
                },
            ]))),
        ];

        let converted = convert_generation_input(
            &messages,
            &["You are a helpful assistant.".to_string()],
            &[],
        );
        let msgs = converted["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[3]["role"], "tool");
    }

    #[test]
    fn all_content_block_types_handled() {
        let message = assistant(vec![
            ContentBlock::Text {
                text: "hello".to_string(),
            },
            ContentBlock::Thinking {
                thinking: "deep thought".to_string(),
                signature: None,
            },
            ContentBlock::RedactedThinking {
                data: "redacted".to_string(),
            },
            ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "abc".to_string(),
                },
            },
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
            ContentBlock::ServerToolUse {
                id: "srv1".to_string(),
                name: "web_search".to_string(),
                input: json!({"query": "rust"}),
            },
        ]);

        let result = convert_assistant_output(&message);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("[thinking redacted]"));
        assert!(content.contains("[image omitted]"));
        assert!(content.contains("[server tool use omitted]"));
        assert_eq!(result["tool_calls"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn tool_result_blocks_omitted() {
        let message = Message::User(user(MessageContent::Blocks(vec![
            ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: ToolResultContent::Blocks(vec![]),
                is_error: false,
            },
        ])));

        let converted = convert_generation_input(&[message], &[], &[]);
        let msg = &converted["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["content"], "[complex tool result omitted]");
    }

    #[test]
    fn empty_messages() {
        let converted = convert_generation_input(&[], &[], &[]);
        assert!(converted["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tool_definitions_converted() {
        let tools = vec![LangfuseToolDefinition::new(
            "Read",
            json!({"type": "object", "properties": {"file_path": {"type": "string"}}}),
        )];

        let converted = convert_tools(&tools);
        let tool = &converted.as_array().unwrap()[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "Read");
        assert_eq!(
            tool["function"]["parameters"]["properties"]["file_path"]["type"],
            "string"
        );
    }
}
