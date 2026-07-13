use std::sync::Arc;
use std::time::Instant;

use allthecodes_types::sdk::*;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::session::record_replay::types::{
    CompactionBoundaryRecord, CompactionKind, MessageRecord, QueryEventRecord, RecordItem,
};
use crate::types::config::QueryEngineConfig;
use crate::types::message::{
    Attachment, ContentBlock, Message, MessageContent, QueryYield, RequestStartEvent, StreamEvent,
    SystemSubtype, ToolResultContent, Usage,
};

use super::super::types::AbortReason;
use super::super::QueryEngineState;
use super::transaction::SubmitTransaction;
use super::{finish_submit_telemetry, SubmitTelemetrySpan, SubmitTurnState};

pub(super) enum QueryTurnEvent {
    Stream(StreamEvent),
    RequestStart(RequestStartEvent),
    Message(Message),
    Tombstone(crate::types::message::TombstoneMessage),
    ToolUseSummary(crate::types::message::ToolUseSummaryMessage),
    BriefMessage(allthecodes_types::brief::BriefMessagePayload),
}

impl From<QueryYield> for QueryTurnEvent {
    fn from(item: QueryYield) -> Self {
        match item {
            QueryYield::Stream(event) => Self::Stream(event),
            QueryYield::RequestStart(request_start) => Self::RequestStart(request_start),
            QueryYield::Message(message) => Self::Message(message),
            QueryYield::Tombstone(tombstone) => Self::Tombstone(tombstone),
            QueryYield::ToolUseSummary(summary) => Self::ToolUseSummary(summary),
            QueryYield::BriefMessage(payload) => Self::BriefMessage(payload),
        }
    }
}

impl QueryTurnEvent {
    pub(super) fn record_items(&self, backend_name: &str, model_name: &str) -> Vec<RecordItem> {
        match self {
            Self::Message(message) => {
                let mut items = vec![RecordItem::Message(MessageRecord::from_message(message))];
                if let Message::System(system) = message {
                    match &system.subtype {
                        SystemSubtype::CompactBoundary { compact_metadata } => {
                            items.push(RecordItem::CompactionBoundary(CompactionBoundaryRecord {
                                kind: CompactionKind::Compact,
                                summary_message_uuid: Some(system.uuid.to_string()),
                                metadata: compact_metadata
                                    .as_ref()
                                    .and_then(|metadata| serde_json::to_value(metadata).ok()),
                            }))
                        }
                        SystemSubtype::MicrocompactBoundary {
                            microcompact_metadata,
                        } => items.push(RecordItem::CompactionBoundary(CompactionBoundaryRecord {
                            kind: CompactionKind::Microcompact,
                            summary_message_uuid: Some(system.uuid.to_string()),
                            metadata: microcompact_metadata
                                .as_ref()
                                .and_then(|metadata| serde_json::to_value(metadata).ok()),
                        })),
                        _ => {}
                    }
                }
                items
            }
            Self::RequestStart(request_start) => {
                vec![RecordItem::QueryEvent(QueryEventRecord::RequestStart {
                    provider: request_start
                        .provider
                        .clone()
                        .or_else(|| Some(backend_name.to_string())),
                    model: request_start
                        .model
                        .clone()
                        .or_else(|| Some(model_name.to_string())),
                })]
            }
            _ => Vec::new(),
        }
    }
}

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
    pub(super) backend_name: &'a str,
    pub(super) request_event: Option<&'a crate::types::message::RequestStartEvent>,
    pub(super) api_started_at: Instant,
}

pub(super) struct BudgetStop {
    pub(super) goal_update: Option<SdkMessage>,
    pub(super) result: SdkResult,
}

pub(super) fn process_stream_item(
    item: QueryTurnEvent,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    match item {
        QueryTurnEvent::Message(Message::Assistant(assistant_msg)) => {
            handle_assistant_message(assistant_msg, ctx)
        }
        QueryTurnEvent::Message(Message::User(user_msg)) => handle_user_message(user_msg, ctx),
        QueryTurnEvent::Message(Message::Progress(progress_msg)) => {
            handle_progress_message(progress_msg, ctx)
        }
        QueryTurnEvent::Message(Message::System(system_msg)) => {
            handle_system_message(system_msg, ctx)
        }
        QueryTurnEvent::Message(Message::Attachment(attachment_msg)) => {
            handle_attachment_message(attachment_msg, ctx)
        }
        QueryTurnEvent::Stream(event) => handle_stream_event(event, ctx),
        QueryTurnEvent::RequestStart(_) => handle_request_start(),
        QueryTurnEvent::Tombstone(tombstone) => handle_tombstone(tombstone, ctx),
        QueryTurnEvent::ToolUseSummary(summary_msg) => handle_tool_use_summary(summary_msg, ctx),
        QueryTurnEvent::BriefMessage(payload) => handle_brief_message(payload),
    }
}

