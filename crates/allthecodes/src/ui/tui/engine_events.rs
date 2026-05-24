use crate::ui::app::App;
use crate::ui::permissions::PermissionChoice;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::QuerySource;
use allthecodes_engine::types::tool::ToolProgress;
use allthecodes_services::prompt_suggestion::PromptSuggestionService;
use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionEventPayload, PermissionRequestPayload,
    PermissionResponsePayload,
};
use allthecodes_types::message::ProgressMessage;
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, InfoLevel, Message, MessageContent, StreamEvent, SystemMessage,
    SystemSubtype, UserMessage,
};
use allthecodes_types::permission_events::{
    HookPermissionDecisionEvent, PermissionDecisionDebugEvent,
};
use allthecodes_types::sdk::SdkMessage;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
/// Tracks the partial assistant message being streamed.
pub(super) struct StreamingState {
    /// Accumulated content blocks from streaming events.
    blocks: Vec<ContentBlock>,
    /// Whether we are inside a content block.
    active: bool,
}

impl StreamingState {
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            active: false,
        }
    }

    fn clear(&mut self) {
        self.blocks.clear();
        self.active = false;
    }

    fn is_partial(&self) -> bool {
        self.active || !self.blocks.is_empty()
    }

    fn ensure_block(&mut self, index: usize, fallback: ContentBlock) -> &mut ContentBlock {
        while self.blocks.len() <= index {
            self.blocks.push(ContentBlock::Text {
                text: String::new(),
            });
        }
        if matches!(self.blocks[index], ContentBlock::Text { ref text } if text.is_empty()) {
            self.blocks[index] = fallback;
        }
        &mut self.blocks[index]
    }
}

// ---------------------------------------------------------------------------
// Engine event channel type
// ---------------------------------------------------------------------------

/// Events sent from engine tasks to the TUI main loop.
pub(super) enum EngineEvent {
    /// An SDK message from the engine stream.
    Sdk(Box<SdkMessage>),
    /// Progress from a long-running tool.
    ToolProgress(ProgressMessage),
    /// A tool permission prompt that must be answered by the UI.
    PermissionRequest {
        request: PermissionRequestPayload,
        response_tx: oneshot::Sender<PermissionResponsePayload>,
    },
    /// An AskUserQuestion prompt that must be answered by the UI.
    QuestionRequest {
        id: String,
        request: AskUserRequestPayload,
        response_tx: oneshot::Sender<String>,
    },
    HookPermissionDecision(HookPermissionDecisionEvent),
    PermissionDecisionDebug(PermissionDecisionDebugEvent),
    /// The engine query task has completed (stream exhausted).
    Done,
}

// ---------------------------------------------------------------------------
// Permission bridge
// ---------------------------------------------------------------------------

pub(super) fn install_tui_permission_callback(
    engine: &Arc<QueryEngine>,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    let callback: allthecodes_engine::types::tool::PermissionCallback =
        Arc::new(move |request: PermissionRequestPayload| {
            let tx = tx.clone();
            Box::pin(async move {
                let (response_tx, response_rx) = oneshot::channel();
                let event = EngineEvent::PermissionRequest {
                    request,
                    response_tx,
                };

                if tx.send(event).is_err() {
                    return PermissionResponsePayload::deny();
                }

                response_rx
                    .await
                    .unwrap_or_else(|_| PermissionResponsePayload::deny())
            })
        });
    engine.set_permission_callback(callback);
}

pub(super) fn install_tui_ask_user_callback(
    engine: &Arc<QueryEngine>,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    let callback: allthecodes_engine::types::tool::AskUserCallback =
        Arc::new(move |request: AskUserRequestPayload| {
            let tx = tx.clone();
            Box::pin(async move {
                let (response_tx, response_rx) = oneshot::channel();
                let id = uuid::Uuid::new_v4().to_string();
                let event = EngineEvent::QuestionRequest {
                    id,
                    request,
                    response_tx,
                };

                if tx.send(event).is_err() {
                    return String::new();
                }

                response_rx.await.unwrap_or_default()
            })
        });
    engine.set_ask_user_callback(callback);
}

