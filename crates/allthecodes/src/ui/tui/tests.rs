use super::command_availability::{
    slash_command_availability_during_task, TaskCommandAvailability,
};
use super::commands::query_prompt_text;
use super::engine_events::{
    create_user_message, handle_sdk_message, handle_tool_progress, now_ts,
    progress_message_from_tool_progress, StreamingState,
};
use super::subsystem_events::handle_subsystem_event;
use super::{clear_proactive_sleep_for_user_submit, reject_unavailable_streaming_command};
use crate::ui::app::App;
use allthecodes_config::features::{self, FeatureFlags};
use allthecodes_engine::types::tool::ToolProgress;
use allthecodes_ipc_protocol::subsystem_events::{LspEvent, SubsystemEvent};
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_services::skill_search_prefetch::{
    collect_skill_discovery_prefetch, start_skill_discovery_prefetch, SkillPrefetchContext,
};
use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
use allthecodes_types::brief::{BriefMessagePayload, BriefMessageStatus};
use allthecodes_types::message::{
    ContentBlock, InfoLevel, Message, MessageContent, StreamEvent, SystemMessage, SystemSubtype,
    ToolResultContent, UserMessage,
};
use allthecodes_types::sdk::{
    ResultSubtype, SdkAssistantMessage, SdkMessage, SdkResult, SdkStreamEvent, SdkTombstone,
    SdkUserReplay, UsageTracking,
};
use serde_json::json;
use serial_test::serial;
fn stream_event(event: StreamEvent) -> SdkMessage {
    SdkMessage::StreamEvent(SdkStreamEvent {
        event,
        session_id: "test-session".to_string(),
        uuid: uuid::Uuid::new_v4(),
    })
}

fn last_assistant_blocks(app: &App) -> &[ContentBlock] {
    match app.messages().last().expect("message exists") {
        Message::Assistant(assistant) => &assistant.content,
        other => panic!("expected assistant message, got {:?}", other),
    }
}

struct FeatureGuard;

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

impl FeatureGuard {
    fn set(flags: FeatureFlags) -> Self {
        features::set_runtime_override(flags);
        Self
    }
}

impl Drop for FeatureGuard {
    fn drop(&mut self) {
        features::clear_runtime_override();
    }
}

struct SkillRegistryGuard;

impl SkillRegistryGuard {
    fn new(skills: Vec<SkillDefinition>) -> Self {
        allthecodes_skills::clear_skills();
        for skill in skills {
            allthecodes_skills::register_skill(skill);
        }
        Self
    }
}

impl Drop for SkillRegistryGuard {
    fn drop(&mut self) {
        allthecodes_skills::clear_skills();
    }
}

fn make_skill(name: &str, description: &str) -> SkillDefinition {
    SkillDefinition {
        name: name.to_string(),
        source: SkillSource::User,
        base_dir: None,
        frontmatter: SkillFrontmatter {
            name: Some(name.to_string()),
            description: description.to_string(),
            when_to_use: Some(format!("Use {name} locally")),
            ..Default::default()
        },
        prompt_body: "local prompt body".to_string(),
    }
}

#[test]
fn query_prompt_text_uses_returned_user_message_content() {
    let msgs = vec![Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        role: "user".to_string(),
        content: MessageContent::Text("Please recap the session".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })];

    assert_eq!(
        query_prompt_text(&msgs),
        "Please recap the session".to_string()
    );
}

#[test]
fn query_prompt_text_joins_multiple_messages_with_spacing() {
    let msgs = vec![
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: now_ts(),
            role: "user".to_string(),
            content: MessageContent::Text("First".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }),
        Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: now_ts(),
            subtype: SystemSubtype::Informational {
                level: InfoLevel::Info,
            },
            content: "Second".to_string(),
        }),
    ];

    assert_eq!(query_prompt_text(&msgs), "First\n\nSecond".to_string());
}

#[test]
fn streaming_disabled_slash_command_is_restored_with_error() {
    let mut app = App::new();
    app.set_streaming(true);
    app.restore_prompt_text("/review these changes".to_string());

    assert!(reject_unavailable_streaming_command(
        "/review these changes".to_string(),
        &mut app,
    ));

    assert_eq!(app.prompt_text(), "/review these changes");
    match app.messages().last().expect("system error message") {
        Message::System(message) => {
            assert_eq!(
                message.content,
                "'/review' is disabled while a task is in progress."
            );
        }
        other => panic!("expected system message, got {other:?}"),
    }
}

#[test]
fn streaming_ordinary_text_and_available_slash_command_are_not_rejected() {
    let mut app = App::new();

    assert!(!reject_unavailable_streaming_command(
        "keep going".to_string(),
        &mut app,
    ));
    assert!(!reject_unavailable_streaming_command(
        "/status".to_string(),
        &mut app,
    ));
    assert_eq!(
        slash_command_availability_during_task("/diff"),
        TaskCommandAvailability::Allowed
    );
}