fn handle_assistant_message(
    assistant_msg: crate::types::message::AssistantMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    if let Some(ref sr) = assistant_msg.stop_reason {
        ctx.submit_turn.last_stop_reason = Some(sr.clone());
    }

    let mut transaction = SubmitTransaction::new();
    transaction.append_message(Message::Assistant(assistant_msg.clone()));
    if let Some(ref msg_usage) = assistant_msg.usage {
        transaction.record_usage_cost(msg_usage.clone(), assistant_msg.cost_usd);
    }

    // Emit a durable cost event for completed model API calls.
    if let Some(ref usage) = assistant_msg.usage {
        use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
        let mut audit_ctx = ctx.state_ref.read().runtime.audit_ctx.clone();
        if let Some(request_event) = ctx.request_event {
            audit_ctx.submit_id = request_event.submit_id.clone();
            audit_ctx.turn_id = request_event.turn_id.clone();
            audit_ctx.request_id = request_event.request_id.clone();
        }
        audit_ctx.message_id = Some(assistant_msg.uuid.to_string());

        let cost_usd = assistant_msg.cost_usd;
        let model = ctx
            .request_event
            .and_then(|event| event.model.clone())
            .unwrap_or_else(|| ctx.model_name.to_string());
        let provider = ctx.request_event.and_then(|event| event.provider.clone());
        let backend = ctx
            .request_event
            .and_then(|event| event.backend.clone())
            .unwrap_or_else(|| ctx.backend_name.to_string());
        let attempt = ctx
            .request_event
            .map(|event| event.attempt.max(1))
            .unwrap_or(1);
        let is_retry = ctx
            .request_event
            .map(|event| event.is_retry)
            .unwrap_or(false);
        let pricing_match = allthecodes_types::models::get_pricing_match(&model);

        let data = serde_json::json!({
            "schema_version": 1,
            "session_id": ctx.session_id.to_string(),
            "submit_id": audit_ctx.submit_id.clone(),
            "turn_id": audit_ctx.turn_id.clone(),
            "request_id": audit_ctx.request_id.clone(),
            "message_id": assistant_msg.uuid.to_string(),
            "provider": provider,
            "backend": backend,
            "model": model,
            "pricing": {
                "source": pricing_match.source,
                "matched_key": pricing_match.matched_key,
                "currency": "USD",
                "input_per_1m": pricing_match.pricing.input_per_1m,
                "output_per_1m": pricing_match.pricing.output_per_1m,
                "cache_read_multiplier": 0.1,
                "cache_creation_multiplier": 1.25,
            },
            "usage": {
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "reasoning_output_tokens": usage.reasoning_output_tokens,
                "cache_read_input_tokens": usage.cache_read_input_tokens,
                "cache_creation_input_tokens": usage.cache_creation_input_tokens,
            },
            "cost_usd": cost_usd,
            "stop_reason": assistant_msg.stop_reason.clone(),
            "is_retry": is_retry,
            "attempt": attempt,
            "backfilled": false,
        });

        audit_ctx.emit(
            EventKind::CostRecorded,
            Stage::Cost,
            AuditLevel::Info,
            Outcome::Completed,
            None,
            Some(data),
        );
        audit_ctx.flush();
    }

    transaction.emit(SdkMessage::Assistant(SdkAssistantMessage {
        message: assistant_msg.clone(),
        session_id: ctx.session_id.to_string(),
        parent_tool_use_id: None,
    }));

    transaction.persist(Message::Assistant(assistant_msg.clone()));
    transaction.save_session_after_commit();

    let mut actions = transaction
        .commit(ctx.state_ref, ctx.session_id, ctx.config)
        .into_sdk_messages()
        .into_iter()
        .map(StreamAction::Yield)
        .collect::<Vec<_>>();
    let token_delta = assistant_msg.usage.as_ref().map(assistant_usage_tokens);
    if let Some(goal_update) =
        account_goal_runtime_message(ctx.session_id.as_str(), ctx.state_ref, token_delta)
    {
        actions.push(StreamAction::Yield(goal_update));
    }
    actions
}

