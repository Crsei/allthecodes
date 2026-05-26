use std::sync::Arc;
use std::time::Instant;

use allthecodes_types::sdk::*;
use tracing::{debug, info};
use uuid::Uuid;

use crate::session::transcript;
use crate::types::config::QueryEngineConfig;
use crate::types::message::{
    Attachment, Message, MessageContent, QueryYield, StreamEvent, SystemSubtype,
};

use super::super::types::{AbortReason, UsageTrackingExt};
use super::super::QueryEngineState;
use super::{finish_submit_telemetry, SubmitTelemetrySpan, SubmitTurnState};

pub(super) enum StreamAction {
    Yield(SdkMessage),
    Terminate(SdkResult),
}

pub(super) struct StreamContext<'a> {
    pub(super) config: &'a QueryEngineConfig,
    pub(super) state_ref: &'a Arc<parking_lot::RwLock<QueryEngineState>>,
    pub(super) session_id: &'a crate::bootstrap::SessionId,
    pub(super) submit_turn: &'a mut SubmitTurnState,
    pub(super) replay_user_messages: bool,
    pub(super) submit_langfuse_trace: &'a mut Option<crate::services::langfuse::LangfuseTrace>,
    pub(super) telemetry_submit_span: &'a mut SubmitTelemetrySpan,
    pub(super) model_name: &'a str,
    pub(super) api_started_at: Instant,
}

pub(super) fn process_stream_item(
    item: QueryYield,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    match item {
        QueryYield::Message(Message::Assistant(assistant_msg)) => {
            handle_assistant_message(assistant_msg, ctx)
        }
        QueryYield::Message(Message::User(user_msg)) => handle_user_message(user_msg, ctx),
        QueryYield::Message(Message::Progress(progress_msg)) => {
            handle_progress_message(progress_msg, ctx)
        }
        QueryYield::Message(Message::System(system_msg)) => handle_system_message(system_msg, ctx),
        QueryYield::Message(Message::Attachment(attachment_msg)) => {
            handle_attachment_message(attachment_msg, ctx)
        }
        QueryYield::Stream(event) => handle_stream_event(event, ctx),
        QueryYield::RequestStart(_) => handle_request_start(),
        QueryYield::Tombstone(tombstone) => handle_tombstone(tombstone, ctx),
        QueryYield::ToolUseSummary(summary_msg) => handle_tool_use_summary(summary_msg, ctx),
    }
}

fn handle_assistant_message(
    assistant_msg: crate::types::message::AssistantMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    if let Some(ref sr) = assistant_msg.stop_reason {
        ctx.submit_turn.last_stop_reason = Some(sr.clone());
    }

    {
        let mut state = ctx.state_ref.write();
        state
            .messages
            .push(Message::Assistant(assistant_msg.clone()));
        if let Some(ref msg_usage) = assistant_msg.usage {
            state.usage.add_usage(msg_usage, assistant_msg.cost_usd);
        }
    }

    let action = StreamAction::Yield(SdkMessage::Assistant(SdkAssistantMessage {
        message: assistant_msg.clone(),
        session_id: ctx.session_id.to_string(),
        parent_tool_use_id: None,
    }));

    let _ = transcript::record_transcript(
        ctx.session_id.as_str(),
        &[Message::Assistant(assistant_msg.clone())],
    );

    if ctx.config.auto_save_session {
        let all_msgs = ctx.state_ref.read().messages.clone();
        let _ = crate::session::storage::save_session(
            ctx.session_id.as_str(),
            &all_msgs,
            &ctx.config.cwd,
        );
    }

    vec![action]
}

fn handle_user_message(
    user_msg: crate::types::message::UserMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    ctx.submit_turn.turn_count_this_submit += 1;

    {
        let mut state = ctx.state_ref.write();
        state.total_turn_count += 1;
        state.messages.push(Message::User(user_msg.clone()));
    }

    let mut actions = Vec::new();
    if ctx.replay_user_messages {
        let (content_text, content_blocks) = match &user_msg.content {
            MessageContent::Text(text) => (text.clone(), None),
            MessageContent::Blocks(blocks) => (
                format!("[{} content blocks]", blocks.len()),
                Some(blocks.clone()),
            ),
        };
        actions.push(StreamAction::Yield(SdkMessage::UserReplay(SdkUserReplay {
            content: content_text,
            session_id: ctx.session_id.to_string(),
            uuid: user_msg.uuid,
            timestamp: user_msg.timestamp,
            is_replay: true,
            is_synthetic: user_msg.is_meta,
            tool_use_result: user_msg.tool_use_result.clone(),
            source_tool_assistant_uuid: user_msg.source_tool_assistant_uuid,
            content_blocks,
        })));
    }

    let _ = transcript::record_transcript(ctx.session_id.as_str(), &[Message::User(user_msg)]);
    actions
}

