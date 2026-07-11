use super::*;
use crate::types::tool::Tool;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct MockTool {
    name: &'static str,
    mcp_server: Option<&'static str>,
}

#[async_trait::async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self, _input: &serde_json::Value) -> String {
        String::new()
    }

    fn input_json_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent_message: &crate::types::message::AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<crate::types::tool::ToolResult> {
        Ok(Default::default())
    }

    async fn prompt(&self) -> String {
        String::new()
    }

    fn mcp_server_name(&self) -> Option<&str> {
        self.mcp_server
    }
}

fn agent_definition(source: AgentDefinitionSource) -> AgentDefinitionEntry {
    AgentDefinitionEntry {
        name: "Explore".to_string(),
        description: "explore".to_string(),
        system_prompt: "prompt".to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        model: None,
        color: None,
        permission_mode: None,
        memory: None,
        max_turns: None,
        effort: None,
        background: false,
        isolation: None,
        skills: vec![],
        hooks: serde_json::Value::Null,
        mcp_servers: vec![],
        initial_prompt: None,
        filename: None,
        source,
        file_path: None,
    }
}

#[test]
fn delegated_agent_envelope_round_trips_without_schema_exposure() {
    let parsed: AgentInput = serde_json::from_value(json!({
        "prompt": "inspect parser",
        "description": "inspect parser",
        "run_in_background": true,
        "verification_policy": "targeted_tests",
        "_delegate_task_id": "task-1",
        "_delegate_task_list_id": "parent-session",
        "_delegate_session_id": "session-1",
        "_delegate_cwd": "/tmp/project",
        "_delegate_worktree_slug": "parser-check"
    }))
    .unwrap();
    assert_eq!(parsed.delegate_task_id.as_deref(), Some("task-1"));
    assert_eq!(
        parsed.delegate_task_list_id.as_deref(),
        Some("parent-session")
    );
    assert_eq!(parsed.delegate_session_id.as_deref(), Some("session-1"));
    assert_eq!(parsed.delegate_cwd.as_deref(), Some("/tmp/project"));
    assert_eq!(
        parsed.delegate_worktree_slug.as_deref(),
        Some("parser-check")
    );
    assert_eq!(
        parsed.verification_policy.as_deref(),
        Some("targeted_tests")
    );

    let schema = AgentTool.input_json_schema();
    let properties = schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("verification_policy"));
    for hidden in [
        "_delegate_task_id",
        "_delegate_task_list_id",
        "_delegate_session_id",
        "_delegate_cwd",
        "_delegate_worktree_slug",
    ] {
        assert!(!properties.contains_key(hidden));
    }
}

#[test]
fn test_resolve_model_alias() {
    assert_eq!(
        resolve_model_alias("SOTA", "fallback").unwrap(),
        allthecodes_types::models::SOTA_MODEL_ID
    );
    assert_eq!(
        resolve_model_alias("MOTA", "fallback").unwrap(),
        allthecodes_types::models::MOTA_MODEL_ID
    );
    assert_eq!(
        resolve_model_alias("FOTA", "fallback").unwrap(),
        allthecodes_types::models::FOTA_MODEL_ID
    );
    assert!(resolve_model_alias("opus", "fallback").is_err());
    assert_eq!(
        resolve_model_alias("inherit", "fallback").unwrap(),
        "fallback"
    );
    assert_eq!(
        resolve_model_alias("custom-model", "fallback").unwrap(),
        "custom-model"
    );
}

#[test]
fn test_agent_tool_schema() {
    let tool = AgentTool;
    let schema = tool.input_json_schema();
    assert!(schema.get("properties").is_some());
    let props = schema["properties"].as_object().unwrap();
    assert!(props.contains_key("prompt"));
    assert!(props.contains_key("description"));
    assert!(props.contains_key("subagent_type"));
    assert!(props.contains_key("model"));
    assert!(props.contains_key("run_in_background"));
    assert!(props.contains_key("isolation"));
}