#[test]
#[serial]
fn user_submit_clears_proactive_sleep_state() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());

    allthecodes_services::proactive::write_sleep_state(60, "waiting").unwrap();
    assert!(allthecodes_services::proactive::active_sleep_state()
        .unwrap()
        .is_some());

    clear_proactive_sleep_for_user_submit();

    assert!(allthecodes_services::proactive::active_sleep_state()
        .unwrap()
        .is_none());
}

#[test]
fn tui_streaming_preserves_empty_thinking_block_without_replacing_user_prompt() {
    let mut app = App::new();
    app.add_message(create_user_message("run a tool"));
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        }),
        &mut state,
    );

    assert_eq!(app.messages().len(), 2);
    assert!(matches!(app.messages()[0], Message::User(_)));
    match &last_assistant_blocks(&app)[0] {
        ContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, ""),
        other => panic!("expected empty thinking block, got {:?}", other),
    }
}

#[test]
fn tui_streaming_keeps_tool_use_after_empty_thinking_block() {
    let mut app = App::new();
    app.add_message(create_user_message("read cargo"));
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        }),
        &mut state,
    );
    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 1,
            content_block: ContentBlock::ToolUse {
                id: "call_empty_reasoning".to_string(),
                name: "Read".to_string(),
                input: json!({"file_path": "Cargo.toml"}),
            },
        }),
        &mut state,
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0], ContentBlock::Thinking { .. }));
    match &blocks[1] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_empty_reasoning");
            assert_eq!(name, "Read");
            assert_eq!(input["file_path"], "Cargo.toml");
        }
        other => panic!("expected tool use block, got {:?}", other),
    }
}

#[test]
fn tui_ignores_tool_input_delta_until_final_assistant() {
    let mut app = App::new();
    app.add_message(create_user_message("read cargo"));
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::ToolUse {
                id: "toolu_streamed".to_string(),
                name: "Read".to_string(),
                input: json!({}),
            },
        }),
        &mut state,
    );
    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": "{\"file_path\":\"Cargo.toml\"}"
            }),
        }),
        &mut state,
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &json!({}));
        }
        other => panic!("expected partial tool use block, got {:?}", other),
    }

    handle_sdk_message(
        &mut app,
        SdkMessage::Assistant(SdkAssistantMessage {
            message: allthecodes_types::message::AssistantMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: now_ts(),
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_streamed".to_string(),
                    name: "Read".to_string(),
                    input: json!({"file_path": "Cargo.toml"}),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.0,
            },
            session_id: "test-session".to_string(),
            parent_tool_use_id: None,
        }),
        &mut state,
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_streamed");
            assert_eq!(name, "Read");
            assert_eq!(input["file_path"], "Cargo.toml");
        }
        other => panic!("expected final tool use block, got {:?}", other),
    }
}

#[test]
fn tui_ignores_unsupported_text_like_delta() {
    let mut app = App::new();
    app.add_message(create_user_message("stream connector text"));
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        }),
        &mut state,
    );
    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "connector_text_delta",
                "text": "not assistant text"
            }),
        }),
        &mut state,
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::Text { text } => assert_eq!(text, ""),
        other => panic!("expected text block, got {:?}", other),
    }
}

#[test]
fn tui_tombstone_removes_partial_streaming_assistant() {
    let mut app = App::new();
    app.add_message(create_user_message("fallback please"));
    let mut state = StreamingState::new();
    let tombstone_message = allthecodes_types::message::AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: "orphaned".to_string(),
        }],
        usage: None,
        stop_reason: None,
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    };

    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        }),
        &mut state,
    );
    handle_sdk_message(
        &mut app,
        stream_event(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: json!({
                "type": "text_delta",
                "text": "orphaned"
            }),
        }),
        &mut state,
    );
    assert_eq!(app.messages().len(), 2);

    handle_sdk_message(
        &mut app,
        SdkMessage::Tombstone(SdkTombstone {
            message: tombstone_message,
            session_id: "test-session".to_string(),
            uuid: uuid::Uuid::new_v4(),
        }),
        &mut state,
    );

    assert_eq!(app.messages().len(), 1);
    assert!(matches!(app.messages()[0], Message::User(_)));
}

#[test]
fn tui_user_replay_preserves_tool_result_preview() {
    let mut app = App::new();
    let mut state = StreamingState::new();
    let source_uuid = uuid::Uuid::new_v4();

    handle_sdk_message(
        &mut app,
        SdkMessage::UserReplay(SdkUserReplay {
            content: "[1 content blocks]".to_string(),
            session_id: "test-session".to_string(),
            uuid: uuid::Uuid::new_v4(),
            timestamp: now_ts(),
            is_replay: true,
            is_synthetic: true,
            tool_use_result: Some("{\"kind\":\"file_edit\"}".to_string()),
            source_tool_assistant_uuid: Some(source_uuid),
            content_blocks: Some(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: ToolResultContent::Text("The file was updated.".to_string()),
                is_error: false,
            }]),
        }),
        &mut state,
    );

    match app.messages().last().expect("user replay message") {
        Message::User(user) => {
            assert_eq!(
                user.tool_use_result.as_deref(),
                Some("{\"kind\":\"file_edit\"}")
            );
            assert_eq!(user.source_tool_assistant_uuid, Some(source_uuid));
        }
        other => panic!("expected user message, got {:?}", other),
    }
}