pub(super) fn account_goal_runtime_message(
    session_id: &str,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    explicit_token_delta: Option<u64>,
) -> Option<SdkMessage> {
    let goal = match allthecodes_tools::goals::load_goal_for_session(session_id) {
        Ok(Some(goal)) => goal,
        Ok(None) => {
            state_ref.write().runtime.goal_runtime.clear_active();
            return None;
        }
        Err(error) => {
            warn!(%error, "failed to load goal for runtime accounting");
            return None;
        }
    };

    if !allthecodes_tools::goals::goal_is_active(&goal) {
        state_ref
            .write()
            .runtime
            .goal_runtime
            .clear_for_goal(&goal.goal_id);
        return None;
    }

    let now = Instant::now();
    let (token_delta, seconds_delta) = {
        let mut state = state_ref.write();
        let usage = state.transcript.usage.clone();
        if let Some(token_delta) = explicit_token_delta {
            state.runtime.goal_runtime.account_explicit_token_delta(
                &goal.goal_id,
                &usage,
                token_delta,
                now,
            )
        } else {
            let (token_delta, seconds_delta) =
                state
                    .runtime
                    .goal_runtime
                    .account_delta(&goal.goal_id, &usage, now)?;
            (token_delta, seconds_delta)
        }
    };

    if token_delta == 0 && seconds_delta == 0 {
        return None;
    }

    match allthecodes_tools::goals::account_goal_runtime_delta_for_session(
        session_id,
        &goal.goal_id,
        token_delta,
        seconds_delta,
        chrono::Utc::now(),
    ) {
        Ok(Some(goal)) => goal_runtime_update_message(session_id, state_ref, goal),
        Ok(None) => {
            state_ref.write().runtime.goal_runtime.clear_active();
            None
        }
        Err(error) => {
            warn!(%error, "failed to update goal runtime accounting");
            None
        }
    }
}

fn assistant_usage_tokens(usage: &Usage) -> u64 {
    usage
        .input_tokens
        .saturating_add(usage.output_tokens)
        .saturating_add(usage.cache_read_input_tokens)
        .saturating_add(usage.cache_creation_input_tokens)
}

fn goal_runtime_update_message(
    session_id: &str,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    goal: allthecodes_tools::goals::GoalRecord,
) -> Option<SdkMessage> {
    use allthecodes_tools::goals::GoalStatus;

    match goal.status {
        GoalStatus::BudgetLimited => {
            let mut state = state_ref.write();
            state.runtime.goal_runtime.clear_for_goal(&goal.goal_id);
            if state
                .runtime
                .goal_runtime
                .budget_warning_already_sent(&goal.goal_id)
            {
                None
            } else {
                state
                    .runtime
                    .goal_runtime
                    .mark_budget_warning_sent(&goal.goal_id);
                Some(goal_updated_message(session_id, "budget_limited", goal))
            }
        }
        GoalStatus::Active => Some(goal_updated_message(session_id, "runtime_updated", goal)),
        _ => {
            state_ref
                .write()
                .runtime
                .goal_runtime
                .clear_for_goal(&goal.goal_id);
            Some(goal_updated_message(
                session_id,
                goal_status_event(&goal.status),
                goal,
            ))
        }
    }
}

pub(super) fn prime_goal_runtime_for_session(
    session_id: &str,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
) {
    match allthecodes_tools::goals::load_goal_for_session(session_id) {
        Ok(Some(goal)) if allthecodes_tools::goals::goal_is_active(&goal) => {
            let usage = state_ref.read().transcript.usage.clone();
            state_ref.write().runtime.goal_runtime.prime_active_goal(
                &goal.goal_id,
                &usage,
                Instant::now(),
            );
        }
        Ok(Some(goal)) => {
            state_ref
                .write()
                .runtime
                .goal_runtime
                .clear_for_goal(&goal.goal_id);
        }
        Ok(None) => {
            state_ref.write().runtime.goal_runtime.clear_active();
        }
        Err(error) => {
            warn!(%error, "failed to prime goal runtime accounting");
        }
    }
}

pub(super) fn maybe_stage_background_review_after_turn(
    config: &QueryEngineConfig,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    session_id: &crate::bootstrap::SessionId,
    turn_count_this_submit: usize,
    result_text: &str,
    is_error: bool,
) -> Vec<String> {
    let review_config = crate::services::background_review::BackgroundReviewConfig::from_env();
    let settings = state_ref.read().app_state.settings.clone();
    if !background_review_enabled_by_hermes(&settings, &review_config) {
        return Vec::new();
    }

    let (observed_turn_count, recent_summary) = {
        let state = state_ref.read();
        let user_message_count = state
            .transcript
            .messages
            .iter()
            .filter(|message| matches!(message, Message::User(_)))
            .count();
        let observed_turn_count = state
            .transcript
            .total_turn_count
            .max(turn_count_this_submit)
            .max(user_message_count);
        (
            observed_turn_count,
            build_review_summary(&state.transcript.messages, result_text, is_error),
        )
    };

    let similar_session_hits = if observed_turn_count >= review_config.turn_threshold {
        similar_session_ids(&config.cwd, &recent_summary, session_id.as_str())
    } else {
        Vec::new()
    };

    let input = crate::services::background_review::BackgroundReviewInput {
        source_session_id: session_id.to_string(),
        cwd: config.cwd.clone(),
        turn_count: observed_turn_count,
        replay_seq_start: None,
        replay_seq_end: None,
        recent_summary,
        tool_errors: Vec::new(),
        similar_session_hits,
    };

    match crate::services::background_review::stage_background_review_if_due(input, &review_config)
    {
        Ok(Some(proposal)) => vec![proposal.id],
        Ok(None) => Vec::new(),
        Err(error) => {
            warn!(session_id = %session_id, %error, "failed to stage background review proposal");
            Vec::new()
        }
    }
}

