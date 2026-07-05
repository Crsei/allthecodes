//! Helper types and functions for the query loop.
//!
//! Extracted from loop_impl.rs to keep the core stream! macro body focused.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::warn;
use uuid::Uuid;

use crate::types::message::{
    AssistantMessage, ContentBlock, MessageContent, ToolResultContent, UserMessage,
};
use crate::types::tool::{ToolProgress, Tools};
use allthecodes_api::api::streaming::CompletedToolUse;

use super::deps::{QueryDeps, ToolExecRequest, ToolExecResult};

/// Maximum number of concurrent tool executions.
/// Matches codex DEFAULT_AGENT_MAX_THREADS.
const MAX_CONCURRENT_TOOLS: usize = 6;

type ToolUseTuple = (String, String, serde_json::Value);
type ToolUseBatch = (bool, Vec<ToolUseTuple>);

struct StartedStreamingTool {
    tool_use_id: String,
    tool_name: String,
    handle: JoinHandle<anyhow::Result<ToolExecResult>>,
}

pub(crate) struct FinishedStreamingToolExecutions {
    pub started_tool_use_ids: HashSet<String>,
    pub results: Vec<ToolExecResult>,
}

pub(crate) struct StreamingToolExecutor {
    started: Vec<StartedStreamingTool>,
    blocked_by_serial_tool: bool,
}

impl StreamingToolExecutor {
    pub(crate) fn new() -> Self {
        Self {
            started: Vec::new(),
            blocked_by_serial_tool: false,
        }
    }

    pub(crate) fn add_tool_use(
        &mut self,
        deps: Arc<dyn QueryDeps>,
        tools: &Tools,
        parent_message: &AssistantMessage,
        on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
        tool_use: CompletedToolUse,
    ) -> bool {
        if self.blocked_by_serial_tool {
            return false;
        }

        let is_safe = tools
            .iter()
            .find(|tool| tool.name() == tool_use.name)
            .is_some_and(|tool| tool.is_concurrency_safe(&tool_use.input));
        if !is_safe {
            self.blocked_by_serial_tool = true;
            return false;
        }

        let tools = tools.clone();
        let parent = parent_message.clone();
        let request = ToolExecRequest {
            tool_use_id: tool_use.id.clone(),
            tool_name: tool_use.name.clone(),
            input: tool_use.input,
            langfuse_batch_span: None,
        };
        let tool_use_id = request.tool_use_id.clone();
        let tool_name = request.tool_name.clone();
        let handle = tokio::spawn(async move {
            deps.execute_tool(request, &tools, &parent, on_progress)
                .await
        });
        self.started.push(StartedStreamingTool {
            tool_use_id,
            tool_name,
            handle,
        });
        true
    }

    pub(crate) async fn finish(self) -> FinishedStreamingToolExecutions {
        let mut started_tool_use_ids = HashSet::new();
        let mut results = Vec::new();

        for started in self.started {
            started_tool_use_ids.insert(started.tool_use_id.clone());
            match started.handle.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(error)) => {
                    warn!(error = %error, tool = %started.tool_name, "streaming tool execution error");
                    results.push(internal_tool_error_result(
                        started.tool_use_id,
                        started.tool_name,
                        error,
                    ));
                }
                Err(error) => {
                    warn!(error = %error, tool = %started.tool_name, "streaming tool task aborted or panicked");
                    results.push(internal_tool_error_result(
                        started.tool_use_id,
                        started.tool_name,
                        error,
                    ));
                }
            }
        }

        FinishedStreamingToolExecutions {
            started_tool_use_ids,
            results,
        }
    }

    pub(crate) fn abort(self) {
        for started in self.started {
            started.handle.abort();
        }
    }
}