fn handle_progress_message(
    progress_msg: crate::types::message::ProgressMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    ctx.state_ref
        .write()
        .messages
        .push(Message::Progress(progress_msg.clone()));

    let _ =
        transcript::record_transcript(ctx.session_id.as_str(), &[Message::Progress(progress_msg)]);
    Vec::new()
}

fn handle_system_message(
    system_msg: crate::types::message::SystemMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    match &system_msg.subtype {
        SystemSubtype::CompactBoundary { compact_metadata } => {
            ctx.state_ref
                .write()
                .messages
                .push(Message::System(system_msg.clone()));

            vec![StreamAction::Yield(SdkMessage::CompactBoundary(
                SdkCompactBoundary {
                    session_id: ctx.session_id.to_string(),
                    uuid: system_msg.uuid,
                    compact_metadata: compact_metadata.clone(),
                },
            ))]
        }
        SystemSubtype::ApiError {
            retry_attempt,
            max_retries,
            retry_in_ms,
            error,
        } => {
            ctx.state_ref
                .write()
                .messages
                .push(Message::System(system_msg.clone()));

            ctx.submit_turn.collected_errors.push(error.message.clone());

            vec![StreamAction::Yield(SdkMessage::ApiRetry(SdkApiRetry {
                attempt: *retry_attempt,
                max_retries: *max_retries,
                retry_delay_ms: *retry_in_ms,
                error_status: error.status,
                error: error.message.clone(),
                session_id: ctx.session_id.to_string(),
                uuid: system_msg.uuid,
            }))]
        }
        _ => {
            ctx.state_ref
                .write()
                .messages
                .push(Message::System(system_msg));
            Vec::new()
        }
    }
}