#[test]
fn tui_sdk_brief_message_adds_assistant_text_message() {
    let mut app = App::new();
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        SdkMessage::BriefMessage(BriefMessagePayload {
            message: "Build finished.".to_string(),
            status: BriefMessageStatus::Normal,
            attachments: Vec::new(),
            level: None,
            source_tool_name: Some("Brief".to_string()),
            tool_use_id: Some("toolu_brief".to_string()),
            session_id: Some("test-session".to_string()),
            timestamp: Some(now_ts()),
        }),
        &mut state,
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Build finished."),
        other => panic!("expected brief text block, got {other:?}"),
    }
}

#[test]
fn tui_backend_brief_message_adds_assistant_text_message() {
    let mut app = App::new();

    super::handle_agent_backend_messages(
        &mut app,
        vec![BackendMessage::BriefMessage {
            message: "Worker finished.".to_string(),
            status: "normal".to_string(),
            attachments: Vec::new(),
            level: None,
            source_tool_name: Some("Brief".to_string()),
            tool_use_id: Some("toolu_brief".to_string()),
            session_id: Some("test-session".to_string()),
            timestamp: Some(now_ts()),
        }],
    );

    let blocks = last_assistant_blocks(&app);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Worker finished."),
        other => panic!("expected backend brief text block, got {other:?}"),
    }
}

#[test]
fn tui_tool_progress_updates_existing_progress_message() {
    let mut app = App::new();

    let first = progress_message_from_tool_progress(ToolProgress {
        tool_use_id: "toolu_1".to_string(),
        data: json!({
            "tool": "Bash",
            "output": "hello",
            "elapsed_seconds": 2,
            "total_lines": 1,
        }),
    });
    handle_tool_progress(&mut app, first);

    assert_eq!(app.messages().len(), 1);
    let Message::Progress(progress) = &app.messages()[0] else {
        panic!("expected progress message");
    };
    assert_eq!(progress.tool_use_id, "toolu_1");
    assert_eq!(progress.data["message"], "Bash running 2s; 1 line; hello");

    let second = progress_message_from_tool_progress(ToolProgress {
        tool_use_id: "toolu_1".to_string(),
        data: json!({
            "tool": "Bash",
            "output": "hello\nworld",
            "elapsed_seconds": 3,
            "total_lines": 2,
        }),
    });
    handle_tool_progress(&mut app, second);

    assert_eq!(
        app.messages().len(),
        1,
        "same tool progress should replace the previous progress message"
    );
    let Message::Progress(progress) = &app.messages()[0] else {
        panic!("expected progress message");
    };
    assert_eq!(
        progress.data["message"],
        "Bash running 3s; 2 lines; hello world"
    );
}

#[test]
#[serial]
fn tui_suggestions_include_turn_zero_skill_discovery_tip() {
    let mut flags = FeatureFlags::all_disabled();
    flags.proactive = true;
    flags.experimental_skill_search = true;
    let _features = FeatureGuard::set(flags);
    let _skills = SkillRegistryGuard::new(vec![make_skill(
        "rust-review",
        "Review Rust code without remote fetch",
    )]);
    collect_skill_discovery_prefetch(start_skill_discovery_prefetch(SkillPrefetchContext {
        session_id: "test-session".to_string(),
        query: "rust".to_string(),
    }));
    let mut app = App::new();
    app.set_session_id("test-session".to_string());
    app.add_message(create_user_message("I need help with Rust code"));
    app.add_message(Message::Assistant(
        allthecodes_types::message::AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: now_ts(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "I can help inspect the implementation.".to_string(),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
    ));
    let mut state = StreamingState::new();

    handle_sdk_message(
        &mut app,
        SdkMessage::Result(SdkResult {
            subtype: ResultSubtype::Success,
            is_error: false,
            duration_ms: 0,
            duration_api_ms: 0,
            num_turns: 1,
            result: "ok".to_string(),
            stop_reason: Some("end_turn".to_string()),
            session_id: "test-session".to_string(),
            total_cost_usd: 0.0,
            usage: UsageTracking::default(),
            permission_denials: vec![],
            structured_output: None,
            uuid: uuid::Uuid::new_v4(),
            errors: vec![],
        }),
        &mut state,
    );

    assert!(
        app.suggestions()
            .unwrap_or_default()
            .iter()
            .any(|suggestion| suggestion.text.contains("/skills rust-review")),
        "TUI suggestions should include turn-zero skill discovery"
    );
}

#[test]
fn lsp_recommendation_event_opens_command_surface() {
    let mut app = App::new();

    handle_subsystem_event(
        &mut app,
        SubsystemEvent::Lsp(LspEvent::RecommendationRequest {
            payload: allthecodes_ipc_protocol::subsystem_types::LspRecommendationPayload {
                request_id: "req-1".to_string(),
                plugin_name: "rust-analyzer".to_string(),
                plugin_description: Some("Rust language server".to_string()),
                file_extension: ".rs".to_string(),
                language_id: Some("rust".to_string()),
            },
        }),
    );

    assert!(app.command_surface_active());
}
