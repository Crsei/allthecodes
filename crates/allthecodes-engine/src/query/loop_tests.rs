use super::*;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use allthecodes_types::hooks::{
    HookEventConfig, HookOutput, HookRunner, HooksMap, PostToolHookResult, PreToolHookResult,
};
use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;

use super::super::deps::{
    CompactionResult, ModelCallParams, ModelResponse, QueryDeps, ToolExecRequest, ToolExecResult,
};
use crate::types::app_state::AppState;
use crate::types::config::{QueryGates, QuerySource, TaskBudget};
use crate::types::message::{
    AssistantMessage, ContentBlock, ImageSource, MessageContent, StreamEvent, ToolResultContent,
    Usage, UserMessage,
};
use crate::types::state::AutoCompactTracking;
use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools};

#[allow(clippy::large_enum_variant)]
enum MockStreamStep {
    Response(ModelResponse),
    Error(String),
    Events(Vec<Result<StreamEvent, String>>),
    DelayedEvents(Vec<(Duration, Result<StreamEvent, String>)>),
}

/// Mock deps for testing.
struct MockDeps {
    stream_steps: parking_lot::Mutex<Vec<MockStreamStep>>,
    call_params: parking_lot::Mutex<Vec<ModelCallParams>>,
    autocompact_params: parking_lot::Mutex<Vec<ModelCallParams>>,
    collapse_drain_result: parking_lot::Mutex<Option<CompactionResult>>,
    collapse_drain_calls: AtomicUsize,
    reactive_compact_result: parking_lot::Mutex<Option<CompactionResult>>,
    reactive_compact_calls: AtomicUsize,
    aborted: AtomicBool,
    stream_finished: Arc<AtomicBool>,
    tool_executed_before_stream_finished: AtomicBool,
    tool_execution_count: AtomicUsize,
    tool_completed_count: AtomicUsize,
    hook_stopped_tool_execution: AtomicBool,
    active_tools: AtomicUsize,
    max_active_tools: AtomicUsize,
    tool_delay: Duration,
    tools: Tools,
    refreshed_tools: parking_lot::Mutex<Option<Tools>>,
    refresh_seen: AtomicBool,
    hook_runner: parking_lot::Mutex<Arc<dyn HookRunner>>,
    steer_drains: parking_lot::Mutex<VecDeque<Vec<String>>>,
    app_state: parking_lot::Mutex<AppState>,
}

impl MockDeps {
    fn new(responses: Vec<ModelResponse>) -> Self {
        Self::from_steps(
            responses
                .into_iter()
                .map(MockStreamStep::Response)
                .collect(),
        )
    }

    fn from_steps(stream_steps: Vec<MockStreamStep>) -> Self {
        Self {
            stream_steps: parking_lot::Mutex::new(stream_steps),
            call_params: parking_lot::Mutex::new(Vec::new()),
            autocompact_params: parking_lot::Mutex::new(Vec::new()),
            collapse_drain_result: parking_lot::Mutex::new(None),
            collapse_drain_calls: AtomicUsize::new(0),
            reactive_compact_result: parking_lot::Mutex::new(None),
            reactive_compact_calls: AtomicUsize::new(0),
            aborted: AtomicBool::new(false),
            stream_finished: Arc::new(AtomicBool::new(false)),
            tool_executed_before_stream_finished: AtomicBool::new(false),
            tool_execution_count: AtomicUsize::new(0),
            tool_completed_count: AtomicUsize::new(0),
            hook_stopped_tool_execution: AtomicBool::new(false),
            active_tools: AtomicUsize::new(0),
            max_active_tools: AtomicUsize::new(0),
            tool_delay: Duration::ZERO,
            tools: vec![],
            refreshed_tools: parking_lot::Mutex::new(None),
            refresh_seen: AtomicBool::new(false),
            hook_runner: parking_lot::Mutex::new(Arc::new(
                allthecodes_types::hooks::NoopHookRunner,
            )),
            steer_drains: parking_lot::Mutex::new(VecDeque::new()),
            app_state: parking_lot::Mutex::new(AppState::default()),
        }
    }

    fn with_tools(mut self, tools: Tools) -> Self {
        self.tools = tools;
        self
    }

    fn with_refreshed_tools(self, tools: Tools) -> Self {
        *self.refreshed_tools.lock() = Some(tools);
        self
    }

    fn with_tool_delay(mut self, delay: Duration) -> Self {
        self.tool_delay = delay;
        self
    }

    fn with_app_state(self, app_state: AppState) -> Self {
        *self.app_state.lock() = app_state;
        self
    }

    fn recorded_params(&self) -> Vec<ModelCallParams> {
        self.call_params.lock().clone()
    }

    fn recorded_autocompact_params(&self) -> Vec<ModelCallParams> {
        self.autocompact_params.lock().clone()
    }

    fn set_reactive_compact_result(&self, result: Option<CompactionResult>) {
        *self.reactive_compact_result.lock() = result;
    }

    fn set_collapse_drain_result(&self, result: Option<CompactionResult>) {
        *self.collapse_drain_result.lock() = result;
    }

    fn set_hook_runner(&self, runner: Arc<dyn HookRunner>) {
        *self.hook_runner.lock() = runner;
    }

    fn set_steer_drains(&self, drains: Vec<Vec<String>>) {
        *self.steer_drains.lock() = drains.into();
    }

    fn stop_after_tool_execution(&self) {
        self.hook_stopped_tool_execution
            .store(true, Ordering::SeqCst);
    }

    fn pop_stream_step(&self) -> Result<MockStreamStep> {
        let mut steps = self.stream_steps.lock();
        if steps.is_empty() {
            anyhow::bail!("no more mock responses");
        }
        Ok(steps.remove(0))
    }
}

#[async_trait::async_trait]
impl QueryDeps for MockDeps {
    async fn call_model(&self, params: ModelCallParams) -> Result<ModelResponse> {
        self.call_params.lock().push(params);
        match self.pop_stream_step()? {
            MockStreamStep::Response(resp) => Ok(resp),
            MockStreamStep::Error(error) => anyhow::bail!("{}", error),
            MockStreamStep::Events(_) | MockStreamStep::DelayedEvents(_) => {
                anyhow::bail!("raw stream events are not supported by call_model")
            }
        }
    }