#[test]
fn test_agent_tool_name() {
    let tool = AgentTool;
    assert_eq!(tool.name(), "Agent");
}

#[test]
fn test_task_tool_alias_name_and_schema() {
    let tool = TaskAgentTool;
    assert_eq!(tool.name(), "Task");
    assert_eq!(tool.input_json_schema(), AgentTool.input_json_schema());
}

#[test]
fn test_agent_user_facing_name() {
    let tool = AgentTool;
    assert_eq!(tool.user_facing_name(None), "Agent");

    let input = json!({"description": "search codebase"});
    assert_eq!(
        tool.user_facing_name(Some(&input)),
        "Agent(search codebase)"
    );
}

#[test]
fn test_agent_concurrency_safe() {
    let tool = AgentTool;
    assert!(tool.is_concurrency_safe(&json!({})));
}

#[test]
fn test_agent_isolation_field() {
    // No isolation field — should be None
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "do something",
        "description": "test task"
    }))
    .unwrap();
    assert!(input.isolation.is_none());

    // Explicit worktree isolation
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "do something",
        "description": "test task",
        "isolation": "worktree"
    }))
    .unwrap();
    assert_eq!(input.isolation.as_deref(), Some("worktree"));

    // Null isolation — should be None
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "do something",
        "description": "test task",
        "isolation": null
    }))
    .unwrap();
    assert!(input.isolation.is_none());
}

#[test]
fn test_agent_input_deserialization() {
    // Minimal required fields
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "find all TODO comments"
    }))
    .unwrap();
    assert_eq!(input.prompt, "find all TODO comments");
    assert!(input.description.is_none());
    assert!(input.model.is_none());
    assert!(!input.run_in_background);
    assert!(input.isolation.is_none());

    // Full fields
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "search for bugs",
        "description": "bug search",
        "subagent_type": "Explore",
        "model": "FOTA",
        "run_in_background": true,
        "isolation": "worktree"
    }))
    .unwrap();
    assert_eq!(input.prompt, "search for bugs");
    assert_eq!(input.description.as_deref(), Some("bug search"));
    assert_eq!(input.subagent_type.as_deref(), Some("Explore"));
    assert_eq!(input.model.as_deref(), Some("FOTA"));
    assert!(input.run_in_background);
    assert_eq!(input.isolation.as_deref(), Some("worktree"));
}

#[test]
fn test_max_result_size() {
    let tool = AgentTool;
    assert_eq!(tool.max_result_size_chars(), 200_000);
}

#[test]
fn builtin_agent_does_not_get_mcp_tools_by_default() {
    let tools: Tools = vec![
        Arc::new(MockTool {
            name: "Read",
            mcp_server: None,
        }),
        Arc::new(MockTool {
            name: "mcp__github__search",
            mcp_server: Some("github"),
        }),
    ];
    let definition = agent_definition(AgentDefinitionSource::Builtin);

    let filtered = filter_tools_for_agent_definition(tools, &definition);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name(), "Read");
}

#[test]
fn builtin_agent_can_explicitly_allow_mcp_tool_by_tool_spec() {
    let tools: Tools = vec![Arc::new(MockTool {
        name: "mcp__github__search",
        mcp_server: Some("github"),
    })];
    let mut definition = agent_definition(AgentDefinitionSource::Builtin);
    definition.tools = vec!["mcp__github__search".to_string()];

    let filtered = filter_tools_for_agent_definition(tools, &definition);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name(), "mcp__github__search");
}

#[test]
fn custom_agent_mcp_servers_field_filters_visible_servers() {
    let tools: Tools = vec![
        Arc::new(MockTool {
            name: "mcp__github__search",
            mcp_server: Some("github"),
        }),
        Arc::new(MockTool {
            name: "mcp__linear__list",
            mcp_server: Some("linear"),
        }),
    ];
    let mut definition = agent_definition(AgentDefinitionSource::User);
    definition.mcp_servers = vec![json!("linear")];

    let filtered = filter_tools_for_agent_definition(tools, &definition);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].mcp_server_name(), Some("linear"));
}