/// Execute tool calls (batched: concurrency-safe ones together, rest serial).
///
/// Keep this helper thin: it owns batching/order only, then routes every tool
/// call through the canonical [`QueryDeps::execute_tool`] boundary.
pub(crate) async fn execute_tool_calls(
    deps: &Arc<dyn QueryDeps>,
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &crate::types::tool::Tools,
    parent_message: &AssistantMessage,
    on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
) -> Vec<ToolExecResult> {
    let mut results = Vec::new();

    // Partition: consecutive concurrency-safe tools -> one concurrent batch, rest serial
    let mut batches: Vec<ToolUseBatch> = Vec::new();

    for (id, name, input) in tool_uses {
        let tool = tools.iter().find(|t| t.name() == name);
        let is_safe = tool.is_some_and(|t| t.is_concurrency_safe(input));

        if is_safe {
            if let Some(last) = batches.last_mut() {
                if last.0 {
                    last.1.push((id.clone(), name.clone(), input.clone()));
                    continue;
                }
            }
        }

        batches.push((is_safe, vec![(id.clone(), name.clone(), input.clone())]));
    }

    for (batch_index, (is_concurrent, batch)) in batches.into_iter().enumerate() {
        let batch_tool_names = batch
            .iter()
            .map(|(_, name, _)| name.clone())
            .collect::<Vec<String>>();
        let batch_span = if is_concurrent && batch.len() > 1 {
            deps.langfuse_trace().as_ref().and_then(|trace| {
                crate::services::langfuse::create_tool_batch_span(
                    trace,
                    &batch_tool_names,
                    batch_index,
                )
            })
        } else {
            None
        };
        if is_concurrent && batch.len() > 1 {
            // Concurrent execution with concurrency cap
            let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TOOLS));
            let mut handles = Vec::new();
            for (id, name, input) in batch {
                let tool_use_id = id.clone();
                let tool_name = name.clone();
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(error) => {
                        warn!(%error, tool = %tool_name, "tool concurrency limiter closed");
                        results.push(internal_tool_error_result(tool_use_id, tool_name, error));
                        continue;
                    }
                };
                let deps = deps.clone();
                let parent = parent_message.clone();
                let tools = tools.clone();
                let batch_span = batch_span.clone();
                let on_progress_clone = on_progress.clone();
                let handle = tokio::spawn(async move {
                    let _permit = permit;
                    let req = ToolExecRequest {
                        tool_use_id: id,
                        tool_name: name,
                        input,
                        langfuse_batch_span: batch_span,
                    };
                    deps.execute_tool(req, &tools, &parent, on_progress_clone)
                        .await
                });
                handles.push((tool_use_id, tool_name, handle));
            }

            for (tool_use_id, tool_name, handle) in handles {
                match handle.await {
                    Ok(Ok(result)) => results.push(result),
                    Ok(Err(e)) => {
                        warn!(error = %e, tool = %tool_name, "tool execution error");
                        results.push(internal_tool_error_result(tool_use_id, tool_name, e));
                    }
                    Err(e) => {
                        warn!(error = %e, tool = %tool_name, "tool task aborted or panicked");
                        results.push(internal_tool_error_result(tool_use_id, tool_name, e));
                    }
                }
            }
        } else {
            // Serial execution
            for (id, name, input) in batch {
                let tool_use_id = id.clone();
                let tool_name = name.clone();
                let req = ToolExecRequest {
                    tool_use_id: id,
                    tool_name: name,
                    input,
                    langfuse_batch_span: None,
                };
                match deps
                    .execute_tool(req, tools, parent_message, on_progress.clone())
                    .await
                {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        warn!(error = %e, tool = %tool_name, "tool execution error");
                        results.push(internal_tool_error_result(tool_use_id, tool_name, e));
                    }
                }
            }
        }
        crate::services::langfuse::end_span(batch_span);
    }

    results
}

fn internal_tool_error_result(
    tool_use_id: String,
    tool_name: String,
    error: impl std::fmt::Display,
) -> ToolExecResult {
    ToolExecResult {
        tool_use_id,
        tool_name,
        effective_input: serde_json::Value::Null,
        result: crate::types::tool::ToolResult {
            data: serde_json::json!(format!("Internal error: {}", error)),
            new_messages: vec![],
            ..Default::default()
        },
        is_error: true,
        hook_stopped_continuation: false,
        duration_ms: None,
        permission_decision: None,
        brief_message: None,
    }
}

pub(crate) fn merge_tool_results_by_tool_use_order(
    tool_uses: &[(String, String, serde_json::Value)],
    streamed_results: Vec<ToolExecResult>,
    remaining_results: Vec<ToolExecResult>,
) -> Vec<ToolExecResult> {
    let mut by_id = streamed_results
        .into_iter()
        .chain(remaining_results)
        .map(|result| (result.tool_use_id.clone(), result))
        .collect::<HashMap<_, _>>();

    let mut ordered = Vec::new();
    for (tool_use_id, _, _) in tool_uses {
        if let Some(result) = by_id.remove(tool_use_id) {
            ordered.push(result);
        }
    }
    ordered.extend(by_id.into_values());
    ordered
}