fn background_review_enabled_by_hermes(
    settings: &crate::types::app_state::SettingsJson,
    config: &crate::services::background_review::BackgroundReviewConfig,
) -> bool {
    config.enabled && settings.hermes_enabled.unwrap_or(false)
}

fn similar_session_ids(cwd: &str, summary: &str, current_session_id: &str) -> Vec<String> {
    let query = summary
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    if query.trim().is_empty() {
        return Vec::new();
    }
    allthecodes_session::storage::search_workspace_sessions(std::path::Path::new(cwd), &query, 3)
        .map(|hits| {
            hits.into_iter()
                .map(|hit| hit.session_id)
                .filter(|id| id != current_session_id)
                .collect()
        })
        .unwrap_or_default()
}

fn build_review_summary(messages: &[Message], result_text: &str, is_error: bool) -> String {
    let mut lines = messages
        .iter()
        .rev()
        .take(6)
        .map(message_preview)
        .collect::<Vec<_>>();
    lines.reverse();
    if !result_text.trim().is_empty() {
        lines.push(format!(
            "result{}: {}",
            if is_error { " error" } else { "" },
            truncate_chars(result_text, 180)
        ));
    }
    truncate_chars(&lines.join("\n"), 500)
}

fn message_preview(message: &Message) -> String {
    match message {
        Message::User(user) => format!("user: {}", content_preview(&user.content)),
        Message::Assistant(assistant) => {
            format!("assistant: {}", blocks_preview(&assistant.content))
        }
        Message::System(system) => format!("system: {}", truncate_chars(&system.content, 120)),
        Message::Progress(progress) => {
            format!(
                "progress: {} {}",
                progress.tool_use_id,
                truncate_chars(&progress.data.to_string(), 80)
            )
        }
        Message::Attachment(attachment) => {
            format!(
                "attachment: {}",
                truncate_chars(&format!("{:?}", attachment.attachment), 120)
            )
        }
    }
}

fn content_preview(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => truncate_chars(text, 160),
        MessageContent::Blocks(blocks) => blocks_preview(blocks),
    }
}

fn blocks_preview(blocks: &[ContentBlock]) -> String {
    truncate_chars(
        &blocks
            .iter()
            .map(block_preview)
            .collect::<Vec<_>>()
            .join(" "),
        180,
    )
}

fn block_preview(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => truncate_chars(text, 120),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => {
            format!("tool_use {name} {}", truncate_chars(&input.to_string(), 80))
        }
        ContentBlock::ToolResult {
            content, is_error, ..
        } => {
            let prefix = if *is_error {
                "tool_result_error"
            } else {
                "tool_result"
            };
            format!("{prefix} {}", tool_result_preview(content))
        }
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
            "[thinking]".to_string()
        }
        ContentBlock::ConnectorText { connector_text, .. } => truncate_chars(connector_text, 120),
        ContentBlock::Image { .. } => "[image]".to_string(),
    }
}

fn tool_result_preview(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => truncate_chars(text, 120),
        ToolResultContent::Blocks(blocks) => blocks_preview(blocks),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn goal_status_event(status: &allthecodes_tools::goals::GoalStatus) -> &'static str {
    match status {
        allthecodes_tools::goals::GoalStatus::Active => "runtime_updated",
        allthecodes_tools::goals::GoalStatus::Paused => "paused",
        allthecodes_tools::goals::GoalStatus::Complete => "complete",
        allthecodes_tools::goals::GoalStatus::Blocked => "blocked",
        allthecodes_tools::goals::GoalStatus::UsageLimited => "usage_limited",
        allthecodes_tools::goals::GoalStatus::BudgetLimited => "budget_limited",
    }
}

pub(super) fn goal_updated_message(
    session_id: &str,
    event: impl Into<String>,
    goal: allthecodes_tools::goals::GoalRecord,
) -> SdkMessage {
    SdkMessage::GoalUpdated(SdkGoalUpdated {
        event: event.into(),
        goal: serde_json::to_value(goal).unwrap_or(serde_json::Value::Null),
        session_id: session_id.to_string(),
        uuid: Uuid::new_v4(),
    })
}

