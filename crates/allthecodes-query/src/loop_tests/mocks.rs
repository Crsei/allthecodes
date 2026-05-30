use super::*;

pub(super) enum MockStreamStep {
    Response(Box<ModelResponse>),
    Error(String),
    Events(Vec<Result<StreamEvent, String>>),
    DelayedEvents(Vec<(Duration, Result<StreamEvent, String>)>),
}

/// Mock deps for testing.
pub(super) struct MockDeps {
    pub(super) stream_steps: parking_lot::Mutex<Vec<MockStreamStep>>,
    pub(super) call_params: parking_lot::Mutex<Vec<ModelCallParams>>,
    pub(super) autocompact_params: parking_lot::Mutex<Vec<ModelCallParams>>,
    pub(super) autocompact_result: parking_lot::Mutex<Option<CompactionResult>>,
    pub(super) collapse_drain_result: parking_lot::Mutex<Option<CompactionResult>>,
    pub(super) collapse_drain_calls: AtomicUsize,
    pub(super) reactive_compact_result: parking_lot::Mutex<Option<CompactionResult>>,
    pub(super) reactive_compact_calls: AtomicUsize,
    pub(super) aborted: AtomicBool,
    pub(super) stream_finished: Arc<AtomicBool>,
    pub(super) tool_executed_before_stream_finished: AtomicBool,
    pub(super) tool_execution_count: AtomicUsize,
    pub(super) tool_completed_count: AtomicUsize,
    pub(super) hook_stopped_tool_execution: AtomicBool,
    pub(super) active_tools: AtomicUsize,
    pub(super) max_active_tools: AtomicUsize,
    pub(super) tool_delay: Duration,
    pub(super) tools: Tools,
    pub(super) refreshed_tools: parking_lot::Mutex<Option<Tools>>,
    pub(super) refresh_seen: AtomicBool,
    pub(super) hook_runner: parking_lot::Mutex<Arc<dyn HookRunner>>,
    pub(super) audit_session_id: parking_lot::Mutex<String>,
    pub(super) app_state: parking_lot::Mutex<AppState>,
}

impl MockDeps {
    pub(super) fn new(responses: Vec<ModelResponse>) -> Self {
        Self::from_steps(
            responses
                .into_iter()
                .map(|response| MockStreamStep::Response(Box::new(response)))
                .collect(),
        )
    }

    pub(super) fn from_steps(stream_steps: Vec<MockStreamStep>) -> Self {
        Self {
            stream_steps: parking_lot::Mutex::new(stream_steps),
            call_params: parking_lot::Mutex::new(Vec::new()),
            autocompact_params: parking_lot::Mutex::new(Vec::new()),
            autocompact_result: parking_lot::Mutex::new(None),
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
            audit_session_id: parking_lot::Mutex::new("mock-session".to_string()),
            app_state: parking_lot::Mutex::new(AppState::default()),
        }
    }

    pub(super) fn with_tools(mut self, tools: Tools) -> Self {
        self.tools = tools;
        self
    }

    pub(super) fn with_refreshed_tools(self, tools: Tools) -> Self {
        *self.refreshed_tools.lock() = Some(tools);
        self
    }

    pub(super) fn with_autocompact_result(self, result: CompactionResult) -> Self {
        *self.autocompact_result.lock() = Some(result);
        self
    }

    pub(super) fn with_audit_session_id(self, session_id: &str) -> Self {
        *self.audit_session_id.lock() = session_id.to_string();
        self
    }

    pub(super) fn with_app_state(self, app_state: AppState) -> Self {
        *self.app_state.lock() = app_state;
        self
    }

    pub(super) fn with_tool_delay(mut self, delay: Duration) -> Self {
        self.tool_delay = delay;
        self
    }

    pub(super) fn recorded_params(&self) -> Vec<ModelCallParams> {
        self.call_params.lock().clone()
    }

    pub(super) fn recorded_autocompact_params(&self) -> Vec<ModelCallParams> {
        self.autocompact_params.lock().clone()
    }

    pub(super) fn set_reactive_compact_result(&self, result: Option<CompactionResult>) {
        *self.reactive_compact_result.lock() = result;
    }

    pub(super) fn set_collapse_drain_result(&self, result: Option<CompactionResult>) {
        *self.collapse_drain_result.lock() = result;
    }

    pub(super) fn set_hook_runner(&self, runner: Arc<dyn HookRunner>) {
        *self.hook_runner.lock() = runner;
    }

    pub(super) fn stop_after_tool_execution(&self) {
        self.hook_stopped_tool_execution
            .store(true, Ordering::SeqCst);
    }

