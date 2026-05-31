use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

use allthecodes_engine::types::tool::{Tool, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, ToolResultContent, UserMessage,
};

use crate::langfuse::convert::{convert_assistant_output, convert_generation_input, convert_tools};

#[test]
fn generation_input_includes_system_user_messages_and_tools() {
    let messages = vec![user_text("hello")];
    let system_prompt = vec!["system one".to_string(), "system two".to_string()];
    let tools: Tools = vec![Arc::new(DummyTool::new("Read"))];

    let converted = convert_generation_input(&messages, &system_prompt, &tools);

    assert_eq!(converted["messages"][0]["role"], "system");
    assert_eq!(converted["messages"][1]["role"], "user");
    assert_eq!(converted["tools"][0]["function"]["name"], "Read");
}

#[test]
fn assistant_output_contains_sanitized_tool_calls() {
    let assistant = assistant(vec![
        ContentBlock::Text {
            text: "done".to_string(),
        },
        ContentBlock::ToolUse {
            id: "toolu_1".to_string(),
            name: "Bash".to_string(),
            input: json!({"command": "echo ok", "token": "sk-secret"}),
        },
    ]);

    let converted = convert_assistant_output(&assistant);
    let arguments = converted["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .expect("arguments should serialize as string");

    assert_eq!(converted["role"], "assistant");
    assert!(arguments.contains("echo ok"));
    assert!(!arguments.contains("sk-secret"));
}

#[test]
fn generation_input_maps_tool_results_to_original_tool_name() {
    let messages = vec![
        Message::Assistant(assistant(vec![ContentBlock::ToolUse {
            id: "toolu_read".to_string(),
            name: "Read".to_string(),
            input: json!({"file_path": "/tmp/a.txt"}),
        }])),
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 2,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_read".to_string(),
                content: ToolResultContent::Text("secret file content".to_string()),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }),
    ];
    let tools: Tools = Vec::new();

    let converted = convert_generation_input(&messages, &[], &tools);

    assert_eq!(converted["messages"][1]["role"], "tool");
    assert_eq!(converted["messages"][1]["name"], "Read");
    assert_eq!(
        converted["messages"][1]["content"],
        "[file content redacted, 19 chars]"
    );
}

#[test]
fn generation_input_omits_non_text_blocks_with_placeholders() {
    let messages = vec![Message::Assistant(assistant(vec![
        ContentBlock::Thinking {
            thinking: "private chain".to_string(),
            signature: None,
        },
        ContentBlock::ServerToolUse {
            id: "srv_1".to_string(),
            name: "web_search".to_string(),
            input: json!({"query": "x"}),
        },
    ]))];
    let tools: Tools = Vec::new();

    let converted = convert_generation_input(&messages, &[], &tools);
    let content = converted["messages"][0]["content"]
        .as_str()
        .expect("assistant content should be string");

    assert!(content.contains("[thinking redacted]"));
    assert!(content.contains("[server tool use omitted]"));
}

#[test]
fn tool_conversion_preserves_schema() {
    let tools: Tools = vec![Arc::new(DummyTool::new("Write"))];

    let converted = convert_tools(&tools);

    assert_eq!(converted[0]["type"], "function");
    assert_eq!(converted[0]["function"]["name"], "Write");
    assert_eq!(
        converted[0]["function"]["parameters"]["properties"]["file_path"]["type"],
        "string"
    );
}

fn user_text(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1,
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

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

struct DummyTool {
    name: &'static str,
}

impl DummyTool {
    fn new(name: &'static str) -> Self {
        Self { name }
    }
}

#[async_trait]
impl Tool for DummyTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self, _input: &Value) -> String {
        "dummy".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string"},
            },
        })
    }

    async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        ValidationResult::Ok
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<
            Box<dyn Fn(allthecodes_engine::types::tool::ToolProgress) + Send + Sync>,
        >,
    ) -> Result<ToolResult> {
        Ok(ToolResult::default())
    }

    async fn prompt(&self) -> String {
        "dummy prompt".to_string()
    }
}