fn handle_user_message(
    user_msg: crate::types::message::UserMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    ctx.submit_turn.turn_count_this_submit += 1;

    let mut transaction = SubmitTransaction::new();
    transaction.increment_turn_count();
    transaction.append_message(Message::User(user_msg.clone()));
    transaction.persist(Message::User(user_msg.clone()));
    if ctx.replay_user_messages {
        let (content_text, content_blocks) = match &user_msg.content {
            MessageContent::Text(text) => (text.clone(), None),
            MessageContent::Blocks(blocks) => (
                format!("[{} content blocks]", blocks.len()),
                Some(blocks.clone()),
            ),
        };
        transaction.emit(SdkMessage::UserReplay(SdkUserReplay {
            content: content_text,
            session_id: ctx.session_id.to_string(),
            uuid: user_msg.uuid,
            timestamp: user_msg.timestamp,
            is_replay: true,
            is_synthetic: user_msg.is_meta,
            tool_use_result: user_msg.tool_use_result.clone(),
            source_tool_assistant_uuid: user_msg.source_tool_assistant_uuid,
            content_blocks,
        }));
    }

    transaction
        .commit(ctx.state_ref, ctx.session_id, ctx.config)
        .into_sdk_messages()
        .into_iter()
        .map(StreamAction::Yield)
        .collect()
}

fn handle_progress_message(
    progress_msg: crate::types::message::ProgressMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    let mut transaction = SubmitTransaction::new();
    transaction.append_message(Message::Progress(progress_msg.clone()));
    transaction.persist(Message::Progress(progress_msg));
    transaction
        .commit(ctx.state_ref, ctx.session_id, ctx.config)
        .into_sdk_messages()
        .into_iter()
        .map(StreamAction::Yield)
        .collect()
}

fn handle_system_message(
    system_msg: crate::types::message::SystemMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    match &system_msg.subtype {
        SystemSubtype::CompactBoundary { compact_metadata } => {
            let mut transaction = SubmitTransaction::new();
            transaction.append_message(Message::System(system_msg.clone()));
            let internal_metadata_hidden = compact_metadata
                .as_ref()
                .is_some_and(|metadata| metadata.has_internal_metadata());
            let public_compact_metadata = compact_metadata
                .as_ref()
                .map(|metadata| metadata.public_copy());

            transaction.emit(SdkMessage::CompactBoundary(SdkCompactBoundary {
                session_id: ctx.session_id.to_string(),
                uuid: system_msg.uuid,
                compact_metadata: public_compact_metadata,
                internal_metadata_hidden,
            }));
            transaction
                .commit(ctx.state_ref, ctx.session_id, ctx.config)
                .into_sdk_messages()
                .into_iter()
                .map(StreamAction::Yield)
                .collect()
        }
        SystemSubtype::ApiError {
            retry_attempt,
            max_retries,
            retry_in_ms,
            error,
        } => {
            let mut transaction = SubmitTransaction::new();
            transaction.append_message(Message::System(system_msg.clone()));

            ctx.submit_turn.collected_errors.push(error.message.clone());

            transaction.emit(SdkMessage::ApiRetry(SdkApiRetry {
                attempt: *retry_attempt,
                max_retries: *max_retries,
                retry_delay_ms: *retry_in_ms,
                error_status: error.status,
                error: error.message.clone(),
                session_id: ctx.session_id.to_string(),
                uuid: system_msg.uuid,
            }));
            transaction
                .commit(ctx.state_ref, ctx.session_id, ctx.config)
                .into_sdk_messages()
                .into_iter()
                .map(StreamAction::Yield)
                .collect()
        }
        _ => {
            let mut transaction = SubmitTransaction::new();
            transaction.append_message(Message::System(system_msg));
            transaction
                .commit(ctx.state_ref, ctx.session_id, ctx.config)
                .into_sdk_messages()
                .into_iter()
                .map(StreamAction::Yield)
                .collect()
        }
    }
}