    async fn call_model_streaming(
        &self,
        params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.call_params.lock().push(params);
        self.stream_finished.store(false, Ordering::SeqCst);

        let events = match self.pop_stream_step()? {
            MockStreamStep::Response(resp) => {
                let mut events = Vec::new();
                events.push(Ok(StreamEvent::MessageStart {
                    usage: resp.usage.clone(),
                }));
                for (i, block) in resp.assistant_message.content.iter().enumerate() {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: block.clone(),
                    }));
                    events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
                }
                events.push(Ok(StreamEvent::MessageDelta {
                    delta: crate::types::message::MessageDelta {
                        stop_reason: resp.assistant_message.stop_reason.clone(),
                    },
                    usage: Some(resp.usage),
                }));
                events.push(Ok(StreamEvent::MessageStop));
                events
                    .into_iter()
                    .map(|event| (Duration::from_millis(0), event))
                    .collect()
            }
            MockStreamStep::Error(error) => anyhow::bail!("{}", error),
            MockStreamStep::Events(events) => events
                .into_iter()
                .map(|event| (Duration::from_millis(0), event))
                .collect(),
            MockStreamStep::DelayedEvents(events) => events,
        };

        let stream_finished = self.stream_finished.clone();
        let stream = futures::stream::iter(events).then(move |(delay, event)| {
            let stream_finished = stream_finished.clone();
            async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                let result = match event {
                    Ok(event) => Ok(event),
                    Err(error) => anyhow::bail!("{}", error),
                };
                if matches!(result, Ok(StreamEvent::MessageStop)) {
                    stream_finished.store(true, Ordering::SeqCst);
                }
                result
            }
        });
        Ok(Box::pin(stream))
    }

    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        Ok(messages)
    }

    async fn autocompact(
        &self,
        params: ModelCallParams,
        _tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        self.autocompact_params.lock().push(params);
        Ok(None)
    }

    async fn reactive_compact(&self, _messages: Vec<Message>) -> Result<Option<CompactionResult>> {
        self.reactive_compact_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.reactive_compact_result.lock().take())
    }

    async fn collapse_drain(
        &self,
        _messages: Vec<Message>,
        _tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        self.collapse_drain_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.collapse_drain_result.lock().take())
    }

    async fn execute_tool(
        &self,
        request: ToolExecRequest,
        _tools: &Tools,
        _parent: &AssistantMessage,
        _on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult> {
        self.tool_execution_count.fetch_add(1, Ordering::SeqCst);
        let active = self.active_tools.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active_tools.fetch_max(active, Ordering::SeqCst);
        if !self.stream_finished.load(Ordering::SeqCst) {
            self.tool_executed_before_stream_finished
                .store(true, Ordering::SeqCst);
        }
        if !self.tool_delay.is_zero() {
            tokio::time::sleep(self.tool_delay).await;
        }
        self.active_tools.fetch_sub(1, Ordering::SeqCst);
        self.tool_completed_count.fetch_add(1, Ordering::SeqCst);

        Ok(ToolExecResult {
            tool_use_id: request.tool_use_id,
            tool_name: request.tool_name,
            result: crate::types::tool::ToolResult {
                data: serde_json::json!("mock tool output"),
                new_messages: vec![],
                ..Default::default()
            },
            is_error: false,
            hook_stopped_continuation: self.hook_stopped_tool_execution.load(Ordering::SeqCst),
        })
    }

    fn get_app_state(&self) -> AppState {
        self.app_state.lock().clone()
    }

    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    fn get_tools(&self) -> Tools {
        if self.refresh_seen.load(Ordering::SeqCst) {
            if let Some(tools) = self.refreshed_tools.lock().as_ref() {
                return tools.clone();
            }
        }
        self.tools.clone()
    }

    async fn refresh_tools(&self) -> Result<Tools> {
        self.refresh_seen.store(true, Ordering::SeqCst);
        Ok(self
            .refreshed_tools
            .lock()
            .clone()
            .unwrap_or_else(|| self.tools.clone()))
    }

    fn hook_runner(&self) -> Arc<dyn HookRunner> {
        self.hook_runner.lock().clone()
    }

    fn drain_steer_messages(&self) -> Vec<String> {
        self.steer_drains.lock().pop_front().unwrap_or_default()
    }
}

fn make_user_message_for_test(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn make_query_params(messages: Vec<Message>) -> QueryParams {
    QueryParams {
        messages,
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    }
}

struct LoopTestTool {
    name: &'static str,
    concurrency_safe: bool,
}

struct ObservableInputTool {
    name: &'static str,
    concurrency_safe: bool,
}

#[async_trait::async_trait]
impl Tool for LoopTestTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self, _input: &Value) -> String {
        String::new()
    }

    fn input_json_schema(&self) -> Value {
        serde_json::json!({})
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        self.concurrency_safe
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        Ok(ToolResult::default())
    }

    async fn prompt(&self) -> String {
        String::new()
    }
}

#[async_trait::async_trait]
impl Tool for ObservableInputTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self, _input: &Value) -> String {
        String::new()
    }

    fn input_json_schema(&self) -> Value {
        serde_json::json!({})
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        self.concurrency_safe
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        if input.contains_key("type") {
            return;
        }
        let Some(to) = input.get("to").and_then(Value::as_str).map(str::to_string) else {
            return;
        };
        let Some(message) = input
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return;
        };
        input.insert("type".to_string(), serde_json::json!("message"));
        input.insert("recipient".to_string(), serde_json::json!(to));
        input.insert("content".to_string(), serde_json::json!(message));
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        Ok(ToolResult::default())
    }

    async fn prompt(&self) -> String {
        String::new()
    }
}

fn make_auto_compact_tracking() -> AutoCompactTracking {
    AutoCompactTracking {
        compacted: true,
        turn_counter: 1,
        turn_id: "test-turn".to_string(),
        consecutive_failures: 0,
    }
}

fn make_text_response(text: &str) -> ModelResponse {
    make_text_response_with_stop_and_output_tokens(text, "end_turn", 50)
}

fn make_text_response_with_stop_and_output_tokens(
    text: &str,
    stop_reason: &str,
    output_tokens: u64,
) -> ModelResponse {
    ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
            stop_reason: Some(stop_reason.to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage {
            input_tokens: 100,
            output_tokens,
            reasoning_output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
    }
}

fn request_start_count(items: &[QueryYield]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, QueryYield::RequestStart(_)))
        .count()
}

fn has_api_error_containing(items: &[QueryYield], needle: &str) -> bool {
    items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.is_api_error_message
                && msg
                    .api_error
                    .as_deref()
                    .is_some_and(|error| error.contains(needle))
        } else {
            false
        }
    })
}

#[tokio::test]
async fn query_refreshes_tools_before_first_model_call() {
    let refreshed_tool: Arc<dyn Tool> = Arc::new(LoopTestTool {
        name: "mcp__late__fresh",
        concurrency_safe: true,
    });
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")]).with_refreshed_tools(vec![refreshed_tool]),
    );

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("use the late tool")]),
        deps.clone(),
    )
    .collect()
    .await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].tools.len(), 1);
    assert_eq!(recorded[0].tools[0].name(), "mcp__late__fresh");
}

#[tokio::test]
async fn query_shapes_autocompact_with_final_request_context() {
    let refreshed_tool: Arc<dyn Tool> = Arc::new(LoopTestTool {
        name: "mcp__late__fresh",
        concurrency_safe: true,
    });
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")]).with_refreshed_tools(vec![refreshed_tool]),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("count the full request")]);
    params.system_prompt = vec!["system boundary".to_string()];
    params.max_output_tokens_override = Some(1234);
    params.skip_cache_write = Some(true);

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_autocompact_params();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].system_prompt,
        vec!["system boundary".to_string()]
    );
    assert_eq!(recorded[0].tools.len(), 1);
    assert_eq!(recorded[0].tools[0].name(), "mcp__late__fresh");
    assert_eq!(recorded[0].max_output_tokens, Some(1234));
    assert_eq!(recorded[0].skip_cache_write, Some(true));
}

