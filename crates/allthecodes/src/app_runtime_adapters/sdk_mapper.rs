//! SdkMessage ->BackendMessage mapping.
//!
//! Pure mapping layer extracted from `headless.rs`.  Each [`SdkMessage`] variant
//! is translated into one or more [`BackendMessage`]s and written via the
//! [`FrontendSink`].  The only retained state is per-session tool-use context
//! used to classify later tool-result replay events.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;

use parking_lot::Mutex;

use tracing::debug;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_ipc::adapters::{extract_tool_result_output, stream_event_to_backend_message};
use allthecodes_ipc::transport::FrontendSink;
use allthecodes_services::prompt_suggestion::PromptSuggestionService;
use allthecodes_tool_display::ToolClassifier;
use allthecodes_types::message::{ContentBlock, Message, StreamEvent, ToolResultContent};
use allthecodes_types::sdk::SdkMessage;
use allthecodes_types::tool_operation::OperationStatus;

use crate::ui::status_line::payload::{build_payload_from_snapshot, StatusLineSnapshot};
use allthecodes_ipc_protocol::BackendMessage;

/// Cache mapping (session_id, tool_use_id) -> (tool_name, tool_input) so that
/// ToolResult events arriving later can be classified with the original tool
/// context without leaking between sessions.
type ToolUseCacheKey = (String, String);

static TOOL_USE_CACHE: Lazy<Mutex<HashMap<ToolUseCacheKey, (String, serde_json::Value)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn cache_tool_use(session_id: &str, id: &str, name: &str, input: &serde_json::Value) {
    TOOL_USE_CACHE.lock().insert(
        (session_id.to_string(), id.to_string()),
        (name.to_string(), input.clone()),
    );
}

fn remove_tool_use(session_id: &str, id: &str) -> Option<(String, serde_json::Value)> {
    TOOL_USE_CACHE
        .lock()
        .remove(&(session_id.to_string(), id.to_string()))
}

fn clear_session_tool_uses(session_id: &str) {
    TOOL_USE_CACHE
        .lock()
        .retain(|(cached_session_id, _), _| cached_session_id != session_id);
}

// ---------------------------------------------------------------------------
// SdkMessage ->BackendMessage mapping
// ---------------------------------------------------------------------------