pub(super) fn install_tui_permission_event_callback(
    engine: &Arc<QueryEngine>,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    let callback = Arc::new(move |event: PermissionEventPayload| {
        let engine_event = match event {
            PermissionEventPayload::HookDecision { event } => {
                EngineEvent::HookPermissionDecision(event)
            }
            PermissionEventPayload::DecisionDebug { event } => {
                EngineEvent::PermissionDecisionDebug(event)
            }
        };
        let _ = tx.send(engine_event);
    });
    engine.set_permission_event_callback(callback);
}

pub(super) fn install_tui_tool_progress_callback(
    engine: &Arc<QueryEngine>,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    let callback = Arc::new(move |progress: ToolProgress| {
        let _ = tx.send(EngineEvent::ToolProgress(
            progress_message_from_tool_progress(progress),
        ));
    });
    engine.set_tool_progress_callback(callback);
}

pub(super) fn progress_message_from_tool_progress(progress: ToolProgress) -> ProgressMessage {
    let message = tool_progress_display_text(&progress.data);
    let data = match progress.data {
        serde_json::Value::Object(mut map) => {
            map.insert("message".to_string(), serde_json::Value::String(message));
            serde_json::Value::Object(map)
        }
        other => serde_json::json!({
            "message": message,
            "raw": other,
        }),
    };

    ProgressMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        tool_use_id: progress.tool_use_id,
        data,
    }
}