fn handle_attachment_message(
    attachment_msg: crate::types::message::AttachmentMessage,
    ctx: &mut StreamContext<'_>,
) -> Vec<StreamAction> {
    let mut transaction = SubmitTransaction::new();
    transaction.append_message(Message::Attachment(attachment_msg.clone()));
    let appended_events = transaction
        .commit(ctx.state_ref, ctx.session_id, ctx.config)
        .into_sdk_messages()
        .into_iter()
        .map(StreamAction::Yield)
        .collect::<Vec<_>>();

    match &attachment_msg.attachment {
        Attachment::MaxTurnsReached {
            max_turns,
            turn_count,
        } => {
            let result_text = format!("Reached maximum of {} turns", max_turns);
            let (usage_snap, denials_snap) = {
                let state = ctx.state_ref.read();
                (
                    state.transcript.usage.clone(),
                    state.permissions.denials.clone(),
                )
            };
            crate::services::langfuse::end_trace(
                ctx.submit_langfuse_trace.take(),
                Some(&result_text),
                Some(crate::services::langfuse::TraceStatus::Error),
            );
            finish_submit_telemetry(ctx.telemetry_submit_span, ctx.model_name, &usage_snap);

            let mut actions = appended_events;
            actions.push(StreamAction::Terminate(SdkResult {
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
            }));
            actions
        }
        Attachment::StructuredOutput { data } => {
            ctx.submit_turn.structured_output = Some(data.clone());
            appended_events
        }
        Attachment::QueuedCommand {
            prompt: cmd_prompt,
            source_uuid,
        } => {
            let _ = source_uuid;
            if ctx.replay_user_messages {
                let mut actions = appended_events;
                actions.push(StreamAction::Yield(SdkMessage::UserReplay(SdkUserReplay {
                    content: cmd_prompt.clone(),
                    session_id: ctx.session_id.to_string(),
                    uuid: attachment_msg.uuid,
                    timestamp: attachment_msg.timestamp,
                    is_replay: false,
                    is_synthetic: true,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                    content_blocks: None,
                })));
                actions
            } else {
                appended_events
            }
        }
        Attachment::SkillDiscovery { skills } => {
            let mut state = ctx.state_ref.write();
            for skill in skills {
                state.tools.discovered_skill_names.insert(skill.clone());
            }
            appended_events
        }
        Attachment::NestedMemory { path, .. } => {
            ctx.state_ref
                .write()
                .tools
                .loaded_nested_memory_paths
                .insert(path.clone());
            appended_events
        }
        _ => appended_events,
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
    let mut transaction = SubmitTransaction::new();
    transaction.remove_assistant_message(tombstone.message.uuid);
    transaction.emit(SdkMessage::Tombstone(SdkTombstone {
        message: tombstone.message.clone(),
        session_id: ctx.session_id.to_string(),
        uuid: Uuid::new_v4(),
    }));
    transaction.save_session_after_commit();

    transaction
        .commit(ctx.state_ref, ctx.session_id, ctx.config)
        .into_sdk_messages()
        .into_iter()
        .map(StreamAction::Yield)
        .collect()
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

fn handle_brief_message(
    payload: allthecodes_types::brief::BriefMessagePayload,
) -> Vec<StreamAction> {
    vec![StreamAction::Yield(SdkMessage::BriefMessage(payload))]
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::lifecycle::QueryEngine;
    use crate::types::config::QueryEngineConfig;
    use crate::types::message::{
        AssistantMessage, CompactMetadata, ContentBlock, SystemMessage, Usage,
    };

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
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

    fn make_config() -> QueryEngineConfig {
        QueryEngineConfig {
            cwd: "/tmp".to_string(),
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
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }
    }

    #[test]
    fn background_review_gate_requires_hermes_enabled_setting() {
        let enabled_config = crate::services::background_review::BackgroundReviewConfig {
            enabled: true,
            turn_threshold: 1,
        };
        let mut settings = crate::types::app_state::SettingsJson::default();

        assert!(!background_review_enabled_by_hermes(
            &settings,
            &enabled_config
        ));

        settings.hermes_enabled = Some(false);
        assert!(!background_review_enabled_by_hermes(
            &settings,
            &enabled_config
        ));

        settings.hermes_enabled = Some(true);
        assert!(background_review_enabled_by_hermes(
            &settings,
            &enabled_config
        ));

        let disabled_config = crate::services::background_review::BackgroundReviewConfig {
            enabled: false,
            turn_threshold: 1,
        };
        assert!(!background_review_enabled_by_hermes(
            &settings,
            &disabled_config
        ));
    }

    #[test]
    #[serial_test::serial]
    fn assistant_usage_updates_active_goal_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let engine = QueryEngine::new(make_config());
        let session_id = engine.session_id.clone();
        let goal = allthecodes_tools::goals::create_goal_record(
            "stay within budget",
            Some(10),
            chrono::Utc::now(),
        )
        .unwrap();
        allthecodes_tools::goals::save_goal_for_session(session_id.as_str(), &goal).unwrap();

        let assistant = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            usage: Some(Usage {
                input_tokens: 7,
                output_tokens: 5,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                reasoning_output_tokens: 0,
            }),
            stop_reason: Some("end_turn".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let mut submit_turn = SubmitTurnState::new();
        let mut submit_langfuse_trace = None;
        let mut telemetry_submit_span = None;
        let mut ctx = StreamContext {
            config: &engine.config,
            state_ref: &engine.state,
            session_id: &session_id,
            submit_turn: &mut submit_turn,
            replay_user_messages: false,
            submit_langfuse_trace: &mut submit_langfuse_trace,
            telemetry_submit_span: &mut telemetry_submit_span,
            model_name: "test-model",
            backend_name: "test-backend",
            request_event: None,
            api_started_at: Instant::now(),
        };

        let actions = handle_assistant_message(assistant, &mut ctx);
        assert!(actions.iter().any(|action| matches!(
            action,
            StreamAction::Yield(SdkMessage::GoalUpdated(update))
                if update.event == "budget_limited"
        )));
        let goal = allthecodes_tools::goals::load_goal_for_session(session_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(goal.tokens_used, 12);
        assert_eq!(
            goal.status,
            allthecodes_tools::goals::GoalStatus::BudgetLimited
        );
    }

    #[test]
    #[serial_test::serial]
    fn cost_budget_stop_marks_goal_usage_limited() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let mut config = make_config();
        config.max_budget_usd = Some(1.0);
        let engine = QueryEngine::new(config);
        let session_id = engine.session_id.clone();
        let goal = allthecodes_tools::goals::create_goal_record(
            "stay within cost limit",
            None,
            chrono::Utc::now(),
        )
        .unwrap();
        let goal_id = goal.goal_id.clone();
        allthecodes_tools::goals::save_goal_for_session(session_id.as_str(), &goal).unwrap();
        {
            let mut state = engine.state.write();
            state.transcript.usage.total_cost_usd = 2.0;
            state.runtime.goal_runtime.active_goal_id = Some(goal_id);
        }

        let mut submit_turn = SubmitTurnState::new();
        let mut submit_langfuse_trace = None;
        let mut telemetry_submit_span = None;
        let mut ctx = StreamContext {
            config: &engine.config,
            state_ref: &engine.state,
            session_id: &session_id,
            submit_turn: &mut submit_turn,
            replay_user_messages: false,
            submit_langfuse_trace: &mut submit_langfuse_trace,
            telemetry_submit_span: &mut telemetry_submit_span,
            model_name: "test-model",
            backend_name: "test-backend",
            request_event: None,
            api_started_at: Instant::now(),
        };

        let stop = check_budget(&mut ctx).expect("budget stop");
        let Some(SdkMessage::GoalUpdated(update)) = stop.goal_update else {
            panic!("expected usage_limited goal update");
        };
        assert_eq!(update.event, "usage_limited");
        let goal = allthecodes_tools::goals::load_goal_for_session(session_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            goal.status,
            allthecodes_tools::goals::GoalStatus::UsageLimited
        );
    }

    #[test]
    fn compact_boundary_sdk_event_hides_internal_metadata() {
        let engine = QueryEngine::new(make_config());
        let session_id = engine.session_id.clone();
        let system = SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            subtype: SystemSubtype::CompactBoundary {
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 100,
                    post_compact_token_count: 40,
                    preserved_segment: None,
                    pre_compact_discovered_tools: Some(vec!["VaultHttpFetch".to_string()]),
                }),
            },
            content: "compacted".to_string(),
        };
        let mut submit_turn = SubmitTurnState::new();
        let mut submit_langfuse_trace = None;
        let mut telemetry_submit_span = None;
        let mut ctx = StreamContext {
            config: &engine.config,
            state_ref: &engine.state,
            session_id: &session_id,
            submit_turn: &mut submit_turn,
            replay_user_messages: false,
            submit_langfuse_trace: &mut submit_langfuse_trace,
            telemetry_submit_span: &mut telemetry_submit_span,
            model_name: "test-model",
            backend_name: "test-backend",
            request_event: None,
            api_started_at: Instant::now(),
        };

        let actions = handle_system_message(system, &mut ctx);
        let StreamAction::Yield(SdkMessage::CompactBoundary(boundary)) = &actions[0] else {
            panic!("expected compact boundary SDK message");
        };
        assert!(boundary.internal_metadata_hidden);
        let public = boundary
            .compact_metadata
            .as_ref()
            .expect("public compact metadata");
        assert_eq!(public.pre_compact_token_count, 100);
        assert_eq!(public.post_compact_token_count, 40);
        assert!(public.pre_compact_discovered_tools.is_none());

        let stored = engine.state.read().transcript.messages.clone();
        let Some(Message::System(stored_system)) = stored.first() else {
            panic!("expected stored system message");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(stored_metadata),
        } = &stored_system.subtype
        else {
            panic!("expected stored compact metadata");
        };
        assert_eq!(
            stored_metadata.pre_compact_discovered_tools.as_deref(),
            Some(&["VaultHttpFetch".to_string()][..])
        );
    }

    #[test]
    fn submit_transaction_query_yield_adapter_preserves_request_start_payload() {
        let event = QueryTurnEvent::from(QueryYield::RequestStart(
            crate::types::message::RequestStartEvent {
                submit_id: Some("submit-1".to_string()),
                turn_id: Some("turn-1".to_string()),
                request_id: Some("request-1".to_string()),
                provider: Some("anthropic".to_string()),
                backend: Some("native".to_string()),
                model: Some("claude-test".to_string()),
                attempt: 2,
                is_retry: true,
            },
        ));
        let QueryTurnEvent::RequestStart(request_event) = event else {
            panic!("expected typed request start event");
        };
        assert_eq!(request_event.submit_id.as_deref(), Some("submit-1"));
        assert_eq!(request_event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(request_event.request_id.as_deref(), Some("request-1"));
        assert_eq!(request_event.provider.as_deref(), Some("anthropic"));
        assert_eq!(request_event.backend.as_deref(), Some("native"));
        assert_eq!(request_event.model.as_deref(), Some("claude-test"));
        assert_eq!(request_event.attempt, 2);
        assert!(request_event.is_retry);
    }

    #[test]
    fn process_stream_item_forwards_brief_messages_without_transcript_persist() {
        let engine = QueryEngine::new(make_config());
        let session_id = engine.session_id.clone();
        let mut submit_turn = SubmitTurnState::new();
        let mut submit_langfuse_trace = None;
        let mut telemetry_submit_span = None;
        let mut ctx = StreamContext {
            config: &engine.config,
            state_ref: &engine.state,
            session_id: &session_id,
            submit_turn: &mut submit_turn,
            replay_user_messages: false,
            submit_langfuse_trace: &mut submit_langfuse_trace,
            telemetry_submit_span: &mut telemetry_submit_span,
            model_name: "test-model",
            backend_name: "test-backend",
            request_event: None,
            api_started_at: Instant::now(),
        };
        let payload = allthecodes_types::brief::BriefMessagePayload {
            message: "brief body".to_string(),
            status: allthecodes_types::brief::BriefMessageStatus::Normal,
            attachments: vec![],
            level: None,
            source_tool_name: Some(allthecodes_types::brief::BRIEF_TOOL_NAME.to_string()),
            tool_use_id: Some("toolu-brief".to_string()),
            session_id: Some(session_id.to_string()),
            timestamp: Some(100),
        };

        let actions = process_stream_item(
            QueryTurnEvent::from(QueryYield::BriefMessage(payload.clone())),
            &mut ctx,
        );

        let [StreamAction::Yield(SdkMessage::BriefMessage(forwarded))] = actions.as_slice() else {
            panic!("expected single brief message SDK action");
        };
        assert_eq!(forwarded, &payload);
        assert!(engine.state.read().transcript.messages.is_empty());
    }

    #[test]
    fn submit_transaction_query_turn_event_records_request_start_and_messages() {
        let system = SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 1,
            subtype: SystemSubtype::Informational {
                level: crate::types::message::InfoLevel::Info,
            },
            content: "notice".to_string(),
        };
        let event = QueryTurnEvent::from(QueryYield::Message(Message::System(system)));
        let records = event.record_items("native", "claude-test");
        assert!(matches!(
            records.as_slice(),
            [crate::session::record_replay::types::RecordItem::Message(_)]
        ));

        let request_start = QueryTurnEvent::from(QueryYield::RequestStart(
            crate::types::message::RequestStartEvent {
                provider: Some("anthropic".to_string()),
                model: Some("claude-test".to_string()),
                ..Default::default()
            },
        ));
        let records = request_start.record_items("native", "fallback-model");
        assert!(matches!(
            records.as_slice(),
            [crate::session::record_replay::types::RecordItem::QueryEvent(
                crate::session::record_replay::types::QueryEventRecord::RequestStart {
                    provider: Some(provider),
                    model: Some(model),
                },
            )] if provider == "anthropic" && model == "claude-test"
        ));
    }
}