/// Map a single [`SdkMessage`] to the appropriate [`BackendMessage`](s) and
/// send them to the frontend.  This is the central dispatch for the headless
/// protocol — every `SdkMessage` variant is handled here.
pub fn handle_sdk_message(
    sdk_msg: &SdkMessage,
    message_id: &str,
    engine: &Arc<QueryEngine>,
    suggestion_svc: &Arc<Mutex<PromptSuggestionService>>,
    sink: &FrontendSink,
) -> std::io::Result<()> {
    match sdk_msg {
        // ── SystemInit ──────────────────────────────────────────
        SdkMessage::SystemInit(init) => sink.send(&BackendMessage::SystemInfo {
            text: format!(
                "Permission: {}, {} tools",
                init.permission_mode,
                init.tools.len(),
            ),
            level: "info".to_string(),
        }),

        // ── StreamEvent ─────────────────────────────────────────
        SdkMessage::StreamEvent(evt) => handle_stream_event(&evt.event, message_id, sink),

        // ── Assistant message ───────────────────────────────────
        SdkMessage::Assistant(a) => {
            // First send individual ToolUse messages for each tool call
            // so the frontend can render them immediately.
            for block in &a.message.content {
                if let ContentBlock::ToolUse { id, name, input }
                | ContentBlock::ServerToolUse { id, name, input } = block
                {
                    let operation =
                        ToolClassifier::classify(name, input, OperationStatus::InProgress);
                    cache_tool_use(&a.session_id, id, name, input);
                    let _ = sink.send(&BackendMessage::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        operation: Some(operation),
                    });
                }
            }

            // Then send the full assistant message
            let content =
                serde_json::to_value(&a.message.content).unwrap_or(serde_json::Value::Null);
            sink.send(&BackendMessage::AssistantMessage {
                id: a.message.uuid.to_string(),
                content,
                cost_usd: a.message.cost_usd,
            })
        }

        // ── UserReplay (includes tool results) ──────────────────
        SdkMessage::UserReplay(replay) => {
            if replay.is_synthetic {
                debug!("headless: user replay (synthetic): {}", replay.content);
            }

            // Extract and forward tool results from content blocks
            if let Some(ref blocks) = replay.content_blocks {
                let replay_tool_preview = if blocks
                    .iter()
                    .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
                    .count()
                    == 1
                {
                    replay.tool_use_result.clone()
                } else {
                    None
                };
                for block in blocks {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = block
                    {
                        let (output, content_infos) = match content {
                            ToolResultContent::Text(t) => (t.clone(), None),
                            ToolResultContent::Blocks(inner) => extract_tool_result_output(inner),
                        };
                        let output_display = replay_tool_preview.clone().unwrap_or(output);
                        let status = if *is_error {
                            OperationStatus::Error
                        } else {
                            OperationStatus::Resolved
                        };
                        let operation = remove_tool_use(&replay.session_id, tool_use_id).map(
                            |(tool_name, tool_input)| {
                                ToolClassifier::classify_with_result(
                                    &tool_name,
                                    &tool_input,
                                    status,
                                    Some(&output_display),
                                    *is_error,
                                )
                            },
                        );
                        let _ = sink.send(&BackendMessage::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            output: output_display,
                            is_error: *is_error,
                            content_blocks: content_infos,
                            result_summary: operation
                                .as_ref()
                                .and_then(|operation| operation.result_summary.clone()),
                            operation,
                        });
                        maybe_send_plan_workflow_from_tool_result(content, sink);
                    }
                }
            }

            Ok(())
        }

        // ── CompactBoundary ─────────────────────────────────────
        SdkMessage::CompactBoundary(boundary) => {
            let text = if let Some(ref meta) = boundary.compact_metadata {
                format!(
                    "Context compacted: {} ->{} tokens",
                    meta.pre_compact_token_count, meta.post_compact_token_count
                )
            } else {
                "Context compacted".to_string()
            };
            sink.send(&BackendMessage::SystemInfo {
                text,
                level: "info".to_string(),
            })
        }

        // ── ApiRetry ────────────────────────────────────────────
        SdkMessage::ApiRetry(retry) => sink.send(&BackendMessage::Error {
            message: format!(
                "API retry {}/{}: {} (waiting {}ms)",
                retry.attempt, retry.max_retries, retry.error, retry.retry_delay_ms
            ),
            recoverable: true,
        }),

        // ── ToolUseSummary ──────────────────────────────────────
        SdkMessage::ToolUseSummary(summary) => sink.send(&BackendMessage::SystemInfo {
            text: summary.summary.clone(),
            level: "info".to_string(),
        }),

        // ── GoalUpdated ────────────────────────────────────────
        SdkMessage::GoalUpdated(update) => sink.send(&BackendMessage::GoalUpdated {
            event: update.event.clone(),
            goal: update.goal.clone(),
        }),

        // ── Tombstone ───────────────────────────────────────────
        SdkMessage::Tombstone(_) => sink.send(&BackendMessage::Tombstone {
            message_id: message_id.to_string(),
        }),

        // ── Result ──────────────────────────────────────────────
        SdkMessage::Result(r) => {
            clear_session_tool_uses(&r.session_id);
            // Always send StreamEnd to clear UI streaming state
            let _ = sink.send(&BackendMessage::StreamEnd {
                message_id: message_id.to_string(),
            });

            let _ = sink.send(&BackendMessage::UsageUpdate {
                input_tokens: r.usage.total_input_tokens,
                output_tokens: r.usage.total_output_tokens,
                cost_usd: r.usage.total_cost_usd,
            });

            // Scriptable status-line snapshot (issue #11). We always emit
            // the payload so a frontend can run its own script even when
            // the Rust runner is disabled; `lines`/`error` come from the
            // Rust runner's cache when one is running.
            if let Ok(payload) = build_status_line_payload(engine, r) {
                let runner_output = engine.app_state().status_line_runner.latest();
                let lines = runner_output.lines(3);
                let error = runner_output.error.clone();
                let _ = sink.send(&BackendMessage::StatusLineUpdate {
                    payload,
                    lines,
                    error,
                });
            }

            if r.is_error {
                let _ = sink.send(&BackendMessage::Error {
                    message: r.result.clone(),
                    recoverable: true,
                });
            }

            // Generate prompt suggestions after query completion
            generate_and_send_suggestions(engine, suggestion_svc, sink);

            // Try to extract session memory insights
            engine.try_extract_session_memory();

            // Fire Notification hook (sound/alert when query finishes)
            {
                let hooks_map = engine.app_state().hooks;
                tokio::spawn(async move {
                    allthecodes_tools::hooks::fire_notification_hook(
                        "allthecodes",
                        "Response ready",
                        &hooks_map,
                    )
                    .await;
                });
            }

            Ok(())
        }
    }
}