#[tokio::test]
#[serial_test::serial]
async fn deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery() {
    allthecodes_tools::deferred_tools::clear_discovered_tools_for_tests();
    allthecodes_tools::deferred_tools::mark_discovered_tools("unknown", ["WebBrowser".to_string()]);
    let mut tools = allthecodes_tools::deferred_tools::tools();
    tools.push(Arc::new(allthecodes_tools::exec::SleepTool));
    tools.push(Arc::new(LoopTestTool {
        name: "WebBrowser",
        concurrency_safe: true,
    }));
    let deps = Arc::new(MockDeps::new(vec![make_text_response("done")]).with_tools(tools));
    let mut params = make_query_params(vec![make_user_message_for_test(
        "use the discovered browser",
    )]);
    params.gates.deferred_tool_loading = true;

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("SearchExtraTools"));
    assert!(names.contains("ExecuteExtraTool"));
    assert!(names.contains("Sleep"));
    assert!(!names.contains("WebBrowser"));
    assert!(
        allthecodes_tools::deferred_tools::discovered_tools_for_session("unknown")
            .contains("WebBrowser")
    );
}

#[tokio::test]
async fn text_only_model_filters_view_image_from_request_tools() {
    let mut app_state = AppState::default();
    app_state.main_loop_model = "text-only".into();
    app_state.settings.model_capabilities.insert(
        "text-only".into(),
        allthecodes_config::settings::ModelCapabilitySettings {
            input_modalities: vec!["text".into()],
            supports_image_detail_original: false,
            ..Default::default()
        },
    );
    let tools: Tools = vec![
        Arc::new(allthecodes_tools::exec::SleepTool),
        Arc::new(LoopTestTool {
            name: "ViewImage",
            concurrency_safe: true,
        }),
        Arc::new(LoopTestTool {
            name: "view_image",
            concurrency_safe: true,
        }),
    ];
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("done")])
            .with_tools(tools)
            .with_app_state(app_state),
    );

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("inspect image")]),
        deps.clone(),
    )
    .collect()
    .await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains("Sleep"));
    assert!(!names.contains("ViewImage"));
    assert!(!names.contains("view_image"));

    let compact = deps.recorded_autocompact_params();
    assert_eq!(compact.len(), 1);
    let compact_names = compact[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!compact_names.contains("ViewImage"));
    assert!(!compact_names.contains("view_image"));
}

#[tokio::test]
async fn agent_session_filters_prompt_and_recursive_tools_from_request_tools() {
    let tools: Tools = [
        "AskUserQuestion",
        "Agent",
        "Task",
        "TeamSpawn",
        "spawn_agent",
        "FollowupTask",
        "followup_task",
        "Read",
        "SendMessage",
    ]
    .into_iter()
    .map(|name| {
        Arc::new(LoopTestTool {
            name,
            concurrency_safe: true,
        }) as Arc<dyn Tool>
    })
    .collect();
    let deps = Arc::new(MockDeps::new(vec![make_text_response("done")]).with_tools(tools));
    let mut params = make_query_params(vec![make_user_message_for_test("continue agent task")]);
    params.query_source = QuerySource::Agent("child-agent".to_string());

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    assert_eq!(request_start_count(&items), 1);

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 1);
    let names = recorded[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for hidden in [
        "AskUserQuestion",
        "Agent",
        "Task",
        "TeamSpawn",
        "spawn_agent",
        "FollowupTask",
        "followup_task",
    ] {
        assert!(!names.contains(hidden), "{hidden} should be session-gated");
    }
    assert!(names.contains("Read"));
    assert!(names.contains("SendMessage"));

    let compact = deps.recorded_autocompact_params();
    assert_eq!(compact.len(), 1);
    let compact_names = compact[0]
        .tools
        .iter()
        .map(|tool| tool.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!compact_names.contains("AskUserQuestion"));
    assert!(!compact_names.contains("Agent"));
    assert!(!compact_names.contains("Task"));
}

#[tokio::test]
async fn query_drains_steer_before_next_model_request() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response("first answer"),
        make_text_response("steered answer"),
    ]));
    deps.set_steer_drains(vec![vec![], vec!["steer now".to_string()]]);

    let items: Vec<QueryYield> = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    )
    .collect()
    .await;

    assert_eq!(request_start_count(&items), 2);
    assert!(items.iter().any(|item| matches!(
        item,
        QueryYield::Message(Message::User(user))
            if matches!(&user.content, MessageContent::Text(text) if text == "steer now")
    )));

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1].messages.iter().any(|message| matches!(
        message,
        Message::User(user)
            if matches!(&user.content, MessageContent::Text(text) if text == "steer now")
    )));
}

struct StopContinuationHookRunner {
    message: String,
    calls: AtomicUsize,
}

impl StopContinuationHookRunner {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl HookRunner for StopContinuationHookRunner {
    fn load_hook_configs(&self, _hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig> {
        if event_name == "Stop" {
            vec![HookEventConfig {
                matcher: None,
                critical: false,
                hooks: Vec::new(),
            }]
        } else {
            Vec::new()
        }
    }

    async fn run_pre_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult> {
        Ok(PreToolHookResult::Continue {
            updated_input: None,
            permission_override: None,
        })
    }

    async fn run_post_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _tool_result_data: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }

    async fn run_post_tool_failure_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _error: &str,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_event_hooks(
        &self,
        _event_name: &str,
        _payload: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput> {
        Ok(HookOutput::default())
    }

    async fn run_stop_hooks(
        &self,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(PostToolHookResult::StopContinuation {
            message: self.message.clone(),
        })
    }
}

#[tokio::test]
async fn test_simple_text_response_terminates() {
    let deps = Arc::new(MockDeps::new(vec![make_text_response("Hello, world!")]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Hi".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec!["You are a helpful assistant.".to_string()],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        items.len() >= 2,
        "expected at least 2 items, got {}",
        items.len()
    );
    assert!(matches!(items[0], QueryYield::RequestStart(_)));

    let has_assistant = items
        .iter()
        .any(|item| matches!(item, QueryYield::Message(Message::Assistant(_))));
    assert!(has_assistant, "expected an assistant message in output");
}

#[tokio::test]
async fn test_prompt_too_long_reactive_compact_retries_model_call() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("prompt_too_long: context window exceeded".to_string()),
        MockStreamStep::Response(make_text_response("Recovered after compact.")),
    ]));
    deps.set_reactive_compact_result(Some(CompactionResult {
        messages: initial_messages.clone(),
        tracking: make_auto_compact_tracking(),
    }));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "prompt_too_long recovery should retry the model call"
    );
    assert_eq!(
        deps.collapse_drain_calls.load(Ordering::SeqCst),
        1,
        "prompt_too_long should drain collapses before reactive compact"
    );
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        1,
        "prompt_too_long should attempt reactive compact once"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[1].max_output_tokens, None,
        "context recovery must not trigger max-output-token escalation"
    );

    let recovered = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.content.iter().any(|block| {
                matches!(block, ContentBlock::Text { text } if text == "Recovered after compact.")
            })
        } else {
            false
        }
    });
    assert!(recovered, "expected recovered assistant response");
}