fn tool_progress_display_text(data: &serde_json::Value) -> String {
    let tool = data
        .get("tool")
        .and_then(|v| v.as_str())
        .filter(|tool| !tool.is_empty())
        .unwrap_or("Tool");
    let elapsed_seconds = data
        .get("elapsed_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = data
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .replace(['\r', '\n'], " ");
    let total_lines = data.get("total_lines").and_then(|v| v.as_u64());

    let mut text = format!("{tool} running {elapsed_seconds}s");
    if let Some(total_lines) = total_lines {
        let label = if total_lines == 1 { "line" } else { "lines" };
        text.push_str(&format!("; {total_lines} {label}"));
    }
    let output = output.trim();
    if !output.is_empty() {
        let output = allthecodes_utils::messages::truncate_text(output, 120);
        text.push_str(&format!("; {output}"));
    }
    text
}

pub(super) fn handle_tool_progress(app: &mut App, progress: ProgressMessage) {
    if let Some(text) = progress.data.get("message").and_then(|v| v.as_str()) {
        app.set_spinner_message(text.to_string());
    }

    let replace_last = app.messages().last().is_some_and(|message| {
        matches!(
            message,
            Message::Progress(existing) if existing.tool_use_id == progress.tool_use_id
        )
    });

    if replace_last {
        app.replace_last_message(Message::Progress(progress));
    } else {
        app.add_message(Message::Progress(progress));
    }
}

pub(super) fn permission_choice_to_response(
    choice: &PermissionChoice,
) -> PermissionResponsePayload {
    let decision = match choice.decision {
        crate::ui::permissions::PermissionDecisionChoice::Allow => "allow",
        crate::ui::permissions::PermissionDecisionChoice::Deny => "deny",
        crate::ui::permissions::PermissionDecisionChoice::AlwaysAllow => "always_allow",
        crate::ui::permissions::PermissionDecisionChoice::Escalate => "escalate",
    };
    PermissionResponsePayload::new(decision, Some(choice.feedback.clone()))
}

// ---------------------------------------------------------------------------
// Engine query task
// ---------------------------------------------------------------------------

/// Spawn a tokio task that drives a QueryEngine query and sends results
/// through the `tx` channel.
pub(super) fn spawn_engine_query(
    engine: Arc<QueryEngine>,
    prompt: String,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    tokio::spawn(async move {
        let stream = engine.submit_message(&prompt, QuerySource::ReplMainThread);
        futures::pin_mut!(stream);

        while let Some(msg) = stream.next().await {
            if tx.send(EngineEvent::Sdk(Box::new(msg))).is_err() {
                break; // receiver dropped (app exited)
            }
        }

        let _ = tx.send(EngineEvent::Done);
    });
}

// ---------------------------------------------------------------------------
// SDK message handler
// ---------------------------------------------------------------------------

/// Handle an SDK message from the engine, updating the App state.
pub(super) fn handle_sdk_message(app: &mut App, msg: SdkMessage, ss: &mut StreamingState) {
    match msg {
        SdkMessage::SystemInit(_init) => {
            debug!("TUI: received SystemInit");
        }

        SdkMessage::StreamEvent(sdk_stream) => {
            match sdk_stream.event {
                StreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    let is_new_message = !ss.active;
                    if is_new_message {
                        // First content block — start a new streaming message
                        ss.blocks.clear();
                        ss.active = true;
                    }
                    while ss.blocks.len() <= index {
                        ss.blocks.push(ContentBlock::Text {
                            text: String::new(),
                        });
                    }
                    ss.blocks[index] = content_block;
                    if is_new_message {
                        app.add_message(make_partial_assistant(&ss.blocks));
                    } else {
                        app.replace_last_message(make_partial_assistant(&ss.blocks));
                    }
                }
                StreamEvent::ContentBlockDelta { index, ref delta } => {
                    let is_new_message = !ss.active && ss.blocks.is_empty();
                    if is_new_message {
                        ss.active = true;
                    }
                    let mut handled = false;
                    if stream_delta_type_matches(delta, "text_delta") {
                        if let Some(t) = delta.get("text").and_then(|v| v.as_str()) {
                            if let ContentBlock::Text { text } = ss.ensure_block(
                                index,
                                ContentBlock::Text {
                                    text: String::new(),
                                },
                            ) {
                                text.push_str(t);
                            }
                            if is_new_message {
                                app.add_message(make_partial_assistant(&ss.blocks));
                            } else {
                                app.replace_last_message(make_partial_assistant(&ss.blocks));
                            }
                            handled = true;
                        }
                    }
                    if !handled && stream_delta_type_matches(delta, "thinking_delta") {
                        if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                            if let ContentBlock::Thinking { thinking, .. } = ss.ensure_block(
                                index,
                                ContentBlock::Thinking {
                                    thinking: String::new(),
                                    signature: None,
                                },
                            ) {
                                thinking.push_str(t);
                            }
                            if is_new_message {
                                app.add_message(make_partial_assistant(&ss.blocks));
                            } else {
                                app.replace_last_message(make_partial_assistant(&ss.blocks));
                            }
                        }
                    }
                    if !handled && stream_delta_type_matches(delta, "connector_text_delta") {
                        if let Some(t) = delta.get("connector_text").and_then(|v| v.as_str()) {
                            if let ContentBlock::ConnectorText { connector_text, .. } = ss
                                .ensure_block(
                                    index,
                                    ContentBlock::ConnectorText {
                                        connector_text: String::new(),
                                        signature: None,
                                    },
                                )
                            {
                                connector_text.push_str(t);
                            }
                            if is_new_message {
                                app.add_message(make_partial_assistant(&ss.blocks));
                            } else {
                                app.replace_last_message(make_partial_assistant(&ss.blocks));
                            }
                        }
                    }
                }
                StreamEvent::MessageStop => {
                    // Stream complete; the full Assistant message follows.
                    ss.active = false;
                }
                _ => {}
            }
        }

        SdkMessage::Assistant(assistant) => {
            // Replace the partial streaming message with the final one.
            if ss.is_partial() {
                app.replace_last_message(Message::Assistant(assistant.message));
                ss.clear();
            } else {
                app.add_message(Message::Assistant(assistant.message));
            }
        }

        SdkMessage::Tombstone(_) => {
            if ss.is_partial() {
                app.remove_last_message();
                ss.clear();
            }
        }

        SdkMessage::UserReplay(user) => {
            if user.is_replay && !user.is_synthetic {
                return;
            }

            let content = match user.content_blocks {
                Some(blocks) => MessageContent::Blocks(blocks),
                None => MessageContent::Text(user.content),
            };
            app.add_message(Message::User(UserMessage {
                uuid: user.uuid,
                timestamp: user.timestamp,
                role: "user".to_string(),
                content,
                is_meta: user.is_synthetic,
                tool_use_result: user.tool_use_result,
                source_tool_assistant_uuid: user.source_tool_assistant_uuid,
            }));
        }

        SdkMessage::Result(result) => {
            // Finalize any leftover streaming state
            ss.clear();

            app.set_streaming(false);
            app.update_session_cost(result.total_cost_usd);
            // Feed aggregate usage into the status-line payload (issue #11).
            // `result.usage` is engine `UsageTracking` (accumulated across
            // turns) — the payload wants per-session totals, so we pass
            // the totals straight through.
            app.update_session_usage(
                result.usage.total_input_tokens,
                result.usage.total_output_tokens,
                result.usage.total_cache_read_tokens,
                result.usage.total_cache_creation_tokens,
                result.usage.api_call_count,
            );
            if result.is_error {
                app.add_message(Message::System(SystemMessage {
                    uuid: uuid::Uuid::new_v4(),
                    timestamp: now_ts(),
                    subtype: SystemSubtype::Informational {
                        level: InfoLevel::Error,
                    },
                    content: result.result,
                }));
            }
            if result.duration_ms > 0 {
                app.add_message(Message::System(SystemMessage {
                    uuid: uuid::Uuid::new_v4(),
                    timestamp: now_ts(),
                    subtype: SystemSubtype::Informational {
                        level: InfoLevel::Info,
                    },
                    content: format!("worked for {}", format_duration_ms(result.duration_ms)),
                }));
            }

            // Generate next-prompt suggestions from last assistant turn
            generate_suggestions(app);

            debug!(
                turns = result.num_turns,
                cost = format!("{:.4}", result.total_cost_usd),
                duration_ms = result.duration_ms,
                "TUI: query completed"
            );
        }

        SdkMessage::ApiRetry(retry) => {
            app.set_spinner_message(format!(
                "Retrying ({}/{})...",
                retry.attempt, retry.max_retries
            ));
        }

        SdkMessage::CompactBoundary(_) => {
            app.add_message(Message::System(SystemMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: now_ts(),
                subtype: SystemSubtype::CompactBoundary {
                    compact_metadata: None,
                },
                content: String::new(),
            }));
        }

        _ => {}
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}s", ms / 1000)
    }
}