#[tokio::test]
async fn test_run_in_background_without_tx_falls_back() {
    // When bg_agent_tx is None, run_in_background should parse correctly
    let input: AgentInput = serde_json::from_value(json!({
        "prompt": "test task",
        "description": "test",
        "run_in_background": true
    }))
    .unwrap();
    assert!(input.run_in_background);
}

#[tokio::test]
async fn test_background_agent_placeholder_format() {
    // Verify the placeholder message format
    let agent_id = "test-agent-123";
    let description = "search codebase";
    let placeholder = format!(
        "Agent '{}' launched in background (id: {}). You will be notified when it completes.",
        description, agent_id
    );
    assert!(placeholder.contains("search codebase"));
    assert!(placeholder.contains("test-agent-123"));
    assert!(placeholder.contains("background"));
}

// ---------------------------------------------------------------------------
// sdk_to_agent_event — SdkMessage → AgentEvent mapping
// ---------------------------------------------------------------------------

fn make_stream_event(delta: serde_json::Value) -> allthecodes_types::sdk::SdkMessage {
    allthecodes_types::sdk::SdkMessage::StreamEvent(allthecodes_types::sdk::SdkStreamEvent {
        event: crate::types::message::StreamEvent::ContentBlockDelta { index: 0, delta },
        session_id: "s1".into(),
        uuid: Uuid::nil(),
    })
}