#[tokio::test]
async fn test_prompt_too_long_collapse_drain_retries_before_reactive_compact() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("prompt_too_long: context window exceeded".to_string()),
        MockStreamStep::Response(make_text_response("Recovered after collapse drain.")),
    ]));
    deps.set_collapse_drain_result(Some(CompactionResult {
        messages: initial_messages.clone(),
        tracking: make_auto_compact_tracking(),
    }));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "collapse drain recovery should retry the model call"
    );
    assert_eq!(deps.collapse_drain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        0,
        "reactive compact should not run when collapse drain succeeds"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2);
    assert_eq!(
        params[1].max_output_tokens, None,
        "collapse-drain retry must not trigger max-output-token escalation"
    );

    let recovered = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.content.iter().any(|block| {
                matches!(block, ContentBlock::Text { text } if text == "Recovered after collapse drain.")
            })
        } else {
            false
        }
    });
    assert!(recovered, "expected recovered assistant response");
}

#[tokio::test]
async fn test_prompt_too_long_terminals_after_collapse_and_reactive_fail() {
    let initial_messages = vec![make_user_message_for_test("Summarize this long context")];
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Error(
        "prompt_too_long: context window exceeded".to_string(),
    )]));

    let stream = query(make_query_params(initial_messages), deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert_eq!(deps.collapse_drain_calls.load(Ordering::SeqCst), 1);
    assert_eq!(deps.reactive_compact_calls.load(Ordering::SeqCst), 1);
    assert!(items.iter().any(|item| {
        matches!(
            item,
            QueryYield::Message(Message::Assistant(message))
                if message.is_api_error_message
        )
    }));
}

#[tokio::test]
async fn test_fallback_model_retries_stream_start_capacity_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded: high demand".to_string()),
        MockStreamStep::Response(make_text_response("Recovered on fallback")),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use the fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "fallback should emit a second model request start"
    );

    let recorded = deps.recorded_params();
    let primary_model = AppState::default().main_loop_model;
    assert_eq!(
        recorded.len(),
        2,
        "primary failure should be retried once with fallback"
    );
    assert_eq!(recorded[0].model.as_deref(), Some(primary_model.as_str()));
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        !has_api_error_containing(&items, "529 overloaded"),
        "recoverable stream-start error should be withheld when fallback succeeds"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Recovered on fallback")
                ))
        )),
        "fallback response should be yielded as the assistant message"
    );
}

#[tokio::test]
async fn test_fallback_strips_signature_blocks_from_retry_messages() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded: high demand".to_string()),
        MockStreamStep::Response(make_text_response("Recovered without signed thinking")),
    ]));

    let signed_assistant = Message::Assistant(AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "assistant".to_string(),
        content: vec![
            ContentBlock::Text {
                text: "keep visible context".to_string(),
            },
            ContentBlock::Thinking {
                thinking: "private chain".to_string(),
                signature: Some("primary-model-signature".to_string()),
            },
            ContentBlock::RedactedThinking {
                data: "redacted-signature-payload".to_string(),
            },
            ContentBlock::ToolUse {
                id: "toolu_keep".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "Cargo.toml"}),
            },
        ],
        usage: Some(Usage::default()),
        stop_reason: Some("tool_use".to_string()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    });
    let mut params = make_query_params(vec![
        make_user_message_for_test("Use fallback with prior thinking"),
        signed_assistant,
    ]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let _items: Vec<QueryYield> = stream.collect().await;

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);

    let primary_assistant = recorded[0]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant(assistant) => Some(assistant),
            _ => None,
        })
        .expect("primary request should include assistant context");
    assert!(
        primary_assistant
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Thinking { .. })),
        "primary request keeps original signed thinking"
    );

    let fallback_assistant = recorded[1]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant(assistant) => Some(assistant),
            _ => None,
        })
        .expect("fallback request should include assistant context");
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        fallback_assistant.content.iter().all(|block| !matches!(
            block,
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
        )),
        "fallback request must not replay old model thinking signatures"
    );
    assert!(
        fallback_assistant.content.iter().any(|block| matches!(
            block,
            ContentBlock::Text { text } if text == "keep visible context"
        )),
        "fallback request should keep visible assistant context"
    );
    assert!(
        fallback_assistant.content.iter().any(|block| matches!(
            block,
            ContentBlock::ToolUse { id, .. } if id == "toolu_keep"
        )),
        "fallback request should keep tool_use context"
    );
}

#[tokio::test]
async fn test_fallback_tombstones_partial_assistant_after_stream_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Events(vec![
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({
                    "type": "text_delta",
                    "text": "orphaned partial"
                }),
            }),
            Err("529 overloaded during stream".to_string()),
        ]),
        MockStreamStep::Response(make_text_response("Recovered after tombstone")),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "partial stream failure should retry with fallback"
    );
    let tombstone = items.iter().find_map(|item| {
        if let QueryYield::Tombstone(tombstone) = item {
            Some(tombstone)
        } else {
            None
        }
    });
    let tombstone = tombstone.expect("expected tombstone for orphaned partial assistant");
    assert!(
        tombstone.message.content.iter().any(|block| matches!(
            block,
            ContentBlock::Text { text } if text == "orphaned partial"
        )),
        "tombstone should carry the orphaned partial assistant content"
    );

    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
    assert!(
        !has_api_error_containing(&items, "529 overloaded during stream"),
        "recoverable mid-stream error should be withheld when fallback succeeds"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Recovered after tombstone")
                ))
        )),
        "fallback response should be yielded after tombstone"
    );
}

#[tokio::test]
async fn test_chunk_read_error_after_text_accepts_partial_assistant() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::Events(vec![
        Ok(StreamEvent::MessageStart {
            usage: Usage::default(),
        }),
        Ok(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        }),
        Ok(StreamEvent::ContentBlockDelta {
            index: 0,
            delta: serde_json::json!({
                "type": "text_delta",
                "text": "partial but usable"
            }),
        }),
        Err("error reading response chunk: connection closed".to_string()),
    ])]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Use compatible gateway")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        !has_api_error_containing(&items, "error reading response chunk"),
        "chunk read errors after text should not replace the response with an API error"
    );
    assert!(
        items.iter().any(|item| matches!(
            item,
            QueryYield::Message(Message::Assistant(msg))
                if msg.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text == "partial but usable"
                ))
        )),
        "partial text should be finalized as the assistant response"
    );
}