pub(super) fn check_budget(ctx: &mut StreamContext<'_>) -> Option<BudgetStop> {
    let max_budget = ctx.config.max_budget_usd?;
    let current_cost = ctx.state_ref.read().transcript.usage.total_cost_usd;
    if current_cost < max_budget {
        return None;
    }

    info!(
        spent = current_cost,
        limit = max_budget,
        "max budget exceeded"
    );

    ctx.state_ref.write().runtime.abort_reason = Some(AbortReason::MaxBudget {
        spent_usd: current_cost,
        limit_usd: max_budget,
    });

    let (usage_snap, denials_snap) = {
        let state = ctx.state_ref.read();
        (
            state.transcript.usage.clone(),
            state.permissions.denials.clone(),
        )
    };
    let active_goal_id = ctx
        .state_ref
        .read()
        .runtime
        .goal_runtime
        .active_goal_id
        .clone();
    let goal_update = match allthecodes_tools::goals::mark_goal_usage_limited_for_session(
        ctx.session_id.as_str(),
        active_goal_id.as_deref(),
        format!("max budget exceeded: cost ${current_cost:.4} >= ${max_budget:.4}"),
    ) {
        Ok(Some(goal)) if goal.status == allthecodes_tools::goals::GoalStatus::UsageLimited => {
            ctx.state_ref
                .write()
                .runtime
                .goal_runtime
                .clear_for_goal(&goal.goal_id);
            Some(goal_updated_message(
                ctx.session_id.as_str(),
                "usage_limited",
                goal,
            ))
        }
        Ok(_) => None,
        Err(error) => {
            warn!(%error, "failed to mark goal usage-limited after cost budget stop");
            None
        }
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

    Some(BudgetStop {
        goal_update,
        result: SdkResult {
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
        },
    })
}