fn maybe_send_plan_workflow_from_tool_result(content: &ToolResultContent, sink: &FrontendSink) {
    let ToolResultContent::Text(text) = content else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(record_value) = value.get("plan_workflow") else {
        return;
    };
    let Ok(record) = serde_json::from_value::<allthecodes_types::plan_workflow::PlanWorkflowRecord>(
        record_value.clone(),
    ) else {
        return;
    };

    let event = match record.approval_state {
        allthecodes_types::plan_workflow::PlanApprovalState::Approved => "approval_approved",
        allthecodes_types::plan_workflow::PlanApprovalState::Rejected => "approval_rejected",
        allthecodes_types::plan_workflow::PlanApprovalState::Pending => "approval_requested",
        allthecodes_types::plan_workflow::PlanApprovalState::NotRequested => "updated",
    };

    let _ = sink.send(&BackendMessage::PlanWorkflowEvent {
        event: event.to_string(),
        summary: allthecodes_types::plan_workflow::summarize(&record),
        record,
    });
}

// ---------------------------------------------------------------------------
// StreamEvent mapping
// ---------------------------------------------------------------------------

/// Map a [`StreamEvent`] to the appropriate [`BackendMessage`] and send it.
fn handle_stream_event(
    event: &StreamEvent,
    message_id: &str,
    sink: &FrontendSink,
) -> std::io::Result<()> {
    if let Some(message) = stream_event_to_backend_message(event, message_id) {
        sink.send(&message)
    } else {
        Ok(())
    }
}

/// Build the JSON payload published alongside every `Result` SDK message.
/// Extracted so `handle_sdk_message` stays readable.
///
/// The output is a free-form `serde_json::Value` so a future schema bump
/// (adding/removing fields) doesn't need an IPC protocol change — the
/// frontend validates it against its own shape.
fn build_status_line_payload(
    engine: &Arc<QueryEngine>,
    result: &allthecodes_types::sdk::SdkResult,
) -> serde_json::Result<serde_json::Value> {
    let app_state = engine.app_state();
    let cwd = std::path::Path::new(engine.cwd());
    let payload = build_payload_from_snapshot(StatusLineSnapshot {
        session_id: Some(result.session_id.clone()),
        model_id: &app_state.main_loop_model,
        backend: Some(&app_state.main_loop_backend),
        cwd,
        input_tokens: result.usage.total_input_tokens,
        output_tokens: result.usage.total_output_tokens,
        cache_read_tokens: result.usage.total_cache_read_tokens,
        cache_creation_tokens: result.usage.total_cache_creation_tokens,
        total_cost_usd: result.total_cost_usd,
        api_calls: result.usage.api_call_count,
        session_duration_secs: Some(result.duration_ms / 1000),
        resolved_output_style_name: crate::ui::status_line_resolver::resolve_output_style_name(
            app_state.settings.output_style.as_deref(),
            cwd,
        ),
        editor_mode: app_state.settings.editor_mode.as_deref(),
        worktree: crate::ui::status_line_resolver::current_worktree_status(),
        streaming: false,
        message_count: engine.messages().len(),
    });

    serde_json::to_value(&payload)
}

// ---------------------------------------------------------------------------
// Prompt suggestions
// ---------------------------------------------------------------------------