#[tokio::test]
async fn test_fallback_exhaustion_releases_terminal_stream_start_error() {
    let deps = Arc::new(MockDeps::from_steps(vec![
        MockStreamStep::Error("529 overloaded primary".to_string()),
        MockStreamStep::Error("529 overloaded fallback".to_string()),
    ]));

    let mut params = make_query_params(vec![make_user_message_for_test("Use fallback")]);
    params.fallback_model = Some("claude-fallback".to_string());

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "primary should be retried once on the fallback model"
    );
    assert!(
        !has_api_error_containing(&items, "529 overloaded primary"),
        "recoverable primary failure should remain withheld"
    );
    assert!(
        has_api_error_containing(&items, "529 overloaded fallback"),
        "fallback exhaustion should release the final visible error"
    );
    let recorded = deps.recorded_params();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].model.as_deref(), Some("claude-fallback"));
}

#[tokio::test]
async fn test_stream_idle_watchdog_errors_when_first_event_never_arrives() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::DelayedEvents(
        vec![(
            Duration::from_millis(75),
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        )],
    )]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Wait for stream")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(request_start_count(&items), 1);
    assert!(
        has_api_error_containing(&items, "stream idle timeout"),
        "idle watchdog should surface a stream timeout error: {:?}",
        items
    );
}

#[tokio::test]
async fn test_stream_stall_detection_errors_after_handshake_without_progress() {
    let deps = Arc::new(MockDeps::from_steps(vec![MockStreamStep::DelayedEvents(
        vec![
            (
                Duration::from_millis(0),
                Ok(StreamEvent::MessageStart {
                    usage: Usage::default(),
                }),
            ),
            (
                Duration::from_millis(40),
                Ok(StreamEvent::ContentBlockStart {
                    index: 0,
                    content_block: ContentBlock::Text {
                        text: String::new(),
                    },
                }),
            ),
        ],
    )]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Detect stall")]),
        deps,
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert!(
        items
            .iter()
            .any(|item| matches!(item, QueryYield::Stream(StreamEvent::MessageStart { .. }))),
        "stream handshake should be forwarded before the stall is detected"
    );
    assert!(
        has_api_error_containing(&items, "stream stalled"),
        "passive stall detection should surface a stream stalled error: {:?}",
        items
    );
}

#[tokio::test]
async fn test_max_tokens_recovery_escalates_next_request_limit() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response_with_stop_and_output_tokens("Partial answer", "max_tokens", 50),
        make_text_response("Continuation after larger output limit."),
    ]));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Write a long answer")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "max_tokens should trigger one retry"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected two model calls");
    assert_eq!(params[0].max_output_tokens, None);
    assert_eq!(
        params[1].max_output_tokens,
        Some(super::super::loop_helpers::ESCALATED_MAX_TOKENS)
    );
    assert_eq!(
        deps.collapse_drain_calls.load(Ordering::SeqCst),
        0,
        "max_tokens recovery must not invoke context collapse drain"
    );
    assert_eq!(
        deps.reactive_compact_calls.load(Ordering::SeqCst),
        0,
        "max_tokens recovery must not invoke reactive compact"
    );
}

#[tokio::test]
async fn test_stop_hook_continuation_injects_meta_user_message_once() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response("Need final audit."),
        make_text_response("Final answer after stop hook."),
    ]));
    deps.set_hook_runner(Arc::new(StopContinuationHookRunner::new(
        "Run one more validation pass.",
    )));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Finish the task")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "stop hook continuation should trigger one more model call"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected continuation model call");

    let continuation = params[1].messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some((user.is_meta, text.as_str()));
            }
        }
        None
    });
    assert_eq!(
        continuation,
        Some((true, "Run one more validation pass.")),
        "stop hook continuation should be injected as a meta user message"
    );
}

#[tokio::test]
async fn test_token_budget_continuation_injects_nudge_message() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response_with_stop_and_output_tokens("Still working.", "end_turn", 50),
        make_text_response_with_stop_and_output_tokens("Budget complete.", "end_turn", 50),
    ]));
    let mut params = make_query_params(vec![make_user_message_for_test("Spend the budget")]);
    params.task_budget = Some(TaskBudget { total: 100 });

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "token budget nudge should trigger one continuation"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected continuation model call");

    let nudge = params[1].messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some((user.is_meta, text.as_str()));
            }
        }
        None
    });
    assert!(
        matches!(nudge, Some((true, text)) if text.contains("Token budget at 50%")),
        "token budget continuation should inject a meta nudge message"
    );
}

#[tokio::test]
async fn test_tool_use_then_text_response() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "echo hello"}),
                },
            ],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 80,
                ..Default::default()
            }),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let text_response = make_text_response("Done! The output was hello.");

    let deps = Arc::new(MockDeps::new(vec![tool_response, text_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Run echo hello".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 2, "expected 2 request starts (two turns)");

    let assistant_msgs = items
        .iter()
        .filter(|i| matches!(i, QueryYield::Message(Message::Assistant(_))))
        .count();
    assert_eq!(assistant_msgs, 2, "expected 2 assistant messages");

    assert!(
        !deps
            .tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "tool execution should not start before the model stream reaches message_stop"
    );
}

async fn tool_use_summary_gate_case(emit_tool_use_summaries: bool) -> Vec<QueryYield> {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_summary".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "echo hello"}),
            }],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 80,
                ..Default::default()
            }),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };
    let deps = Arc::new(MockDeps::new(vec![
        tool_response,
        make_text_response("Done!"),
    ]));
    let mut params = make_query_params(vec![make_user_message_for_test("Run echo hello")]);
    params.gates.emit_tool_use_summaries = emit_tool_use_summaries;

    query(params, deps).collect().await
}

#[tokio::test]
async fn tool_use_summary_gate_defaults_off() {
    let items = tool_use_summary_gate_case(false).await;

    assert!(
        !items
            .iter()
            .any(|item| matches!(item, QueryYield::ToolUseSummary(_))),
        "default gates should not emit tool use summaries"
    );
}

#[tokio::test]
async fn tool_use_summary_gate_yields_summary_when_enabled() {
    let items = tool_use_summary_gate_case(true).await;
    let summary = items
        .iter()
        .find_map(|item| {
            if let QueryYield::ToolUseSummary(summary) = item {
                Some(summary)
            } else {
                None
            }
        })
        .expect("tool use summary should be emitted");

    assert!(summary.summary.contains("Bash"));
    assert!(summary.summary.contains("mock tool output"));
    assert_eq!(summary.preceding_tool_use_ids, vec!["tu_summary"]);
}

async fn run_observable_input_backfill_case(
    streaming_tool_execution: bool,
) -> (Vec<QueryYield>, Vec<ModelCallParams>) {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_observable".to_string(),
                name: "ObservableMessage".to_string(),
                input: serde_json::json!({"to": "worker", "message": "hello"}),
            }],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };
    let tools: Tools = vec![Arc::new(ObservableInputTool {
        name: "ObservableMessage",
        concurrency_safe: true,
    })];
    let deps = Arc::new(
        MockDeps::new(vec![
            tool_response,
            make_text_response("observable input complete"),
        ])
        .with_tools(tools),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("send message")]);
    params.gates.streaming_tool_execution = streaming_tool_execution;

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;
    let recorded_params = deps.recorded_params();
    (items, recorded_params)
}

