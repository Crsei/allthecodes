//! SdkMessage ->BackendMessage mapping.
//!
//! Pure mapping layer extracted from `headless.rs`.  Each [`SdkMessage`] variant
//! is translated into one or more [`BackendMessage`]s and written via the
//! [`FrontendSink`].  The only retained state is per-session tool-use context
//! used to classify later tool-result replay events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;

use parking_lot::Mutex;

use tracing::debug;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_ipc::adapters::{extract_tool_result_output, stream_event_to_backend_message};
use allthecodes_ipc::transport::FrontendSink;
use allthecodes_services::{
    cost_ledger,
    prompt_suggestion::PromptSuggestionService,
    search_tips::{SearchTipCandidate, SearchTipContext},
    skill_search_prefetch::{candidates_from_prefetch, ensure_turn_zero_skill_discovery},
};
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

fn session_report_summary(session_id: &str) -> BackendMessage {
    let not_generated = || BackendMessage::SessionReportSummary {
        session_id: session_id.to_string(),
        state: "not_generated".to_string(),
        policy: None,
        status: None,
        rounds: None,
        evidence_count: 0,
        missing_requirements: Vec::new(),
        risk_event_count: 0,
        cost_usd: None,
        integrity_valid: None,
    };
    let Ok(Some(rollout_path)) = allthecodes_session::record_replay::lookup_rollout(session_id)
    else {
        return not_generated();
    };
    let report_path = allthecodes_config::paths::runs_dir(session_id)
        .join(allthecodes_session::session_report::SESSION_REPORT_FILE_NAME);
    if !report_path.is_file() {
        return not_generated();
    }
    let Ok(read) = allthecodes_session::record_replay::read_rollout_file(&rollout_path) else {
        return BackendMessage::SessionReportSummary {
            session_id: session_id.to_string(),
            state: "generated".to_string(),
            policy: None,
            status: None,
            rounds: None,
            evidence_count: 0,
            missing_requirements: Vec::new(),
            risk_event_count: 0,
            cost_usd: None,
            integrity_valid: Some(false),
        };
    };
    let Ok(bytes) = std::fs::read(&report_path) else {
        return not_generated();
    };
    let Ok(report) =
        serde_json::from_slice::<allthecodes_session::session_report::SessionReportV1>(&bytes)
    else {
        return BackendMessage::SessionReportSummary {
            session_id: session_id.to_string(),
            state: "generated".to_string(),
            policy: None,
            status: None,
            rounds: None,
            evidence_count: 0,
            missing_requirements: Vec::new(),
            risk_event_count: 0,
            cost_usd: None,
            integrity_valid: Some(false),
        };
    };
    let missing_requirements = report
        .verification
        .missing_requirements
        .iter()
        .filter_map(|kind| serde_json::to_string(kind).ok())
        .map(|kind| kind.trim_matches('"').to_string())
        .collect();
    let integrity_valid =
        allthecodes_session::session_report::verify_session_report_file(&report_path, &read.lines)
            .unwrap_or(false);
    BackendMessage::SessionReportSummary {
        session_id: session_id.to_string(),
        state: "generated".to_string(),
        policy: Some(report.verification.policy),
        status: Some(
            serde_json::to_string(&report.verification.status)
                .unwrap_or_else(|_| "unknown".to_string())
                .trim_matches('"')
                .to_string(),
        ),
        rounds: Some(report.verification.rounds),
        evidence_count: report.verification.evidence_ids.len() as u64,
        missing_requirements,
        risk_event_count: report.risk_events.len() as u64,
        cost_usd: report.cost.map(|cost| cost.cost_usd),
        integrity_valid: Some(integrity_valid),
    }
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

        // ── BriefMessage ─────────────────────────────────────────
        SdkMessage::BriefMessage(brief) => sink.send(&BackendMessage::BriefMessage {
            message: brief.message.clone(),
            status: brief.status.as_str().to_string(),
            attachments: brief.attachments.clone(),
            level: brief.level.map(|level| level.as_str().to_string()),
            source_tool_name: brief.source_tool_name.clone(),
            tool_use_id: brief.tool_use_id.clone(),
            session_id: brief.session_id.clone(),
            timestamp: brief.timestamp,
        }),

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
                cache_read_input_tokens: r.usage.total_cache_read_tokens,
                cache_creation_input_tokens: r.usage.total_cache_creation_tokens,
                reasoning_output_tokens: r.usage.total_reasoning_output_tokens,
                api_call_count: r.usage.api_call_count,
                kind: Some("cumulative".to_string()),
            });

            let _ = sink.send(&session_report_summary(&r.session_id));

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
    let messages = engine.messages();
    let cost_summary = cost_ledger::get_session_cost_summary(&result.session_id, &messages);
    let (unknown_pricing_count, backfilled_count) = if cost_summary.api_calls > 0 {
        (
            Some(cost_summary.unknown_pricing_count),
            Some(cost_summary.backfilled_count),
        )
    } else {
        (None, None)
    };
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
        unknown_pricing_count,
        backfilled_count,
        session_duration_secs: Some(result.duration_ms / 1000),
        resolved_output_style_name: crate::ui::status_line_resolver::resolve_output_style_name(
            app_state.settings.output_style.as_deref(),
            cwd,
        ),
        editor_mode: app_state.settings.editor_mode.as_deref(),
        worktree: crate::ui::status_line_resolver::current_worktree_status_for_session(None),
        streaming: false,
        message_count: messages.len(),
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

    let session_id = engine.current_session_id().to_string();
    let search_candidates = skill_prefetch_candidates_for_session(&session_id);
    let search_context = (!search_candidates.is_empty()).then(|| SearchTipContext {
        session_id,
        now_ms: current_time_ms(),
    });

    if let Some(suggestions) =
        svc.try_generate_with_search_tips(&summary, &tool_names, search_context, &search_candidates)
    {
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

fn skill_prefetch_candidates_for_session(session_id: &str) -> Vec<SearchTipCandidate> {
    if session_id.is_empty() {
        return Vec::new();
    }

    let result = ensure_turn_zero_skill_discovery(session_id, "");
    candidates_from_prefetch(&result)
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_ipc_protocol::ToolResultContentInfo;
    use allthecodes_services::skill_search_prefetch::{
        collect_skill_discovery_prefetch, start_skill_discovery_prefetch, SkillPrefetchContext,
    };
    use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
    use allthecodes_types::message::{AssistantMessage, ImageSource, MessageContent, UserMessage};
    use allthecodes_types::sdk::{ResultSubtype, SdkResult};
    use serial_test::serial;
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
            verification_policy: None,
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

    struct FeatureGuard;

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

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn assistant_message(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 2,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
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
                    total_reasoning_output_tokens: 0,
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

    #[tokio::test]
    async fn result_usage_update_preserves_reasoning_tokens() {
        let engine = make_engine(env!("CARGO_MANIFEST_DIR"));
        let suggestion_svc = Arc::new(Mutex::new(PromptSuggestionService::new(false)));
        let sink = FrontendSink::memory();

        handle_sdk_message(
            &SdkMessage::Result(SdkResult {
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
                    total_reasoning_output_tokens: 42,
                    total_cost_usd: 0.1234,
                    api_call_count: 2,
                },
                permission_denials: vec![],
                structured_output: None,
                uuid: Uuid::new_v4(),
                errors: vec![],
            }),
            "message-1",
            &engine,
            &suggestion_svc,
            &sink,
        )
        .expect("handle result message");

        let reasoning_tokens = sink
            .captured()
            .into_iter()
            .find_map(|message| match message {
                BackendMessage::UsageUpdate {
                    reasoning_output_tokens,
                    ..
                } => Some(reasoning_output_tokens),
                _ => None,
            });

        assert_eq!(reasoning_tokens, Some(42));
    }

    #[test]
    fn headless_maps_sdk_brief_message_to_backend_brief_message() {
        let engine = make_engine(env!("CARGO_MANIFEST_DIR"));
        let suggestion_svc = Arc::new(Mutex::new(PromptSuggestionService::new(false)));
        let sink = FrontendSink::memory();

        handle_sdk_message(
            &SdkMessage::BriefMessage(allthecodes_types::brief::BriefMessagePayload {
                message: "brief body".into(),
                status: allthecodes_types::brief::BriefMessageStatus::Proactive,
                attachments: vec!["docs/brief.md".into()],
                level: Some(allthecodes_types::brief::BriefMessageLevel::Warning),
                source_tool_name: Some("Brief".into()),
                tool_use_id: Some("toolu-brief".into()),
                session_id: Some("session-1".into()),
                timestamp: Some(100),
            }),
            "message-1",
            &engine,
            &suggestion_svc,
            &sink,
        )
        .expect("handle brief message");

        let captured = sink.captured();
        assert!(matches!(
            captured.as_slice(),
            [BackendMessage::BriefMessage {
                message,
                status,
                attachments,
                level: Some(level),
                tool_use_id: Some(tool_use_id),
                session_id: Some(session_id),
                timestamp: Some(100),
                ..
            }] if message == "brief body"
                && status == "proactive"
                && attachments == &vec!["docs/brief.md".to_string()]
                && level == "warning"
                && tool_use_id == "toolu-brief"
                && session_id == "session-1"
        ));
    }

    #[test]
    #[serial]
    fn headless_suggestions_include_turn_zero_skill_discovery_tip() {
        let mut flags = FeatureFlags::all_disabled();
        flags.proactive = true;
        flags.experimental_skill_search = true;
        let _features = FeatureGuard::set(flags);
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            "Review Rust code without remote fetch",
        )]);
        let engine = make_engine(env!("CARGO_MANIFEST_DIR"));
        let session_id = engine.current_session_id().to_string();
        collect_skill_discovery_prefetch(start_skill_discovery_prefetch(SkillPrefetchContext {
            session_id: session_id.clone(),
            query: "rust".to_string(),
        }));
        engine.replace_messages(vec![
            user_message("I need help with Rust code"),
            assistant_message("I can help inspect the implementation."),
        ]);
        let suggestion_svc = Arc::new(Mutex::new(PromptSuggestionService::new(true)));
        let sink = FrontendSink::memory();

        generate_and_send_suggestions(&engine, &suggestion_svc, &sink);

        let suggestions = sink
            .captured()
            .into_iter()
            .find_map(|message| match message {
                BackendMessage::Suggestions { items } => Some(items),
                _ => None,
            });
        assert!(
            suggestions
                .unwrap_or_default()
                .iter()
                .any(|item| item.contains("/skills rust-review")),
            "headless suggestions should include turn-zero skill discovery"
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