/// Generate prompt suggestions from the last assistant message and send them.
pub fn generate_and_send_suggestions(
    engine: &Arc<QueryEngine>,
    svc: &Arc<Mutex<PromptSuggestionService>>,
    sink: &FrontendSink,
) {
    let messages = engine.messages();

    let mut svc = svc.lock();

    // Check suppression (too few messages, rate-limited, etc.)
    if svc.get_suppression_reason(messages.len(), false).is_some() {
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
        let items: Vec<String> = suggestions
            .into_iter()
            .take(3)
            .map(|s| format!("{} {}", s.category.icon(), s.text))
            .collect();

        if !items.is_empty() {
            let _ = sink.send(&BackendMessage::Suggestions { items });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_ipc_protocol::ToolResultContentInfo;
    use allthecodes_types::message::ImageSource;
    use allthecodes_types::sdk::{ResultSubtype, SdkResult};
    use uuid::Uuid;

    fn make_engine(cwd: &str) -> Arc<QueryEngine> {
        Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: cwd.to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: Some(allthecodes_types::models::default_model_id()),
            auto_save_session: false,
            agent_context: None,
        }))
    }

    #[test]
    fn test_extract_text_only_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "line 1".into(),
            },
            ContentBlock::Text {
                text: "line 2".into(),
            },
        ];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "line 1\nline 2");
        assert!(infos.is_none(), "no non-text blocks ->None");
    }

    #[test]
    fn test_extract_image_block_shows_placeholder() {
        let blocks = vec![ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".into(),
                media_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            },
        }];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "[image: image/png]");
        let infos = infos.expect("should have content_infos");
        assert_eq!(infos.len(), 1);
        match &infos[0] {
            ToolResultContentInfo::Image { media_type, .. } => {
                assert_eq!(media_type, "image/png");
            }
            _ => panic!("expected Image info"),
        }
    }

    #[test]
    fn test_extract_mixed_text_and_image() {
        let blocks = vec![
            ContentBlock::Text {
                text: "screenshot taken".into(),
            },
            ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: "image/jpeg".into(),
                    data: "AAAA".into(),
                },
            },
        ];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert!(output.contains("screenshot taken"));
        assert!(output.contains("[image: image/jpeg]"));
        let infos = infos.expect("has image ->Some");
        assert_eq!(infos.len(), 2);
    }

    #[test]
    fn test_extract_empty_blocks() {
        let blocks: Vec<ContentBlock> = vec![];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "(no output)");
        assert!(infos.is_none());
    }

    #[test]
    fn tool_use_cache_is_scoped_by_session() {
        cache_tool_use(
            "session-a",
            "tool-1",
            "Read",
            &serde_json::json!({"file_path": "a.rs"}),
        );
        cache_tool_use(
            "session-b",
            "tool-1",
            "Bash",
            &serde_json::json!({"command": "cargo test"}),
        );

        let first = remove_tool_use("session-a", "tool-1").expect("session-a tool use");
        let second = remove_tool_use("session-b", "tool-1").expect("session-b tool use");

        assert_eq!(first.0, "Read");
        assert_eq!(first.1["file_path"], "a.rs");
        assert_eq!(second.0, "Bash");
        assert_eq!(second.1["command"], "cargo test");
    }

    #[test]
    fn status_line_payload_uses_runtime_snapshot_fields() {
        let engine = make_engine(env!("CARGO_MANIFEST_DIR"));
        engine.update_app_state(|state| {
            state.main_loop_backend = "native".into();
            state.settings.output_style = Some("explanatory".into());
            state.settings.editor_mode = Some("vim".into());
        });

        let payload = build_status_line_payload(
            &engine,
            &SdkResult {
                subtype: ResultSubtype::Success,
                is_error: false,
                duration_ms: 4200,
                duration_api_ms: 1500,
                num_turns: 1,
                result: "ok".into(),
                stop_reason: Some("end_turn".into()),
                session_id: engine.session_id.to_string(),
                total_cost_usd: 0.1234,
                usage: allthecodes_types::sdk::UsageTracking {
                    total_input_tokens: 1200,
                    total_output_tokens: 300,
                    total_cache_read_tokens: 20,
                    total_cache_creation_tokens: 10,
                    total_cost_usd: 0.1234,
                    api_call_count: 2,
                },
                permission_denials: vec![],
                structured_output: None,
                uuid: Uuid::new_v4(),
                errors: vec![],
            },
        )
        .unwrap();

        assert_eq!(
            payload
                .pointer("/outputStyle")
                .and_then(|value| value.as_str()),
            Some("explanatory")
        );
        assert_eq!(
            payload
                .pointer("/vim/mode")
                .and_then(|value| value.as_str()),
            Some("NORMAL")
        );
        assert_eq!(
            payload
                .pointer("/context/maxTokens")
                .and_then(|value| value.as_u64()),
            Some(200_000)
        );
        assert_eq!(
            payload
                .pointer("/workspace/projectDir")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty()),
            Some(true)
        );
        assert_eq!(
            payload
                .pointer("/cost/apiCalls")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn headless_stream_event_mapping_ignores_tool_input_delta() {
        let message = stream_event_to_backend_message(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({
                    "type": "input_json_delta",
                    "partial_json": "{\"file_path\":\"Cargo.toml\"}"
                }),
            },
            "message-1",
        );

        assert!(
            message.is_none(),
            "headless should not render tool input deltas as text"
        );
    }

    #[test]
    fn headless_stream_event_mapping_ignores_unsupported_text_like_delta() {
        let message = stream_event_to_backend_message(
            &StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({
                    "type": "connector_text_delta",
                    "text": "not assistant text"
                }),
            },
            "message-1",
        );

        assert!(
            message.is_none(),
            "headless should not render unsupported text-like deltas"
        );
    }

    #[test]
    fn headless_stream_event_mapping_keeps_text_and_thinking_deltas() {
        assert!(matches!(
            stream_event_to_backend_message(
                &StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({
                        "type": "text_delta",
                        "text": "hello"
                    }),
                },
                "message-1",
            ),
            Some(BackendMessage::StreamDelta { text, .. }) if text == "hello"
        ));

        assert!(matches!(
            stream_event_to_backend_message(
                &StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({
                        "type": "thinking_delta",
                        "thinking": "considering"
                    }),
                },
                "message-1",
            ),
            Some(BackendMessage::ThinkingDelta { thinking, .. }) if thinking == "considering"
        ));
    }
}