fn yielded_tool_input(items: &[QueryYield], tool_name: &str) -> Value {
    items
        .iter()
        .find_map(|item| {
            let QueryYield::Message(Message::Assistant(assistant)) = item else {
                return None;
            };
            assistant.content.iter().find_map(|block| match block {
                ContentBlock::ToolUse { name, input, .. } if name == tool_name => {
                    Some(input.clone())
                }
                _ => None,
            })
        })
        .expect("yielded assistant tool input")
}

fn request_tool_input(params: &[ModelCallParams], request_index: usize, tool_name: &str) -> Value {
    params[request_index]
        .messages
        .iter()
        .find_map(|message| {
            let Message::Assistant(assistant) = message else {
                return None;
            };
            assistant.content.iter().find_map(|block| match block {
                ContentBlock::ToolUse { name, input, .. } if name == tool_name => {
                    Some(input.clone())
                }
                _ => None,
            })
        })
        .expect("request assistant tool input")
}

#[tokio::test]
async fn observable_input_backfill_clones_yield_without_changing_next_request_gate_off() {
    let (items, params) = run_observable_input_backfill_case(false).await;

    let yielded_input = yielded_tool_input(&items, "ObservableMessage");
    assert_eq!(
        yielded_input.get("type"),
        Some(&serde_json::json!("message"))
    );
    assert_eq!(
        yielded_input.get("recipient"),
        Some(&serde_json::json!("worker"))
    );
    assert_eq!(
        yielded_input.get("content"),
        Some(&serde_json::json!("hello"))
    );

    let request_input = request_tool_input(&params, 1, "ObservableMessage");
    assert!(request_input.get("type").is_none());
    assert!(request_input.get("recipient").is_none());
    assert!(request_input.get("content").is_none());
}

#[tokio::test]
async fn observable_input_backfill_matches_with_streaming_tool_gate_on() {
    let (items, params) = run_observable_input_backfill_case(true).await;

    let yielded_input = yielded_tool_input(&items, "ObservableMessage");
    assert_eq!(
        yielded_input.get("type"),
        Some(&serde_json::json!("message"))
    );
    assert_eq!(
        yielded_input.get("recipient"),
        Some(&serde_json::json!("worker"))
    );
    assert_eq!(
        yielded_input.get("content"),
        Some(&serde_json::json!("hello"))
    );

    let request_input = request_tool_input(&params, 1, "ObservableMessage");
    assert!(request_input.get("type").is_none());
    assert!(request_input.get("recipient").is_none());
    assert!(request_input.get("content").is_none());
}

#[tokio::test]
async fn streaming_tool_execution_gate_starts_safe_tools_before_message_stop() {
    let events = vec![
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::ToolUse {
                    id: "tu_safe_1".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
        ),
        (
            Duration::from_millis(5),
            Ok(StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlock::ToolUse {
                    id: "tu_safe_2".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 1 }),
        ),
        (
            Duration::from_millis(5),
            Ok(StreamEvent::ContentBlockStart {
                index: 2,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockDelta {
                index: 2,
                delta: serde_json::json!({
                    "type": "text_delta",
                    "text": "still streaming"
                }),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 2 }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageDelta {
                delta: crate::types::message::MessageDelta {
                    stop_reason: Some("tool_use".to_string()),
                },
                usage: Some(Usage::default()),
            }),
        ),
        (Duration::ZERO, Ok(StreamEvent::MessageStop)),
    ];
    let tools: Tools = vec![Arc::new(LoopTestTool {
        name: "SafeTool",
        concurrency_safe: true,
    })];
    let deps = Arc::new(
        MockDeps::from_steps(vec![
            MockStreamStep::DelayedEvents(events),
            MockStreamStep::Response(make_text_response("streamed tools complete")),
        ])
        .with_tools(tools)
        .with_tool_delay(Duration::from_millis(20)),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("run safe tools")]);
    params.gates.streaming_tool_execution = true;

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;

    assert!(
        deps.tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "safe tools should start while the assistant stream is still open"
    );
    assert_eq!(
        deps.tool_execution_count.load(Ordering::SeqCst),
        2,
        "started streaming tools must not be re-executed after message_stop"
    );
    assert_eq!(
        deps.tool_completed_count.load(Ordering::SeqCst),
        2,
        "streaming tool tasks should be awaited before continuation"
    );
    assert_eq!(
        deps.max_active_tools.load(Ordering::SeqCst),
        2,
        "consecutive safe tools should run concurrently"
    );
    let tool_result_messages = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                QueryYield::Message(Message::User(user))
                    if user.is_meta && user.source_tool_assistant_uuid.is_some()
            )
        })
        .count();
    assert_eq!(tool_result_messages, 2);
    assert_eq!(request_start_count(&items), 2);
}

#[tokio::test]
async fn streaming_tool_execution_aborts_started_tools_on_stream_fallback() {
    let events = vec![
        (
            Duration::ZERO,
            Ok(StreamEvent::MessageStart {
                usage: Usage::default(),
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::ToolUse {
                    id: "tu_orphan".to_string(),
                    name: "SafeTool".to_string(),
                    input: serde_json::json!({}),
                },
            }),
        ),
        (
            Duration::ZERO,
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
        ),
        (
            Duration::from_millis(5),
            Err("529 overloaded during stream".to_string()),
        ),
    ];
    let tools: Tools = vec![Arc::new(LoopTestTool {
        name: "SafeTool",
        concurrency_safe: true,
    })];
    let deps = Arc::new(
        MockDeps::from_steps(vec![
            MockStreamStep::DelayedEvents(events),
            MockStreamStep::Response(make_text_response("fallback recovered")),
        ])
        .with_tools(tools)
        .with_tool_delay(Duration::from_millis(50)),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("run then fallback")]);
    params.gates.streaming_tool_execution = true;
    params.fallback_model = Some("fallback-model".to_string());

    let items: Vec<QueryYield> = query(params, deps.clone()).collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "stream interruption should retry on fallback model"
    );
    assert!(
        items
            .iter()
            .any(|item| matches!(item, QueryYield::Tombstone(_))),
        "partial primary assistant should be tombstoned before fallback"
    );
    assert!(
        deps.tool_executed_before_stream_finished
            .load(Ordering::SeqCst),
        "the primary attempt should have started the safe tool before failing"
    );
    assert_eq!(
        deps.tool_completed_count.load(Ordering::SeqCst),
        0,
        "started primary-attempt tools should be aborted instead of awaited into fallback"
    );
    let tool_result_messages = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                QueryYield::Message(Message::User(user))
                    if user.is_meta && user.source_tool_assistant_uuid.is_some()
            )
        })
        .count();
    assert_eq!(
        tool_result_messages, 0,
        "orphaned primary-attempt tool results must not enter fallback transcript"
    );
    assert!(!has_api_error_containing(&items, "529 overloaded"));
}