fn handle_attachment_message(
    attachment_msg: crate::types::message::AttachmentMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    ctx.state_ref
        .write()
        .messages
        .push(Message::Attachment(attachment_msg.clone()));

    match &attachment_msg.attachment {
        Attachment::MaxTurnsReached {
            max_turns,
            turn_count,
        } => {
            let result_text = format!("Reached maximum of {} turns", max_turns);
            let (usage_snap, denials_snap) = {
                let state = ctx.state_ref.read();
                (state.usage.clone(), state.permission_denials.clone())
            };
            crate::services::langfuse::end_trace(
                ctx.submit_langfuse_trace.take(),
                Some(&result_text),
                Some(crate::services::langfuse::TraceStatus::Error),
            );
            finish_submit_telemetry(ctx.telemetry_submit_span, ctx.model_name, &usage_snap);

            vec![StreamAction::Terminate(SdkResult {
                subtype: ResultSubtype::ErrorMaxTurns,
                is_error: true,
                duration_ms: ctx.submit_turn.duration_ms(),
                duration_api_ms: ctx.api_started_at.elapsed().as_millis() as u64,
                num_turns: *turn_count,
                result: result_text,
                stop_reason: ctx.submit_turn.last_stop_reason.clone(),
                session_id: ctx.session_id.to_string(),
                total_cost_usd: usage_snap.total_cost_usd,
                usage: usage_snap,
                permission_denials: denials_snap,
                structured_output: ctx.submit_turn.structured_output.clone(),
                uuid: Uuid::new_v4(),
                errors: ctx.submit_turn.collected_errors.clone(),
            })]
        }
        Attachment::StructuredOutput { data } => {
            ctx.submit_turn.structured_output = Some(data.clone());
            Vec::new()
        }
        Attachment::QueuedCommand {
            prompt: cmd_prompt,
            source_uuid,
        } => {
            let _ = source_uuid;
            if ctx.replay_user_messages {
                vec![StreamAction::Yield(SdkMessage::UserReplay(SdkUserReplay {
                    content: cmd_prompt.clone(),
                    session_id: ctx.session_id.to_string(),
                    uuid: attachment_msg.uuid,
                    timestamp: attachment_msg.timestamp,
                    is_replay: false,
                    is_synthetic: true,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                    content_blocks: None,
                }))]
            } else {
                Vec::new()
            }
        }
        Attachment::SkillDiscovery { skills } => {
            let mut state = ctx.state_ref.write();
            for skill in skills {
                state.discovered_skill_names.insert(skill.clone());
            }
            Vec::new()
        }
        Attachment::NestedMemory { path, .. } => {
            ctx.state_ref
                .write()
                .loaded_nested_memory_paths
                .insert(path.clone());
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_stream_event(event: StreamEvent, ctx: &mut StreamContext<'_>) -> Vec<StreamAction> {
    match &event {
        StreamEvent::MessageStart { usage: msg_usage } => {
            let _ = msg_usage;
        }
        StreamEvent::MessageDelta {
            delta,
            usage: _delta_usage,
        } => {
            if let Some(ref sr) = delta.stop_reason {
                ctx.submit_turn.last_stop_reason = Some(sr.clone());
            }
        }
        StreamEvent::MessageStop => {}
        _ => {}
    }

    vec![StreamAction::Yield(SdkMessage::StreamEvent(
        SdkStreamEvent {
            event,
            session_id: ctx.session_id.to_string(),
            uuid: Uuid::new_v4(),
        },
    ))]
}

fn handle_request_start() -> Vec<StreamAction> {
    debug!("request_start signal received");
    Vec::new()
}

fn handle_tombstone(
    tombstone: crate::types::message::TombstoneMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    debug!(
        assistant_uuid = %tombstone.message.uuid,
        "tombstone received (model fallback retry)"
    );
    {
        let mut state = ctx.state_ref.write();
        state.messages.retain(|message| {
            !matches!(
                message,
                Message::Assistant(assistant) if assistant.uuid == tombstone.message.uuid
            )
        });
    }

    let action = StreamAction::Yield(SdkMessage::Tombstone(SdkTombstone {
        message: tombstone.message.clone(),
        session_id: ctx.session_id.to_string(),
        uuid: Uuid::new_v4(),
    }));

    if ctx.config.auto_save_session {
        let all_msgs = ctx.state_ref.read().messages.clone();
        let _ = crate::session::storage::save_session(
            ctx.session_id.as_str(),
            &all_msgs,
            &ctx.config.cwd,
        );
    }

    vec![action]
}

fn handle_tool_use_summary(
    summary_msg: crate::types::message::ToolUseSummaryMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    vec![StreamAction::Yield(SdkMessage::ToolUseSummary(
        SdkToolUseSummary {
            summary: summary_msg.summary,
            preceding_tool_use_ids: summary_msg.preceding_tool_use_ids,
            session_id: ctx.session_id.to_string(),
            uuid: summary_msg.uuid,
        },
    ))]
}

pub(super) fn check_budget(ctx: &mut StreamContext<'_>) -> Option<SdkResult> {
    let max_budget = ctx.config.max_budget_usd?;
    let current_cost = ctx.state_ref.read().usage.total_cost_usd;
    if current_cost < max_budget {
        return None;
    }

    info!(
        spent = current_cost,
        limit = max_budget,
        "max budget exceeded"
    );

    ctx.state_ref.write().abort_reason = Some(AbortReason::MaxBudget {
        spent_usd: current_cost,
        limit_usd: max_budget,
    });

    let (usage_snap, denials_snap) = {
        let state = ctx.state_ref.read();
        (state.usage.clone(), state.permission_denials.clone())
    };
    let result_text = format!(
        "Stopped: cost ${:.4} exceeded budget ${:.4}",
        current_cost, max_budget
    );
    crate::services::langfuse::end_trace(
        ctx.submit_langfuse_trace.take(),
        Some(&result_text),
        Some(crate::services::langfuse::TraceStatus::Error),
    );
    finish_submit_telemetry(ctx.telemetry_submit_span, ctx.model_name, &usage_snap);

    Some(SdkResult {
        subtype: ResultSubtype::ErrorMaxBudgetUsd,
        is_error: true,
        duration_ms: ctx.submit_turn.duration_ms(),
        duration_api_ms: ctx.api_started_at.elapsed().as_millis() as u64,
        num_turns: ctx.submit_turn.turn_count_this_submit,
        result: result_text,
        stop_reason: ctx.submit_turn.last_stop_reason.clone(),
        session_id: ctx.session_id.to_string(),
        total_cost_usd: current_cost,
        usage: usage_snap,
        permission_denials: denials_snap,
        structured_output: ctx.submit_turn.structured_output.clone(),
        uuid: Uuid::new_v4(),
        errors: ctx.submit_turn.collected_errors.clone(),
    })
}