    pub(super) fn pop_stream_step(&self) -> Result<MockStreamStep> {
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
            MockStreamStep::Response(resp) => Ok(*resp),
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
                    usage: resp.assistant_message.usage.clone().unwrap_or_default(),
                }));
                for (i, block) in resp.assistant_message.content.iter().enumerate() {
                    events.push(Ok(StreamEvent::ContentBlockStart {
                        index: i,
                        content_block: block.clone(),
                    }));
                    events.push(Ok(StreamEvent::ContentBlockStop { index: i }));
                }
                events.push(Ok(StreamEvent::MessageDelta {
                    delta: allthecodes_engine::types::message::MessageDelta {
                        stop_reason: resp.assistant_message.stop_reason.clone(),
                    },
                    usage: resp.assistant_message.usage.clone(),
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
        Ok(self.autocompact_result.lock().take())
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
            result: allthecodes_engine::types::tool::ToolResult {
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

    fn audit_context(&self) -> allthecodes_observability::AuditContext {
        allthecodes_observability::AuditContext::noop(self.audit_session_id.lock().clone())
    }
}

pub(super) fn make_user_message_for_test(text: &str) -> Message {
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

pub(super) fn make_query_params(messages: Vec<Message>) -> QueryParams {
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

pub(super) struct LoopTestTool {
    pub(super) name: &'static str,
    pub(super) concurrency_safe: bool,
}

pub(super) struct ObservableInputTool {
    pub(super) name: &'static str,
    pub(super) concurrency_safe: bool,
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

pub(super) fn make_auto_compact_tracking() -> AutoCompactTracking {
    AutoCompactTracking {
        compacted: true,
        turn_counter: 1,
        turn_id: "test-turn".to_string(),
        consecutive_failures: 0,
    }
}

pub(super) fn make_text_response(text: &str) -> ModelResponse {
    make_text_response_with_stop_and_output_tokens(text, "end_turn", 50)
}

pub(super) fn make_text_response_with_stop_and_output_tokens(
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
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                reasoning_output_tokens: 0,
            }),
            stop_reason: Some(stop_reason.to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        },
    }
}

pub(super) fn request_start_count(items: &[QueryYield]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, QueryYield::RequestStart(_)))
        .count()
}

pub(super) fn has_api_error_containing(items: &[QueryYield], needle: &str) -> bool {
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

pub(super) struct StopContinuationHookRunner {
    pub(super) message: String,
    pub(super) calls: AtomicUsize,
}

impl StopContinuationHookRunner {
    pub(super) fn new(message: &str) -> Self {
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

pub(super) async fn tool_use_summary_gate_case(emit_tool_use_summaries: bool) -> Vec<QueryYield> {
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
    };
    let deps = Arc::new(MockDeps::new(vec![
        tool_response,
        make_text_response("Done!"),
    ]));
    let mut params = make_query_params(vec![make_user_message_for_test("Run echo hello")]);
    params.gates.emit_tool_use_summaries = emit_tool_use_summaries;

    query(params, deps).collect().await
}

pub(super) async fn run_observable_input_backfill_case(
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

pub(super) fn yielded_tool_input(items: &[QueryYield], tool_name: &str) -> Value {
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

pub(super) fn request_tool_input(
    params: &[ModelCallParams],
    request_index: usize,
    tool_name: &str,
) -> Value {
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

/// MockDeps that returns image content in ToolResult.model_content
pub(super) struct ImageMockDeps {
    pub(super) responses: parking_lot::Mutex<Vec<ModelResponse>>,
    pub(super) aborted: std::sync::atomic::AtomicBool,
}

impl ImageMockDeps {
    pub(super) fn new(responses: Vec<ModelResponse>) -> Self {
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
            usage: resp.assistant_message.usage.clone().unwrap_or_default(),
        });
        for (i, block) in resp.assistant_message.content.iter().enumerate() {
            events.push(StreamEvent::ContentBlockStart {
                index: i,
                content_block: block.clone(),
            });
            events.push(StreamEvent::ContentBlockStop { index: i });
        }
        events.push(StreamEvent::MessageDelta {
            delta: allthecodes_engine::types::message::MessageDelta {
                stop_reason: resp.assistant_message.stop_reason.clone(),
            },
            usage: resp.assistant_message.usage.clone(),
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
            result: allthecodes_engine::types::tool::ToolResult {
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

// ---------------------------------------------------------------------------
// Computer Use end-to-end smoke test
// ---------------------------------------------------------------------------

/// MockDeps that dispatches tool results by tool name:
/// - screenshot 鈫?image content
/// - left_click / type_text 鈫?text confirmation
pub(super) struct CuMockDeps {
    pub(super) responses: parking_lot::Mutex<Vec<ModelResponse>>,
    pub(super) aborted: std::sync::atomic::AtomicBool,
}

impl CuMockDeps {
    pub(super) fn new(responses: Vec<ModelResponse>) -> Self {
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
            usage: resp.assistant_message.usage.clone().unwrap_or_default(),
        });
        for (i, block) in resp.assistant_message.content.iter().enumerate() {
            events.push(StreamEvent::ContentBlockStart {
                index: i,
                content_block: block.clone(),
            });
            events.push(StreamEvent::ContentBlockStop { index: i });
        }
        events.push(StreamEvent::MessageDelta {
            delta: allthecodes_engine::types::message::MessageDelta {
                stop_reason: resp.assistant_message.stop_reason.clone(),
            },
            usage: resp.assistant_message.usage.clone(),
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
            allthecodes_engine::types::tool::ToolResult {
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
            allthecodes_engine::types::tool::ToolResult {
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