pub(crate) fn backfill_observable_tool_inputs<'a>(
    assistant: &'a AssistantMessage,
    tools: &Tools,
) -> Cow<'a, AssistantMessage> {
    let mut cloned_content: Option<Vec<ContentBlock>> = None;

    for (index, block) in assistant.content.iter().enumerate() {
        let ContentBlock::ToolUse { id, name, input } = block else {
            continue;
        };
        let Some(original_input) = input.as_object() else {
            continue;
        };
        let Some(tool) = tools.iter().find(|tool| tool.name() == name) else {
            continue;
        };

        let mut input_copy = original_input.clone();
        tool.backfill_observable_input(&mut input_copy);

        let added_fields = input_copy
            .keys()
            .any(|key| !original_input.contains_key(key));
        if !added_fields {
            continue;
        }

        let content = cloned_content.get_or_insert_with(|| assistant.content.clone());
        content[index] = ContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: serde_json::Value::Object(input_copy),
        };
    }

    match cloned_content {
        Some(content) => Cow::Owned(AssistantMessage {
            content,
            ..assistant.clone()
        }),
        None => Cow::Borrowed(assistant),
    }
}

/// Create an abort placeholder assistant message.
pub(crate) fn make_abort_message(deps: &Arc<dyn QueryDeps>, reason: &str) -> AssistantMessage {
    AssistantMessage {
        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "assistant".to_string(),
        content: vec![],
        usage: None,
        stop_reason: Some(reason.to_string()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    }
}

/// Create an API error assistant message.
pub(crate) fn make_error_message(deps: &Arc<dyn QueryDeps>, error: &str) -> AssistantMessage {
    AssistantMessage {
        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: format!("API error: {}", error),
        }],
        usage: None,
        stop_reason: Some("error".to_string()),
        is_api_error_message: true,
        api_error: Some(error.to_string()),
        cost_usd: 0.0,
    }
}

/// Create a system-injected user message.
pub(crate) fn make_user_message(
    deps: &Arc<dyn QueryDeps>,
    content: &str,
    is_meta: bool,
) -> UserMessage {
    UserMessage {
        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(content.to_string()),
        is_meta,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }
}

/// Build the canonical user-facing `tool_result` message for a completed tool.
///
/// This is the only query-loop path that converts [`ToolExecResult`] into the
/// next-turn model context. Keep result normalization here so post-stream and
/// future stream-time tool execution produce identical user messages.
pub(crate) fn make_tool_result_user_message(
    deps: &Arc<dyn QueryDeps>,
    exec_result: &ToolExecResult,
    source_tool_assistant_uuid: Uuid,
) -> UserMessage {
    let (content, display_text) = normalize_tool_result_content(exec_result);
    let tool_result_block = ContentBlock::ToolResult {
        tool_use_id: exec_result.tool_use_id.clone(),
        content,
        is_error: exec_result.is_error,
    };

    UserMessage {
        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![tool_result_block]),
        is_meta: true,
        tool_use_result: Some(display_text),
        source_tool_assistant_uuid: Some(source_tool_assistant_uuid),
    }
}