fn make_assistant_msg(
    content: Vec<crate::types::message::ContentBlock>,
) -> allthecodes_types::sdk::SdkMessage {
    allthecodes_types::sdk::SdkMessage::Assistant(allthecodes_types::sdk::SdkAssistantMessage {
        message: crate::types::message::AssistantMessage {
            uuid: Uuid::nil(),
            timestamp: 0,
            role: "assistant".into(),
            content,
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        session_id: "s1".into(),
        parent_tool_use_id: None,
    })
}

fn make_user_replay(
    blocks: Vec<crate::types::message::ContentBlock>,
) -> allthecodes_types::sdk::SdkMessage {
    allthecodes_types::sdk::SdkMessage::UserReplay(allthecodes_types::sdk::SdkUserReplay {
        content: String::new(),
        session_id: "s1".into(),
        uuid: Uuid::nil(),
        timestamp: 0,
        is_replay: false,
        is_synthetic: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
        content_blocks: Some(blocks),
    })
}

#[test]
fn test_sdk_to_agent_event_stream_delta_text() {
    let msg = make_stream_event(json!({"text": "hello world"}));
    let event = sdk_to_agent_event(&msg, "a1").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::StreamDelta { agent_id, text } => {
            assert_eq!(agent_id, "a1");
            assert_eq!(text, "hello world");
        }
        other => panic!("expected StreamDelta, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_stream_delta_thinking() {
    let msg = make_stream_event(json!({"thinking": "let me consider"}));
    let event = sdk_to_agent_event(&msg, "a2").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ThinkingDelta { agent_id, thinking } => {
            assert_eq!(agent_id, "a2");
            assert_eq!(thinking, "let me consider");
        }
        other => panic!("expected ThinkingDelta, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_stream_delta_unknown_field_returns_none() {
    let msg = make_stream_event(json!({"other_field": 42}));
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_unsupported_text_like_delta_returns_none() {
    let msg = make_stream_event(json!({
        "type": "connector_text_delta",
        "text": "not assistant text"
    }));
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_tool_use() {
    let msg = make_assistant_msg(vec![crate::types::message::ContentBlock::ToolUse {
        id: "tu1".into(),
        name: "Bash".into(),
        input: json!({"command": "ls"}),
    }]);
    let event = sdk_to_agent_event(&msg, "a3").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ToolUse {
            agent_id,
            tool_use_id,
            tool_name,
            input,
        } => {
            assert_eq!(agent_id, "a3");
            assert_eq!(tool_use_id, "tu1");
            assert_eq!(tool_name, "Bash");
            assert_eq!(input, json!({"command": "ls"}));
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_assistant_text_only_returns_none() {
    let msg = make_assistant_msg(vec![crate::types::message::ContentBlock::Text {
        text: "just text".into(),
    }]);
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_tool_result_text() {
    let msg = make_user_replay(vec![crate::types::message::ContentBlock::ToolResult {
        tool_use_id: "tu1".into(),
        content: crate::types::message::ToolResultContent::Text("file contents".into()),
        is_error: false,
    }]);
    let event = sdk_to_agent_event(&msg, "a4").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ToolResult {
            agent_id,
            tool_use_id,
            output,
            is_error,
        } => {
            assert_eq!(agent_id, "a4");
            assert_eq!(tool_use_id, "tu1");
            assert_eq!(output, "file contents");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_tool_result_error() {
    let msg = make_user_replay(vec![crate::types::message::ContentBlock::ToolResult {
        tool_use_id: "tu2".into(),
        content: crate::types::message::ToolResultContent::Text("command failed".into()),
        is_error: true,
    }]);
    let event = sdk_to_agent_event(&msg, "a5").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ToolResult { is_error, .. } => {
            assert!(is_error);
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_tool_result_blocks_shows_placeholder() {
    let msg = make_user_replay(vec![crate::types::message::ContentBlock::ToolResult {
        tool_use_id: "tu3".into(),
        content: crate::types::message::ToolResultContent::Blocks(vec![
            crate::types::message::ContentBlock::Text {
                text: "inner".into(),
            },
        ]),
        is_error: false,
    }]);
    let event = sdk_to_agent_event(&msg, "a6").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ToolResult { output, .. } => {
            assert_eq!(output, "[complex output]");
        }
        other => panic!("expected ToolResult, got {:?}", other),
    }
}

#[test]
fn test_sdk_to_agent_event_user_replay_no_blocks_returns_none() {
    let msg =
        allthecodes_types::sdk::SdkMessage::UserReplay(allthecodes_types::sdk::SdkUserReplay {
            content: "hello".into(),
            session_id: "s1".into(),
            uuid: Uuid::nil(),
            timestamp: 0,
            is_replay: false,
            is_synthetic: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
            content_blocks: None,
        });
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_system_init_returns_none() {
    let msg =
        allthecodes_types::sdk::SdkMessage::SystemInit(allthecodes_types::sdk::SystemInitMessage {
            tools: vec![],
            model: "test".into(),
            permission_mode: "default".into(),
            session_id: "s1".into(),
            uuid: Uuid::nil(),
        });
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_stream_message_start_returns_none() {
    let msg =
        allthecodes_types::sdk::SdkMessage::StreamEvent(allthecodes_types::sdk::SdkStreamEvent {
            event: crate::types::message::StreamEvent::MessageStart {
                usage: crate::types::message::Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                },
            },
            session_id: "s1".into(),
            uuid: Uuid::nil(),
        });
    assert!(sdk_to_agent_event(&msg, "a1").is_none());
}

#[test]
fn test_sdk_to_agent_event_tool_use_picks_first() {
    // When assistant message has multiple tool uses, sdk_to_agent_event returns the first
    let msg = make_assistant_msg(vec![
        crate::types::message::ContentBlock::ToolUse {
            id: "tu-first".into(),
            name: "Read".into(),
            input: json!({"path": "/a"}),
        },
        crate::types::message::ContentBlock::ToolUse {
            id: "tu-second".into(),
            name: "Write".into(),
            input: json!({"path": "/b"}),
        },
    ]);
    let event = sdk_to_agent_event(&msg, "a7").unwrap();
    match event {
        allthecodes_types::agent_events::AgentEvent::ToolUse { tool_use_id, .. } => {
            assert_eq!(tool_use_id, "tu-first");
        }
        other => panic!("expected ToolUse, got {:?}", other),
    }
}