fn stream_delta_type_matches(delta: &serde_json::Value, expected: &str) -> bool {
    delta
        .get("type")
        .and_then(|v| v.as_str())
        .is_none_or(|actual| actual == expected)
}

/// Build a partial assistant message for streaming display.
fn make_partial_assistant(blocks: &[ContentBlock]) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        role: "assistant".to_string(),
        content: blocks.to_vec(),
        usage: None,
        stop_reason: None,
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    })
}
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a user message from text.
pub(super) fn create_user_message(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

/// Generate prompt suggestions from the last assistant message in the conversation.
fn generate_suggestions(app: &mut App) {
    let messages = app.messages();
    let mut svc = PromptSuggestionService::new(true);

    // Check suppression first (not enough messages, etc.)
    if let Some(reason) = svc.get_suppression_reason(messages.len(), false) {
        debug!("prompt suggestions suppressed: {:?}", reason);
        return;
    }

    if !svc.should_enable() {
        return;
    }

    // Find last assistant message
    let last_assistant = messages.iter().rev().find_map(|msg| match msg {
        Message::Assistant(a) => Some(a),
        _ => None,
    });
    let Some(assistant) = last_assistant else {
        return;
    };

    // Extract tool names and text summary
    let tool_names: Vec<String> = assistant
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::ToolUse { name, .. } = b {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    let summary: String = assistant
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(suggestions) = svc.try_generate(&summary, &tool_names) {
        app.set_suggestions(suggestions);
    }
}

/// Current UTC timestamp in seconds.
pub(super) fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}