fn normalize_tool_result_content(exec_result: &ToolExecResult) -> (ToolResultContent, String) {
    if exec_result.is_error {
        let text = format!("Error: {}", exec_result.result.data);
        return (ToolResultContent::Text(text.clone()), text);
    }

    if let Some(ref model_content) = exec_result.result.model_content {
        let preview = exec_result
            .result
            .display_preview
            .clone()
            .unwrap_or_else(|| exec_result.result.data.to_string());
        return (model_content.clone(), preview);
    }

    let text = exec_result.result.data.to_string();
    let display_text = exec_result
        .result
        .display_preview
        .clone()
        .unwrap_or_else(|| text.clone());
    (ToolResultContent::Text(text), display_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use futures::Stream;
    use serde_json::Value;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::deps::{CompactionResult, ModelCallParams, ModelResponse};
    use super::super::recovery::{
        classify_model_call_failure, strip_fallback_signature_blocks, ModelCallFailureRecovery,
        ModelCallFailureStage,
    };
    use crate::types::app_state::AppState;
    use crate::types::message::{Message, StreamEvent, Usage};
    use crate::types::state::AutoCompactTracking;
    use crate::types::tool::{Tool, ToolResult, ToolUseContext, Tools};

    struct BatchTool {
        name: &'static str,
        concurrency_safe: bool,
    }

    enum BackfillMode {
        AddField,
        OverwriteOnly,
    }

    struct BackfillTool {
        name: &'static str,
        mode: BackfillMode,
    }

    #[async_trait::async_trait]
    impl Tool for BatchTool {
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
    impl Tool for BackfillTool {
        fn name(&self) -> &str {
            self.name
        }

        async fn description(&self, _input: &Value) -> String {
            String::new()
        }

        fn input_json_schema(&self) -> Value {
            serde_json::json!({})
        }

        fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
            match self.mode {
                BackfillMode::AddField => {
                    input.insert("type".to_string(), serde_json::json!("message"));
                }
                BackfillMode::OverwriteOnly => {
                    input.insert("file_path".to_string(), serde_json::json!("/abs/file.txt"));
                }
            }
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

    struct RecordingDeps {
        active: AtomicUsize,
        max_active: AtomicUsize,
        events: parking_lot::Mutex<Vec<String>>,
        aborted: AtomicBool,
        panic_tool_use_id: Option<&'static str>,
    }

    impl RecordingDeps {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                events: parking_lot::Mutex::new(Vec::new()),
                aborted: AtomicBool::new(false),
                panic_tool_use_id: None,
            }
        }

        fn panicking_on(tool_use_id: &'static str) -> Self {
            Self {
                panic_tool_use_id: Some(tool_use_id),
                ..Self::new()
            }
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().clone()
        }
    }

    #[async_trait::async_trait]
    impl QueryDeps for RecordingDeps {
        async fn call_model(&self, _params: ModelCallParams) -> Result<ModelResponse> {
            anyhow::bail!("not used")
        }

        async fn call_model_streaming(
            &self,
            _params: ModelCallParams,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
            anyhow::bail!("not used")
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

        async fn reactive_compact(
            &self,
            _messages: Vec<Message>,
        ) -> Result<Option<CompactionResult>> {
            Ok(None)
        }

        async fn execute_tool(
            &self,
            request: ToolExecRequest,
            _tools: &Tools,
            _parent: &AssistantMessage,
            _on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
        ) -> Result<ToolExecResult> {
            if self.panic_tool_use_id == Some(request.tool_use_id.as_str()) {
                panic!("intentional test panic for {}", request.tool_use_id);
            }

            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.events
                .lock()
                .push(format!("start:{}", request.tool_use_id));

            tokio::time::sleep(Duration::from_millis(25)).await;

            self.events
                .lock()
                .push(format!("end:{}", request.tool_use_id));
            self.active.fetch_sub(1, Ordering::SeqCst);

            Ok(ToolExecResult {
                tool_use_id: request.tool_use_id,
                tool_name: request.tool_name,
                effective_input: request.input,
                result: ToolResult {
                    data: serde_json::json!("ok"),
                    new_messages: vec![],
                    ..Default::default()
                },
                is_error: false,
                hook_stopped_continuation: false,
                duration_ms: None,
                permission_decision: None,
                brief_message: None,
            })
        }

        fn get_app_state(&self) -> AppState {
            AppState::default()
        }

        fn uuid(&self) -> String {
            uuid::Uuid::new_v4().to_string()
        }

        fn is_aborted(&self) -> bool {
            self.aborted.load(Ordering::Relaxed)
        }

        fn get_tools(&self) -> Tools {
            vec![]
        }

        async fn refresh_tools(&self) -> Result<Tools> {
            Ok(vec![])
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn model_call_failure_classifier_separates_stage_recovery_paths() {
        assert_eq!(
            classify_model_call_failure(
                ModelCallFailureStage::RequestStart,
                Some("claude-fallback"),
                "claude-primary",
                "prompt_too_long: context window exceeded",
            ),
            ModelCallFailureRecovery::PromptTooLong
        );
        assert_eq!(
            classify_model_call_failure(
                ModelCallFailureStage::RequestStart,
                Some("claude-fallback"),
                "claude-primary",
                "529 overloaded: high demand",
            ),
            ModelCallFailureRecovery::Fallback {
                model: "claude-fallback".to_string()
            }
        );
        assert_eq!(
            classify_model_call_failure(
                ModelCallFailureStage::StreamInterrupted,
                Some("claude-fallback"),
                "claude-primary",
                "529 overloaded during stream",
            ),
            ModelCallFailureRecovery::Fallback {
                model: "claude-fallback".to_string()
            }
        );
        assert_eq!(
            classify_model_call_failure(
                ModelCallFailureStage::StreamInterrupted,
                Some("claude-fallback"),
                "claude-primary",
                "prompt_too_long after stream started",
            ),
            ModelCallFailureRecovery::Terminal
        );
    }

    #[test]
    fn fallback_signature_stripping_removes_signature_blocks_only_from_assistant_messages() {
        let source = vec![
            Message::Assistant(AssistantMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Text {
                        text: "keep text".to_string(),
                    },
                    ContentBlock::Thinking {
                        thinking: "private chain".to_string(),
                        signature: Some("old-model-signature".to_string()),
                    },
                    ContentBlock::RedactedThinking {
                        data: "redacted-signature-payload".to_string(),
                    },
                    ContentBlock::ConnectorText {
                        connector_text: "signed connector output".to_string(),
                        signature: Some("old-connector-signature".to_string()),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "Read".to_string(),
                        input: serde_json::json!({"file_path": "Cargo.toml"}),
                    },
                ],
                usage: Some(Usage::default()),
                stop_reason: Some("tool_use".to_string()),
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.0,
            }),
            Message::User(UserMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                role: "user".to_string(),
                content: MessageContent::Text("keep user".to_string()),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            }),
        ];

        let stripped = strip_fallback_signature_blocks(&source);

        match &stripped[0] {
            Message::Assistant(assistant) => {
                assert_eq!(assistant.content.len(), 2);
                assert!(matches!(
                    assistant.content[0],
                    ContentBlock::Text { ref text } if text == "keep text"
                ));
                assert!(matches!(
                    assistant.content[1],
                    ContentBlock::ToolUse { ref id, .. } if id == "toolu_1"
                ));
            }
            other => panic!("expected assistant message, got {:?}", other),
        }
        assert!(matches!(
            &source[0],
            Message::Assistant(assistant) if assistant.content.len() == 5
        ));
        assert!(matches!(&stripped[1], Message::User(_)));
    }

    #[test]
    fn observable_input_backfill_keeps_byte_identity_when_only_overwriting_fields() {
        let assistant = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_path".to_string(),
                name: "PathTool".to_string(),
                input: serde_json::json!({"file_path": "file.txt"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let before = serde_json::to_string(&assistant).expect("serialize assistant");
        let tools: Tools = vec![Arc::new(BackfillTool {
            name: "PathTool",
            mode: BackfillMode::OverwriteOnly,
        })];

        let backfilled = backfill_observable_tool_inputs(&assistant, &tools);

        assert!(matches!(backfilled, Cow::Borrowed(_)));
        let after = serde_json::to_string(&assistant).expect("serialize assistant");
        assert_eq!(after, before);
    }

    #[test]
    fn observable_input_backfill_clones_when_adding_fields() {
        let assistant = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_msg".to_string(),
                name: "MessageTool".to_string(),
                input: serde_json::json!({"to": "worker", "message": "hello"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let tools: Tools = vec![Arc::new(BackfillTool {
            name: "MessageTool",
            mode: BackfillMode::AddField,
        })];

        let backfilled = backfill_observable_tool_inputs(&assistant, &tools);

        let Cow::Owned(backfilled) = backfilled else {
            panic!("expected observable clone when backfill adds fields");
        };
        match &backfilled.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert_eq!(input.get("type"), Some(&serde_json::json!("message")));
            }
            other => panic!("expected tool_use, got {:?}", other),
        }
        match &assistant.content[0] {
            ContentBlock::ToolUse { input, .. } => {
                assert!(input.get("type").is_none());
            }
            other => panic!("expected tool_use, got {:?}", other),
        }
    }

    #[test]
    fn tool_result_user_message_normalizes_plain_text_result() {
        let deps: Arc<dyn QueryDeps> = Arc::new(RecordingDeps::new());
        let source_uuid = uuid::Uuid::new_v4();
        let exec_result = ToolExecResult {
            tool_use_id: "tu_text".to_string(),
            tool_name: "TextTool".to_string(),
            effective_input: serde_json::Value::Null,
            result: ToolResult {
                data: serde_json::json!({"ok": true}),
                new_messages: vec![],
                ..Default::default()
            },
            is_error: false,
            hook_stopped_continuation: false,
            duration_ms: None,
            permission_decision: None,
            brief_message: None,
        };

        let user_msg = make_tool_result_user_message(&deps, &exec_result, source_uuid);

        assert!(user_msg.is_meta);
        assert_eq!(user_msg.source_tool_assistant_uuid, Some(source_uuid));
        assert_eq!(user_msg.tool_use_result.as_deref(), Some(r#"{"ok":true}"#));
        match &user_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    assert_eq!(tool_use_id, "tu_text");
                    assert!(!is_error);
                    match content {
                        ToolResultContent::Text(text) => assert_eq!(text, r#"{"ok":true}"#),
                        other => panic!("expected text tool result, got {:?}", other),
                    }
                }
                other => panic!("expected tool result block, got {:?}", other),
            },
            other => panic!("expected block user message, got {:?}", other),
        }
    }

    #[test]
    fn tool_result_user_message_uses_display_preview_without_replacing_model_data() {
        let deps: Arc<dyn QueryDeps> = Arc::new(RecordingDeps::new());
        let source_uuid = uuid::Uuid::new_v4();
        let exec_result = ToolExecResult {
            tool_use_id: "tu_preview".to_string(),
            tool_name: "Workflow".to_string(),
            effective_input: serde_json::Value::Null,
            result: ToolResult {
                data: serde_json::json!({"workflow_id": "workflow-1", "status": "started"}),
                display_preview: Some("Workflow workflow-1 is started".to_string()),
                new_messages: vec![],
                ..Default::default()
            },
            is_error: false,
            hook_stopped_continuation: false,
            duration_ms: None,
            permission_decision: None,
            brief_message: None,
        };

        let user_msg = make_tool_result_user_message(&deps, &exec_result, source_uuid);

        assert_eq!(
            user_msg.tool_use_result.as_deref(),
            Some("Workflow workflow-1 is started")
        );
        match &user_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult { content, .. } => match content {
                    ToolResultContent::Text(text) => {
                        let actual: serde_json::Value =
                            serde_json::from_str(text).expect("tool result JSON");
                        assert_eq!(
                            actual,
                            serde_json::json!({
                                "status": "started",
                                "workflow_id": "workflow-1",
                            })
                        );
                    }
                    other => panic!("expected text tool result, got {:?}", other),
                },
                other => panic!("expected tool result block, got {:?}", other),
            },
            other => panic!("expected block user message, got {:?}", other),
        }
    }

    #[test]
    fn tool_result_user_message_preserves_model_content_and_preview() {
        let deps: Arc<dyn QueryDeps> = Arc::new(RecordingDeps::new());
        let source_uuid = uuid::Uuid::new_v4();
        let image = ContentBlock::Image {
            source: crate::types::message::ImageSource {
                source_type: "base64".to_string(),
                media_type: "image/png".to_string(),
                data: "iVBORw0KGgo=".to_string(),
            },
        };
        let exec_result = ToolExecResult {
            tool_use_id: "tu_image".to_string(),
            tool_name: "Screenshot".to_string(),
            effective_input: serde_json::Value::Null,
            result: ToolResult {
                data: serde_json::json!({"raw": "large"}),
                model_content: Some(ToolResultContent::Blocks(vec![image])),
                display_preview: Some("[Screenshot: image/png]".to_string()),
                new_messages: vec![],
                ..Default::default()
            },
            is_error: false,
            hook_stopped_continuation: false,
            duration_ms: None,
            permission_decision: None,
            brief_message: None,
        };

        let user_msg = make_tool_result_user_message(&deps, &exec_result, source_uuid);

        assert_eq!(
            user_msg.tool_use_result.as_deref(),
            Some("[Screenshot: image/png]")
        );
        match &user_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    assert!(!is_error);
                    match content {
                        ToolResultContent::Blocks(inner) => {
                            assert!(matches!(inner.first(), Some(ContentBlock::Image { .. })));
                        }
                        other => panic!("expected structured tool result, got {:?}", other),
                    }
                }
                other => panic!("expected tool result block, got {:?}", other),
            },
            other => panic!("expected block user message, got {:?}", other),
        }
    }

    #[test]
    fn tool_result_user_message_normalizes_errors_before_model_content() {
        let deps: Arc<dyn QueryDeps> = Arc::new(RecordingDeps::new());
        let source_uuid = uuid::Uuid::new_v4();
        let exec_result = ToolExecResult {
            tool_use_id: "tu_error".to_string(),
            tool_name: "FailingTool".to_string(),
            effective_input: serde_json::Value::Null,
            result: ToolResult {
                data: serde_json::json!("permission denied"),
                model_content: Some(ToolResultContent::Text("ignored".to_string())),
                display_preview: Some("ignored".to_string()),
                new_messages: vec![],
                ..Default::default()
            },
            is_error: true,
            hook_stopped_continuation: false,
            duration_ms: None,
            permission_decision: None,
            brief_message: None,
        };

        let user_msg = make_tool_result_user_message(&deps, &exec_result, source_uuid);

        assert_eq!(
            user_msg.tool_use_result.as_deref(),
            Some("Error: \"permission denied\"")
        );
        match &user_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    assert!(is_error);
                    match content {
                        ToolResultContent::Text(text) => {
                            assert_eq!(text, "Error: \"permission denied\"");
                        }
                        other => panic!("expected text error result, got {:?}", other),
                    }
                }
                other => panic!("expected tool result block, got {:?}", other),
            },
            other => panic!("expected block user message, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_tool_calls_batches_consecutive_safe_tools_only() {
        let concrete_deps = Arc::new(RecordingDeps::new());
        let deps: Arc<dyn QueryDeps> = concrete_deps.clone();
        let tools: Tools = vec![
            Arc::new(BatchTool {
                name: "Safe",
                concurrency_safe: true,
            }),
            Arc::new(BatchTool {
                name: "Unsafe",
                concurrency_safe: false,
            }),
        ];
        let tool_uses = vec![
            (
                "safe_1".to_string(),
                "Safe".to_string(),
                serde_json::json!({}),
            ),
            (
                "safe_2".to_string(),
                "Safe".to_string(),
                serde_json::json!({}),
            ),
            (
                "unsafe_1".to_string(),
                "Unsafe".to_string(),
                serde_json::json!({}),
            ),
            (
                "safe_3".to_string(),
                "Safe".to_string(),
                serde_json::json!({}),
            ),
        ];

        let results = execute_tool_calls(&deps, &tool_uses, &tools, &parent_message(), None).await;

        assert_eq!(
            concrete_deps.max_active.load(Ordering::SeqCst),
            2,
            "only the consecutive safe batch should run concurrently"
        );
        assert_eq!(
            results
                .iter()
                .map(|result| result.tool_use_id.as_str())
                .collect::<Vec<_>>(),
            vec!["safe_1", "safe_2", "unsafe_1", "safe_3"],
            "results should stay in tool_use order"
        );

        let events = concrete_deps.events();
        let end_safe_1 = events
            .iter()
            .position(|event| event == "end:safe_1")
            .unwrap();
        let end_safe_2 = events
            .iter()
            .position(|event| event == "end:safe_2")
            .unwrap();
        let start_unsafe = events
            .iter()
            .position(|event| event == "start:unsafe_1")
            .unwrap();
        assert!(
            start_unsafe > end_safe_1 && start_unsafe > end_safe_2,
            "unsafe tool must not start until the preceding safe batch finishes: {:?}",
            events
        );

        let end_unsafe = events
            .iter()
            .position(|event| event == "end:unsafe_1")
            .unwrap();
        let start_safe_3 = events
            .iter()
            .position(|event| event == "start:safe_3")
            .unwrap();
        assert!(
            start_safe_3 > end_unsafe,
            "later safe tool must wait behind preceding unsafe tool: {:?}",
            events
        );
    }

    #[tokio::test]
    async fn execute_tool_calls_synthesizes_failed_result_for_spawned_tool_panic() {
        let deps: Arc<dyn QueryDeps> = Arc::new(RecordingDeps::panicking_on("panic_tool"));
        let tools: Tools = vec![Arc::new(BatchTool {
            name: "Safe",
            concurrency_safe: true,
        })];
        let tool_uses = vec![
            (
                "panic_tool".to_string(),
                "Safe".to_string(),
                serde_json::json!({}),
            ),
            (
                "ok_tool".to_string(),
                "Safe".to_string(),
                serde_json::json!({}),
            ),
        ];

        let results = execute_tool_calls(&deps, &tool_uses, &tools, &parent_message(), None).await;

        assert_eq!(results.len(), 2);
        let panic_result = results
            .iter()
            .find(|result| result.tool_use_id == "panic_tool")
            .expect("panicking tool should still emit a result");
        assert_eq!(panic_result.tool_name, "Safe");
        assert!(panic_result.is_error);
        assert!(panic_result
            .result
            .data
            .as_str()
            .is_some_and(|text| text.contains("Internal error:")));
    }
}