#[tokio::test]
async fn test_max_turns_limit() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let deps = Arc::new(MockDeps::new(vec![tool_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("list files".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: Some(1),
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    let has_max_turns = items.iter().any(|item| {
        matches!(
            item,
            QueryYield::Message(Message::Attachment(AttachmentMessage {
                attachment: Attachment::MaxTurnsReached { .. },
                ..
            }))
        )
    });
    assert!(has_max_turns, "expected MaxTurnsReached attachment");
}

#[tokio::test]
async fn test_hook_stopped_tool_execution_yields_attachment_and_stops() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let deps = Arc::new(MockDeps::new(vec![
        tool_response,
        make_text_response("must not run"),
    ]));
    deps.stop_after_tool_execution();

    let stream = query(
        make_query_params(vec![make_user_message_for_test("run a tool")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        1,
        "hook stopped continuation should not start another model request"
    );
    assert!(
        items.iter().any(|item| {
            matches!(
                item,
                QueryYield::Message(Message::Attachment(AttachmentMessage {
                    attachment: Attachment::HookStoppedContinuation,
                    ..
                }))
            )
        }),
        "expected HookStoppedContinuation attachment"
    );
}

#[tokio::test]
async fn test_abort_before_api_call() {
    let deps = Arc::new(MockDeps::new(vec![]));
    deps.aborted
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Hi".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    let has_assistant = items.iter().any(|item| {
        if let QueryYield::Message(Message::Assistant(msg)) = item {
            msg.stop_reason.as_deref() == Some("AbortedStreaming")
        } else {
            false
        }
    });
    assert!(has_assistant, "expected aborted assistant message");
}

/// MockDeps that returns image content in ToolResult.model_content
struct ImageMockDeps {
    responses: parking_lot::Mutex<Vec<ModelResponse>>,
    aborted: std::sync::atomic::AtomicBool,
}

impl ImageMockDeps {
    fn new(responses: Vec<ModelResponse>) -> Self {
        Self {
            responses: parking_lot::Mutex::new(responses),
            aborted: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait::async_trait]
impl QueryDeps for ImageMockDeps {
    async fn call_model(&self, _params: ModelCallParams) -> Result<ModelResponse> {
        let mut responses = self.responses.lock();
        if responses.is_empty() {
            anyhow::bail!("no more mock responses");
        }
        Ok(responses.remove(0))
    }

    async fn call_model_streaming(
        &self,
        _params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let mut responses = self.responses.lock();
        if responses.is_empty() {
            anyhow::bail!("no more mock responses");
        }
        let resp = responses.remove(0);
        let mut events = Vec::new();
        events.push(StreamEvent::MessageStart {
            usage: resp.usage.clone(),
        });
        for (i, block) in resp.assistant_message.content.iter().enumerate() {
            events.push(StreamEvent::ContentBlockStart {
                index: i,
                content_block: block.clone(),
            });
            events.push(StreamEvent::ContentBlockStop { index: i });
        }
        events.push(StreamEvent::MessageDelta {
            delta: crate::types::message::MessageDelta {
                stop_reason: resp.assistant_message.stop_reason.clone(),
            },
            usage: Some(resp.usage),
        });
        events.push(StreamEvent::MessageStop);
        let stream = futures::stream::iter(events.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }

    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        Ok(messages)
    }

    async fn autocompact(
        &self,
        _params: ModelCallParams,
        _tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        Ok(None)
    }

    async fn reactive_compact(&self, _messages: Vec<Message>) -> Result<Option<CompactionResult>> {
        Ok(None)
    }

    async fn execute_tool(
        &self,
        request: ToolExecRequest,
        _tools: &Tools,
        _parent: &AssistantMessage,
        _on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult> {
        // Return a tool result with image model_content (simulating MCP screenshot)
        let image_blocks = vec![
            ContentBlock::Text {
                text: "Screenshot captured".to_string(),
            },
            ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "iVBORw0KGgoAAAANSUhEUg==".to_string(),
                },
            },
        ];

        Ok(ToolExecResult {
            tool_use_id: request.tool_use_id,
            tool_name: request.tool_name,
            result: crate::types::tool::ToolResult {
                data: serde_json::json!("[Image: image/png]"),
                model_content: Some(ToolResultContent::Blocks(image_blocks)),
                display_preview: Some("[Image: image/png]".to_string()),
                new_messages: vec![],
            },
            is_error: false,
            hook_stopped_continuation: false,
        })
    }

    fn get_app_state(&self) -> AppState {
        AppState::default()
    }

    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn get_tools(&self) -> Tools {
        vec![]
    }

    async fn refresh_tools(&self) -> Result<Tools> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_image_tool_result_flows_as_blocks() {
    // Turn 1: model calls a tool (screenshot)
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me take a screenshot.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_screenshot".to_string(),
                    name: "mcp__computer-use__screenshot".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 80,
                ..Default::default()
            }),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    // Turn 2: model sees image and responds
    let text_response = make_text_response("I can see the desktop.");

    let deps = Arc::new(ImageMockDeps::new(vec![tool_response, text_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Take a screenshot".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    // Find the tool result user message
    let tool_result_msg = items.iter().find_map(|item| {
        if let QueryYield::Message(Message::User(user_msg)) = item {
            if user_msg.is_meta && user_msg.source_tool_assistant_uuid.is_some() {
                return Some(user_msg);
            }
        }
        None
    });

    let user_msg = tool_result_msg.expect("expected a tool result user message");

    // Verify the tool result contains structured Blocks (not plain Text)
    match &user_msg.content {
        MessageContent::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1, "expected 1 tool_result block");
            match &blocks[0] {
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    assert!(!is_error);
                    match content {
                        ToolResultContent::Blocks(inner_blocks) => {
                            assert_eq!(
                                inner_blocks.len(),
                                2,
                                "expected 2 inner blocks (text + image)"
                            );
                            assert!(
                                matches!(&inner_blocks[0], ContentBlock::Text { .. }),
                                "first block should be text"
                            );
                            match &inner_blocks[1] {
                                ContentBlock::Image { source } => {
                                    assert_eq!(source.source_type, "base64");
                                    assert_eq!(source.media_type, "image/png");
                                    assert_eq!(source.data, "iVBORw0KGgoAAAANSUhEUg==");
                                }
                                other => panic!("second block should be Image, got {:?}", other),
                            }
                        }
                        ToolResultContent::Text(t) => {
                            panic!("expected Blocks in tool result, got Text: {}", t);
                        }
                    }
                }
                other => panic!("expected ToolResult block, got {:?}", other),
            }
        }
        _ => panic!("expected Blocks content"),
    }

    // Verify display_preview is used for tool_use_result (not raw base64)
    assert_eq!(
        user_msg.tool_use_result.as_deref(),
        Some("[Image: image/png]"),
        "tool_use_result should contain display preview, not base64 data"
    );

    // Verify there are 2 request starts (two turns = tool call + final response)
    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 2, "expected 2 API turns");
}

// ---------------------------------------------------------------------------
// Computer Use end-to-end smoke test
// ---------------------------------------------------------------------------

/// MockDeps that dispatches tool results by tool name:
/// - screenshot 鈫?image content
/// - left_click / type_text 鈫?text confirmation
struct CuMockDeps {
    responses: parking_lot::Mutex<Vec<ModelResponse>>,
    aborted: std::sync::atomic::AtomicBool,
}

impl CuMockDeps {
    fn new(responses: Vec<ModelResponse>) -> Self {
        Self {
            responses: parking_lot::Mutex::new(responses),
            aborted: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait::async_trait]
impl QueryDeps for CuMockDeps {
    async fn call_model(&self, _params: ModelCallParams) -> Result<ModelResponse> {
        let mut responses = self.responses.lock();
        if responses.is_empty() {
            anyhow::bail!("no more mock responses");
        }
        Ok(responses.remove(0))
    }

    async fn call_model_streaming(
        &self,
        _params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let mut responses = self.responses.lock();
        if responses.is_empty() {
            anyhow::bail!("no more mock responses");
        }
        let resp = responses.remove(0);
        let mut events = Vec::new();
        events.push(StreamEvent::MessageStart {
            usage: resp.usage.clone(),
        });
        for (i, block) in resp.assistant_message.content.iter().enumerate() {
            events.push(StreamEvent::ContentBlockStart {
                index: i,
                content_block: block.clone(),
            });
            events.push(StreamEvent::ContentBlockStop { index: i });
        }
        events.push(StreamEvent::MessageDelta {
            delta: crate::types::message::MessageDelta {
                stop_reason: resp.assistant_message.stop_reason.clone(),
            },
            usage: Some(resp.usage),
        });
        events.push(StreamEvent::MessageStop);
        let stream = futures::stream::iter(events.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }

    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        Ok(messages)
    }

    async fn autocompact(
        &self,
        _params: ModelCallParams,
        _tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>> {
        Ok(None)
    }

    async fn reactive_compact(&self, _messages: Vec<Message>) -> Result<Option<CompactionResult>> {
        Ok(None)
    }

    async fn execute_tool(
        &self,
        request: ToolExecRequest,
        _tools: &Tools,
        _parent: &AssistantMessage,
        _on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult> {
        // Dispatch by tool name to simulate different CU tools
        let result = if request.tool_name.contains("screenshot") {
            crate::types::tool::ToolResult {
                data: serde_json::json!("[Image: image/png]"),
                model_content: Some(ToolResultContent::Blocks(vec![ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_string(),
                        media_type: "image/png".to_string(),
                        data: "iVBORw0KGgoAAAANSUhEUg==".to_string(),
                    },
                }])),
                display_preview: Some("[Screenshot: 1920x1080]".to_string()),
                new_messages: vec![],
            }
        } else {
            // click, type_text, key, scroll 鈫?text confirmation
            crate::types::tool::ToolResult {
                data: serde_json::json!(format!(
                    "Action '{}' executed successfully",
                    request.tool_name
                )),
                new_messages: vec![],
                ..Default::default()
            }
        };

        Ok(ToolExecResult {
            tool_use_id: request.tool_use_id,
            tool_name: request.tool_name,
            result,
            is_error: false,
            hook_stopped_continuation: false,
        })
    }

    fn get_app_state(&self) -> AppState {
        AppState::default()
    }

    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn get_tools(&self) -> Tools {
        vec![]
    }

    async fn refresh_tools(&self) -> Result<Tools> {
        Ok(vec![])
    }
}

/// Full Computer Use smoke test:
///   Turn 1: model calls screenshot 鈫?receives image
///   Turn 2: model calls left_click 鈫?receives text confirmation
///   Turn 3: model responds with final text
#[tokio::test]
async fn test_computer_use_screenshot_click_round_trip() {
    // Turn 1: model takes a screenshot
    let screenshot_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me take a screenshot to see the desktop.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_screenshot".to_string(),
                    name: "mcp__computer-use__screenshot".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    // Turn 2: model sees image, decides to click
    let click_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "I can see a button at (500, 300). Clicking it.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_click".to_string(),
                    name: "mcp__computer-use__left_click".to_string(),
                    input: serde_json::json!({"x": 500, "y": 300}),
                },
            ],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    // Turn 3: model confirms result
    let final_response = make_text_response("I clicked the button successfully.");

    let deps = Arc::new(CuMockDeps::new(vec![
        screenshot_response,
        click_response,
        final_response,
    ]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("Click the button on screen".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: None,
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    // Verify 3 API turns (screenshot, click, final)
    let request_starts = items
        .iter()
        .filter(|i| matches!(i, QueryYield::RequestStart(_)))
        .count();
    assert_eq!(request_starts, 3, "expected 3 API turns");

    // Verify 3 assistant messages
    let assistant_msgs: Vec<_> = items
        .iter()
        .filter_map(|item| {
            if let QueryYield::Message(Message::Assistant(msg)) = item {
                Some(msg)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(assistant_msgs.len(), 3, "expected 3 assistant messages");

    // Verify tool result messages
    let tool_result_msgs: Vec<_> = items
        .iter()
        .filter_map(|item| {
            if let QueryYield::Message(Message::User(msg)) = item {
                if msg.is_meta && msg.source_tool_assistant_uuid.is_some() {
                    return Some(msg);
                }
            }
            None
        })
        .collect();
    assert_eq!(tool_result_msgs.len(), 2, "expected 2 tool result messages");

    // First tool result (screenshot) should have Blocks content with Image
    match &tool_result_msgs[0].content {
        MessageContent::Blocks(blocks) => match &blocks[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert!(
                    matches!(content, ToolResultContent::Blocks(_)),
                    "screenshot result should be Blocks (image), got Text"
                );
            }
            other => panic!("expected ToolResult, got {:?}", other),
        },
        _ => panic!("expected Blocks content"),
    }

    // Second tool result (click) should have Text content
    match &tool_result_msgs[1].content {
        MessageContent::Blocks(blocks) => match &blocks[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert!(
                    matches!(content, ToolResultContent::Text(_)),
                    "click result should be Text"
                );
            }
            other => panic!("expected ToolResult, got {:?}", other),
        },
        _ => panic!("expected Blocks content"),
    }

    // Final message should be text
    let final_msg = assistant_msgs.last().unwrap();
    assert!(
        final_msg
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { text } if text.contains("clicked"))),
        "final message should mention clicking"
    );
}

#[test]
fn snip_projection_removes_requested_messages_only() {
    let keep = Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 1,
        role: "user".to_string(),
        content: MessageContent::Text("keep".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    });
    let remove = Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: 2,
        role: "user".to_string(),
        content: MessageContent::Text("remove".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    });
    let remove_id = remove.uuid().to_string();
    let keep_id = keep.uuid();
    let mut messages = vec![keep, remove];

    let removed = apply_snip_projection(
        &mut messages,
        &serde_json::json!({ "message_ids": [remove_id] }),
    );

    assert_eq!(removed, 1);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].uuid(), keep_id);
}
