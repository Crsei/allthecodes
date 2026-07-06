/// Core query loop -- the heart of the system.
///
/// Corresponds to TypeScript: query.ts's query() async generator.
/// This module is the canonical query-loop implementation for now; do not
/// recreate an `allthecodes-query` crate as a parallel implementation.
///
/// Structure:
///   while true {
///     1. SETUP -- destructure state, increment count
///     2. CONTEXT -- apply tool result budget, microcompact, autocompact
///     3. API CALL -- streaming model call, collect assistant message + tool use blocks
///     4. POST-STREAMING -- check abort, handle pending summary
///     5. TERMINAL CHECK (no tool calls):
///        - prompt_too_long recovery
///        - max_output_tokens recovery
///        - stop hooks
///        - token budget check
///     6. TOOL EXECUTION (has tool calls):
///        - partition into concurrent/serial batches
///        - execute tools
///        - check abort during execution
///     7. ATTACHMENTS -- inject file changes, memory, skill discovery
///     8. CONTINUE -- refresh tools, check maxTurns, state = next
///   }
use std::sync::Arc;

use async_stream::stream;
use futures::Stream;
use tracing::{debug, info, warn};
use uuid::Uuid;

use allthecodes_config::features::{self, Feature};
use allthecodes_types::agent_events::AgentEvent;
use allthecodes_types::agent_runtime_record::{compute_digest, AgentRuntimeExecutionRecord};

use crate::types::config::QueryParams;
use crate::types::message::QueryYield;
use crate::types::message::{
    AssistantMessage, Attachment, AttachmentMessage, ContentBlock, Message, RequestStartEvent,
    StreamEvent, TombstoneMessage, ToolUseSummaryMessage, Usage, UserMessage,
};
use crate::types::state::{BudgetTracker, TokenBudgetDecision};
use crate::types::transitions::Continue;

use crate::services::tool_use_summary::{self, ToolInfo};

use super::deps::{QueryDeps, ToolExecResult};
use super::goal_runtime::{
    mark_active_goal_paused, mark_active_goal_usage_limited, GoalContinuationScheduler,
};
use super::loop_helpers::{
    backfill_observable_tool_inputs, execute_tool_calls, make_abort_message, make_error_message,
    make_tool_result_user_message, make_user_message, merge_tool_results_by_tool_use_order,
    StreamingToolExecutor,
};
use super::recovery::{
    classify_model_call_failure, handle_max_output_tokens, handle_prompt_too_long,
    is_stream_progress_event, stream_idle_timeout, stream_stall_timeout,
    strip_fallback_signature_blocks, MaxTokensRecovery, ModelCallFailureRecovery,
    ModelCallFailureStage, PromptRecovery,
};
use super::stop_hooks::{self, StopHookResult};
use super::token_budget::check_token_budget;
use super::turn_context::{prepare_model_request, QueryRunContext};
use super::turn_state::QueryTurnState;

#[derive(Clone, Debug, Default)]
struct RuntimeRecordTurnContext {
    model: Option<String>,
    fallback_used: bool,
    retry_count: u32,
}

/// query() -- core query loop.
///
/// Takes query parameters and dependency injection, returns a Stream yielding `QueryYield`.
/// The caller (QueryEngine) consumes this stream to drive UI updates and message collection.
pub fn query(params: QueryParams, deps: Arc<dyn QueryDeps>) -> impl Stream<Item = QueryYield> {
    stream! {
        // Initialization

        let (turn_context, mut state) = QueryRunContext::from_params(params);
        let mut budget_tracker = BudgetTracker::new();
        let mut goal_continuation_scheduler = GoalContinuationScheduler::default();
        let mut cumulative_usage = Usage::default();

        // Main loop
        'query_loop: loop {
            // STEP 1: SETUP

            let turn_count = state.turn_count;
            let mut query_turn_state = QueryTurnState::new(turn_count);
            debug!(turn = turn_count, "query loop iteration start");

            // Emit query.turn.start audit event
            let turn_audit_ctx = deps.audit_context().with_turn();
            {
                use allthecodes_observability::{AuditLevel, EventKind, Outcome, Stage};
                turn_audit_ctx.emit(
                    EventKind::QueryTurnStart,
                    Stage::QueryTurn,
                    AuditLevel::Info,
                    Outcome::Started,
                    None,
                    Some(serde_json::json!({
                        "turn": turn_count,
                        "messages_count": state.messages.len(),
                    })),
                );
            }

            if deps.is_aborted() {
                info!("aborted before API call");
                query_turn_state.abort();
                goal_continuation_scheduler.clear();
                mark_active_goal_paused(&deps, "task aborted by user");
                yield QueryYield::Message(Message::Assistant(make_abort_message(
                    &deps,
                    "AbortedStreaming",
                )));
                break;
            }

            // STEP 1b: Inject completed background agent results

            let completed_agents = deps.drain_background_results();
            let coordinator_parent = is_coordinator_parent(&deps);
            for agent in &completed_agents {
                let msg = background_agent_message(agent, coordinator_parent);
                yield QueryYield::Message(msg.clone());
                state.messages.push(msg);
            }

            for steer_msg in drain_steer_messages(&deps) {
                yield QueryYield::Message(steer_msg.clone());
                state.messages.push(steer_msg);
            }

            // STEP 2: CONTEXT -- microcompact + autocompact

            let prepared_request =
                prepare_model_request(&deps, &mut state, &turn_context).await;

            // STEP 3: API CALL -- streaming model call

            let tools = prepared_request.tools;
            let call_params = prepared_request.call_params;
            let provider_for_langfuse = deps
                .langfuse_provider_name()
                .unwrap_or_else(|| "unknown".to_string());
            let generation_input = crate::services::langfuse::convert::convert_generation_input(
                &call_params.messages,
                &call_params.system_prompt,
                &call_params.tools,
            );
            let mut attempt_params = call_params.clone();
            let mut fallback_used = false;
            let mut retry_count = 0_u32;

            use futures::StreamExt;
            let (assistant_message, streaming_tool_executor, runtime_record_turn_context) = loop {
                let attempt_model = attempt_params
                    .model
                    .clone()
                    .unwrap_or_else(|| deps.get_app_state().main_loop_model.clone());
                let req_audit_ctx = turn_audit_ctx.with_request();
                yield QueryYield::RequestStart(RequestStartEvent {
                    submit_id: req_audit_ctx.submit_id.clone(),
                    turn_id: req_audit_ctx.turn_id.clone(),
                    request_id: req_audit_ctx.request_id.clone(),
                    provider: Some(provider_for_langfuse.clone()),
                    backend: None,
                    model: Some(attempt_model.clone()),
                    attempt: retry_count.saturating_add(1),
                    is_retry: fallback_used || retry_count > 0,
                });

                let mut generation_span = deps.langfuse_trace().as_ref().and_then(|trace| {
                    crate::services::langfuse::create_generation_span(
                        trace,
                        &attempt_model,
                        &provider_for_langfuse,
                        generation_input.clone(),
                    )
                });

                {
                    use allthecodes_observability::{AuditLevel, EventKind, Outcome, Stage};
                    req_audit_ctx.emit(
                        EventKind::ModelRequestStart,
                        Stage::ModelCall,
                        AuditLevel::Info,
                        Outcome::Started,
                        None,
                        if fallback_used {
                            Some(serde_json::json!({
                                "fallback_model": &attempt_model,
                            }))
                        } else {
                            None
                        },
                    );
                }
                let model_call_start = std::time::Instant::now();
                if let Err(error) = query_turn_state.start_streaming() {
                    yield QueryYield::Message(Message::Assistant(
                        error.to_terminal_message(turn_count),
                    ));
                    break 'query_loop;
                }

                let stream_result = deps.call_model_streaming(attempt_params.clone()).await;
                let mut event_stream = match stream_result {
                    Ok(s) => s,
                    Err(e) => {
                        let error_str = e.to_string();
                        crate::services::langfuse::finish_generation_span(
                            generation_span.take(),
                            None,
                            None,
                            None,
                            Some(&error_str),
                        );

                        let recovery = classify_model_call_failure(
                            ModelCallFailureStage::RequestStart,
                            turn_context.fallback_model.as_deref(),
                            &attempt_model,
                            &error_str,
                        );

                        if matches!(&recovery, ModelCallFailureRecovery::PromptTooLong) {
                            let terminal =
                                handle_prompt_too_long(&deps, &mut state, &error_str).await;

                            match terminal {
                                PromptRecovery::Continue(reason) => {
                                    state.transition = Some(reason);
                                    continue 'query_loop;
                                }
                                PromptRecovery::Terminal => {
                                    goal_continuation_scheduler.clear();
                                    mark_active_goal_usage_limited(
                                        &deps,
                                        "context overflow prevented goal continuation",
                                    );
                                    yield QueryYield::Message(Message::Assistant(
                                        make_error_message(&deps, &error_str),
                                    ));
                                    break 'query_loop;
                                }
                            }
                        }

                        if !fallback_used {
                            if let ModelCallFailureRecovery::Fallback { model: fallback } = recovery
                            {
                                {
                                    use allthecodes_observability::{
                                        AuditLevel, EventKind, Outcome, Stage,
                                    };
                                    req_audit_ctx.emit(
                                        EventKind::ModelRequestError,
                                        Stage::ModelCall,
                                        AuditLevel::Warn,
                                        Outcome::Failed,
                                        Some(model_call_start.elapsed().as_millis() as u64),
                                        Some(serde_json::json!({
                                            "error": error_str,
                                            "fallback_model": &fallback,
                                        })),
                                    );
                                }

                                warn!(
                                    error = %error_str,
                                    from_model = %attempt_model,
                                    to_model = %fallback,
                                    "model call failed before streaming; retrying with fallback model"
                                );

                                fallback_used = true;
                                retry_count += 1;
                                attempt_params.messages =
                                    strip_fallback_signature_blocks(&attempt_params.messages);
                                attempt_params.model = Some(fallback);
                                continue;
                            }
                        }

                        warn!(error = %e, "model call failed");
                        yield QueryYield::Message(Message::Assistant(
                            make_error_message(&deps, &error_str),
                        ));
                        break 'query_loop;
                    }
                };

                // Consume stream events, forwarding to caller while accumulating
                let mut accumulator = allthecodes_api::api::streaming::StreamAccumulator::new();
                let assistant_uuid = Uuid::new_v4();
                let streaming_tool_parent = AssistantMessage {
                    uuid: assistant_uuid,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    role: "assistant".to_string(),
                    content: vec![],
                    usage: None,
                    stop_reason: None,
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: 0.0,
                };
                let mut streaming_tool_executor = turn_context
                    .gates
                    .streaming_tool_execution
                    .then(StreamingToolExecutor::new);
                let mut stream_error: Option<String> = None;
                let mut first_response_at: Option<std::time::Instant> = None;
                let idle_timeout = stream_idle_timeout();
                let stall_timeout = stream_stall_timeout();
                let mut last_progress_at = std::time::Instant::now();

                loop {
                    let event_result = match tokio::time::timeout(
                        idle_timeout,
                        event_stream.next(),
                    ).await {
                        Ok(Some(event_result)) => event_result,
                        Ok(None) => break,
                        Err(_) => {
                            stream_error = Some(format!(
                                "stream idle timeout after {}ms",
                                idle_timeout.as_millis()
                            ));
                            break;
                        }
                    };

                    match event_result {
                        Ok(event) => {
                            let now = std::time::Instant::now();
                            if is_stream_progress_event(&event) {
                                let stalled_for = now.duration_since(last_progress_at);
                                if stalled_for > stall_timeout {
                                    stream_error = Some(format!(
                                        "stream stalled for {}ms without progress",
                                        stalled_for.as_millis()
                                    ));
                                    break;
                                }
                                last_progress_at = now;
                            }

                            if first_response_at.is_none()
                                && matches!(
                                    &event,
                                    StreamEvent::ContentBlockStart { .. }
                                        | StreamEvent::ContentBlockDelta { .. }
                                )
                            {
                                first_response_at = Some(now);
                            }
                            accumulator.process_event(&event);
                            if let StreamEvent::ContentBlockStop { index } = &event {
                                if let Some(tool_use) = accumulator.completed_tool_use(*index) {
                                    debug!(
                                        tool_index = tool_use.index,
                                        tool_id = %tool_use.id,
                                        tool_name = %tool_use.name,
                                        "identified completed streamed tool_use block"
                                    );
                                    if let Some(executor) = streaming_tool_executor.as_mut() {
                                        if executor.add_tool_use(
                                            deps.clone(),
                                            &tools,
                                            &streaming_tool_parent,
                                            deps.tool_progress_callback(),
                                            tool_use,
                                        ) {
                                            debug!(
                                                tool_index = *index,
                                                "started streaming-safe tool before message_stop"
                                            );
                                        }
                                    }
                                }
                            }
                            yield QueryYield::Stream(event);
                        }
                        Err(e) => {
                            stream_error = Some(e.to_string());
                            break;
                        }
                    }
                }

                if stream_error.as_deref().is_some_and(|err| {
                    should_accept_partial_response_after_chunk_read_error(err, &accumulator)
                }) {
                    let err = stream_error.take().unwrap_or_default();
                    warn!(
                        error = %err,
                        "stream ended with a chunk read error after text content; accepting partial assistant response"
                    );
                    if accumulator.stop_reason.is_none() {
                        accumulator.stop_reason = Some("end_turn".to_string());
                    }
                }

                if let Some(ref err) = stream_error {
                    if let Some(executor) = streaming_tool_executor.take() {
                        executor.abort();
                    }

                    let ttft_ms = first_response_at
                        .map(|instant| instant.duration_since(model_call_start).as_millis() as u64);
                    crate::services::langfuse::finish_generation_span(
                        generation_span.take(),
                        None,
                        None,
                        ttft_ms,
                        Some(err),
                    );
                    warn!(error = %err, "stream error during model call");
                    {
                        use allthecodes_observability::{AuditLevel, EventKind, Outcome, Stage};
                        req_audit_ctx.emit(
                            EventKind::ModelRequestError,
                            Stage::ModelCall,
                            AuditLevel::Error,
                            Outcome::Failed,
                            Some(model_call_start.elapsed().as_millis() as u64),
                            Some(serde_json::json!({"error": err})),
                        );
                    }

                    let recovery = classify_model_call_failure(
                        ModelCallFailureStage::StreamInterrupted,
                        turn_context.fallback_model.as_deref(),
                        &attempt_model,
                        err,
                    );
                    if !fallback_used {
                        if let ModelCallFailureRecovery::Fallback { model: fallback } = recovery {
                            let tombstone_message = accumulator.build(&attempt_model);
                            if !tombstone_message.content.is_empty() {
                                yield QueryYield::Tombstone(TombstoneMessage {
                                    message: tombstone_message,
                                });
                            }

                            warn!(
                                error = %err,
                                from_model = %attempt_model,
                                to_model = %fallback,
                                "stream failed after partial assistant; tombstoning and retrying with fallback model"
                            );

                            fallback_used = true;
                            retry_count += 1;
                            attempt_params.messages =
                                strip_fallback_signature_blocks(&attempt_params.messages);
                            attempt_params.model = Some(fallback);
                            continue;
                        }
                    }

                    yield QueryYield::Message(Message::Assistant(
                        make_error_message(&deps, err),
                    ));
                    break 'query_loop;
                }

                let assistant_message = accumulator.build_with_uuid(&attempt_model, assistant_uuid);
                let ttft_ms = first_response_at
                    .map(|instant| instant.duration_since(model_call_start).as_millis() as u64);
                crate::services::langfuse::finish_generation_span(
                    generation_span.take(),
                    Some(crate::services::langfuse::convert::convert_assistant_output(
                        &assistant_message,
                    )),
                    assistant_message.usage.as_ref(),
                    ttft_ms,
                    None,
                );

                {
                    use allthecodes_observability::{AuditLevel, EventKind, Outcome, Stage};
                    let model_duration = model_call_start.elapsed().as_millis() as u64;
                    req_audit_ctx.emit(
                        EventKind::ModelRequestFinish,
                        Stage::ModelCall,
                        AuditLevel::Info,
                        Outcome::Completed,
                        Some(model_duration),
                        Some(serde_json::json!({
                            "stop_reason": assistant_message.stop_reason,
                            "tool_use_count": assistant_message.content.iter()
                                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                                .count(),
                        })),
                    );
                }

                break (
                    assistant_message,
                    streaming_tool_executor,
                    RuntimeRecordTurnContext {
                        model: Some(attempt_model),
                        fallback_used,
                        retry_count,
                    },
                );
            };

            // Accumulate usage
            if let Some(ref usage) = assistant_message.usage {
                cumulative_usage.input_tokens += usage.input_tokens;
                cumulative_usage.output_tokens += usage.output_tokens;
                cumulative_usage.cache_read_input_tokens += usage.cache_read_input_tokens;
                cumulative_usage.cache_creation_input_tokens += usage.cache_creation_input_tokens;
            }

            // STEP 4: POST-STREAMING -- check abort, pending summary

            if deps.is_aborted() {
                info!("aborted after streaming");
                query_turn_state.abort();
                let observable_assistant =
                    backfill_observable_tool_inputs(&assistant_message, &tools).into_owned();
                yield QueryYield::Message(Message::Assistant(observable_assistant));
                goal_continuation_scheduler.clear();
                mark_active_goal_paused(&deps, "task aborted by user");
                break;
            }

            // Inject pending tool use summary as system message
            if turn_context.gates.emit_tool_use_summaries {
                if let Some(summary) = state.pending_tool_use_summary.take() {
                    debug!(summary = %allthecodes_utils::messages::truncate_text(&summary, 200), "injecting tool use summary");
                    let sys_msg = Message::System(crate::types::message::SystemMessage {
                        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        subtype: crate::types::message::SystemSubtype::Informational {
                            level: crate::types::message::InfoLevel::Info,
                        },
                        content: format!("[tool summary] {}", summary),
                    });
                    state.messages.push(sys_msg);
                }
            }

            // Yield assistant message
            let observable_assistant =
                backfill_observable_tool_inputs(&assistant_message, &tools).into_owned();
            yield QueryYield::Message(Message::Assistant(observable_assistant));
            state.messages.push(Message::Assistant(assistant_message.clone()));

            // STEP 5 vs 6: Branch -- tool calls or not

            let tool_uses = stop_hooks::extract_tool_uses(&assistant_message);
            if let Err(error) = query_turn_state.finish_streaming(!tool_uses.is_empty()) {
                yield QueryYield::Message(Message::Assistant(
                    error.to_terminal_message(turn_count),
                ));
                break 'query_loop;
            }
            if goal_continuation_scheduler
                .observe_assistant_response(&assistant_message)
                .is_some()
            {
                mark_active_goal_usage_limited(
                    &deps,
                    "automatic goal continuation returned empty responses",
                );
                break;
            }

            if tool_uses.is_empty() {
                if let Some(executor) = streaming_tool_executor {
                    executor.abort();
                }

                let steer_messages = drain_steer_messages(&deps);
                if !steer_messages.is_empty() {
                    for steer_msg in steer_messages {
                        yield QueryYield::Message(steer_msg.clone());
                        state.messages.push(steer_msg);
                    }
                    state.transition = Some(Continue::NextTurn);
                    state.turn_count += 1;
                    continue;
                }

                // TERMINAL CHECK (no tool calls)

                // 5a. max_output_tokens recovery
                if assistant_message.stop_reason.as_deref() == Some("max_tokens") {
                    let recovery = handle_max_output_tokens(
                        &deps,
                        &mut state,
                        &assistant_message,
                    );

                    match recovery {
                        MaxTokensRecovery::Continue(reason) => {
                            state.transition = Some(reason);
                            continue;
                        }
                        MaxTokensRecovery::Terminal => {
                            goal_continuation_scheduler.clear();
                            mark_active_goal_usage_limited(
                                &deps,
                                "max output token recovery exhausted",
                            );
                            break;
                        }
                    }
                }

                // 5b. stop hooks
                let hooks_map = deps.get_app_state().hooks;
                let runner = deps.hook_runner();
                let stop_configs = runner.load_hook_configs(&hooks_map, "Stop");

                let stop_result = stop_hooks::run_stop_hooks(
                    runner.as_ref(),
                    &assistant_message,
                    &state.messages,
                    state.stop_hook_active,
                    &stop_configs,
                )
                .await;

                match stop_result {
                    Ok(StopHookResult::PreventStop { continuation_message }) => {
                        let user_msg = make_user_message(
                            &deps,
                            &continuation_message,
                            true,
                        );
                        state.messages.push(Message::User(user_msg));
                        state.stop_hook_active = Some(true);
                        state.transition = Some(Continue::StopHookBlocking);
                        state.turn_count += 1;
                        continue;
                    }
                    Ok(StopHookResult::BlockingError { error }) => {
                        warn!(error = %error, "stop hook blocking error");

                        // Fire StopFailure hook
                        let sf_configs = runner.load_hook_configs(&hooks_map, "StopFailure");
                        if !sf_configs.is_empty() {
                            let payload = serde_json::json!({ "error": error });
                            let _ = runner.run_event_hooks("StopFailure", &payload, &sf_configs).await;
                        }

                        break;
                    }
                    Ok(StopHookResult::AllowStop) => {}
                    Err(e) => {
                        warn!(error = %e, "stop hook execution error");
                    }
                }

                // 5c. token budget check
                let global_turn_tokens = cumulative_usage.output_tokens;
                let budget_decision = check_token_budget(
                    &mut budget_tracker,
                    turn_context.token_budget_scope(),
                    turn_context.task_budget_total,
                    global_turn_tokens,
                );

                match budget_decision {
                    TokenBudgetDecision::Continue {
                        nudge_message,
                        continuation_count,
                        ..
                    } => {
                        debug!(
                            continuation = continuation_count,
                            "token budget: continuing"
                        );
                        let user_msg = make_user_message(&deps, &nudge_message, true);
                        state.messages.push(Message::User(user_msg));
                        state.transition = Some(Continue::TokenBudgetContinuation);
                        state.turn_count += 1;
                        continue;
                    }
                    TokenBudgetDecision::Stop { completion_event } => {
                        if let Some(ref event) = completion_event {
                            debug!(
                                pct = event.pct,
                                turns = event.continuation_count,
                                "token budget: stopping"
                            );
                            break;
                        }
                        if turn_context.task_budget_total.is_some() {
                            break;
                        }
                    }
                }

                if let Some(continuation) =
                    goal_continuation_scheduler.next_idle_continuation(&deps, &turn_context, &state)
                {
                    if let Some(max) = turn_context.max_turns {
                        if state.turn_count >= max {
                            info!(turns = state.turn_count, max = max, "max turns reached");
                            let attachment_msg = AttachmentMessage {
                                uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
                                timestamp: chrono::Utc::now().timestamp_millis(),
                                attachment: Attachment::MaxTurnsReached {
                                    max_turns: max,
                                    turn_count: state.turn_count,
                                },
                            };
                            yield QueryYield::Message(Message::Attachment(attachment_msg));
                            break;
                        }
                    }
                    if !goal_continuation_scheduler.confirm_ready(&deps, &continuation.goal_id) {
                        break;
                    }
                    debug!("active goal still open; continuing query loop");
                    let user_msg = make_user_message(&deps, &continuation.message, true);
                    state.messages.push(Message::User(user_msg));
                    goal_continuation_scheduler.mark_dispatched(&continuation.goal_id);
                    state.transition = Some(Continue::NextTurn);
                    state.turn_count += 1;
                    continue;
                }

                break;
            } else {
                // STEP 6: TOOL EXECUTION

                let tool_results = if let Some(executor) = streaming_tool_executor {
                    let streamed = executor.finish().await;
                    let remaining_tool_uses = tool_uses
                        .iter()
                        .filter(|(tool_use_id, _, _)| {
                            !streamed.started_tool_use_ids.contains(tool_use_id)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let remaining_results = execute_tool_calls(
                        &deps,
                        &remaining_tool_uses,
                        &tools,
                        &assistant_message,
                        deps.tool_progress_callback(),
                    )
                    .await;
                    merge_tool_results_by_tool_use_order(
                        &tool_uses,
                        streamed.results,
                        remaining_results,
                    )
                } else {
                    execute_tool_calls(
                        &deps,
                        &tool_uses,
                        &tools,
                        &assistant_message,
                        deps.tool_progress_callback(),
                    )
                    .await
                };

                if deps.is_aborted() {
                    info!("aborted during tool execution");
                    query_turn_state.abort();
                    goal_continuation_scheduler.clear();
                    mark_active_goal_paused(&deps, "task aborted by user");
                    break;
                }

                // Convert tool results to user messages
                for exec_result in &tool_results {
                    let user_msg =
                        make_tool_result_user_message(&deps, exec_result, assistant_message.uuid);
                    let msg = Message::User(user_msg);
                    yield QueryYield::Message(msg.clone());
                    state.messages.push(msg);

                    if let Some(brief_message) = &exec_result.brief_message {
                        yield QueryYield::BriefMessage(brief_message.clone());
                    }

                    for sub_msg in &exec_result.result.new_messages {
                        yield QueryYield::Message(sub_msg.clone());
                        state.messages.push(sub_msg.clone());
                    }

                    if exec_result.tool_name == "Snip" {
                        let removed =
                            apply_snip_projection(&mut state.messages, &exec_result.result.data);
                        if removed > 0 {
                            debug!(removed, "applied Snip projection to future context");
                        }
                    }

                    // Emit structured execution record for this tool invocation
                    let record =
                        build_execution_record(&deps, exec_result, &runtime_record_turn_context);
                    if let Err(err) = crate::agent_runtime::emit_execution_record(&record) {
                        debug!(error = %err, "failed to emit dashboard execution record");
                    }
                    deps.send_agent_event(AgentEvent::ExecutionRecord {
                        agent_id: deps.agent_id().unwrap_or("main").to_string(),
                        record: Box::new(record),
                    });
                }

                let steer_messages = drain_steer_messages(&deps);
                if !steer_messages.is_empty() {
                    for steer_msg in steer_messages {
                        yield QueryYield::Message(steer_msg.clone());
                        state.messages.push(steer_msg);
                    }
                    state.transition = Some(Continue::NextTurn);
                    state.turn_count += 1;
                    continue;
                }

                // STEP 6b: Generate tool use summary
                if turn_context.gates.emit_tool_use_summaries {
                    let tool_infos: Vec<ToolInfo> = tool_results
                        .iter()
                        .map(|r| ToolInfo {
                            name: r.tool_name.clone(),
                            input_summary: r.result.data.to_string(),
                            output_summary: if r.is_error {
                                format!("Error: {}", r.result.data)
                            } else {
                                r.result.data.to_string()
                            },
                        })
                        .collect();

                    let last_text = assistant_message.content.iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    });

                    if let Some(summary) = tool_use_summary::generate_tool_use_summary(
                        &tool_infos,
                        last_text,
                    ) {
                        let summary_msg = ToolUseSummaryMessage {
                            uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
                            summary: summary.clone(),
                            preceding_tool_use_ids: tool_results
                                .iter()
                                .map(|r| r.tool_use_id.clone())
                                .collect(),
                        };
                        yield QueryYield::ToolUseSummary(summary_msg);
                        state.pending_tool_use_summary = Some(summary);
                    }
                }

                // STEP 7: ATTACHMENTS (placeholder)

                // STEP 8: CONTINUE -- refresh tools, check maxTurns

                if tool_results
                    .iter()
                    .any(|result| result.hook_stopped_continuation)
                {
                    let attachment_msg = AttachmentMessage {
                        uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        attachment: Attachment::HookStoppedContinuation,
                    };
                    let msg = Message::Attachment(attachment_msg);
                    yield QueryYield::Message(msg.clone());
                    state.messages.push(msg);
                    break;
                }

                if let Some(max) = turn_context.max_turns {
                    if state.turn_count >= max {
                        info!(turns = state.turn_count, max = max, "max turns reached");
                        let attachment_msg = AttachmentMessage {
                            uuid: Uuid::parse_str(&deps.uuid()).unwrap_or_else(|_| Uuid::new_v4()),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            attachment: Attachment::MaxTurnsReached {
                                max_turns: max,
                                turn_count: state.turn_count,
                            },
                        };
                        yield QueryYield::Message(Message::Attachment(attachment_msg));
                        break;
                    }
                }

                match deps.refresh_tools().await {
                    Ok(_refreshed) => {
                        debug!("tools refreshed successfully");
                    }
                    Err(e) => {
                        debug!(error = %e, "tool refresh failed, continuing with existing tools");
                    }
                }

                if let Err(error) = query_turn_state.finish_tool_execution() {
                    yield QueryYield::Message(Message::Assistant(
                        error.to_terminal_message(turn_count),
                    ));
                    break 'query_loop;
                }
                state.transition = Some(Continue::NextTurn);
                state.turn_count += 1;
                state.stop_hook_active = None;
                continue;
            }
        }

        info!(turns = state.turn_count, "query loop finished");
    }
}

fn should_accept_partial_response_after_chunk_read_error(
    err: &str,
    accumulator: &allthecodes_api::api::streaming::StreamAccumulator,
) -> bool {
    if !err.contains("error reading response chunk") {
        return false;
    }

    let has_text = accumulator.content_blocks.iter().any(|block| match block {
        ContentBlock::Text { text } => !text.is_empty(),
        ContentBlock::Thinking { thinking, .. } => !thinking.is_empty(),
        ContentBlock::ConnectorText { connector_text, .. } => !connector_text.is_empty(),
        _ => false,
    });
    let has_tool_use = accumulator.content_blocks.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { .. } | ContentBlock::ServerToolUse { .. }
        )
    });

    has_text && !has_tool_use
}

fn drain_steer_messages(deps: &Arc<dyn QueryDeps>) -> Vec<Message> {
    deps.drain_steer_messages()
        .into_iter()
        .filter_map(|text| {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Message::User(make_user_message(deps, trimmed, false)))
            }
        })
        .collect()
}

fn apply_snip_projection(messages: &mut Vec<Message>, data: &serde_json::Value) -> usize {
    let ids = data
        .get("message_ids")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<std::collections::HashSet<_>>();
    if ids.is_empty() {
        return 0;
    }

    let before = messages.len();
    messages.retain(|message| !ids.contains(&message.uuid().to_string()));
    before.saturating_sub(messages.len())
}

/// Build an `AgentRuntimeExecutionRecord` from a completed tool execution.
fn build_execution_record(
    deps: &Arc<dyn QueryDeps>,
    exec_result: &ToolExecResult,
    turn_context: &RuntimeRecordTurnContext,
) -> AgentRuntimeExecutionRecord {
    let shell = exec_result.result.shell.as_ref();
    let shell_like = shell.is_some() || is_shell_tool_name(&exec_result.tool_name);

    let command = shell.and_then(|output| output.command.clone()).or_else(|| {
        shell_like
            .then(|| {
                input_string_field(&exec_result.effective_input, &["command", "cmd", "script"])
            })
            .flatten()
    });
    let cwd = shell.and_then(|output| output.cwd.clone());
    let exit_code = shell.and_then(|output| output.exit_code);
    let stdout_digest = shell.map(|output| compute_digest(output.stdout.as_bytes()));
    let stderr_digest = shell.map(|output| compute_digest(output.stderr.as_bytes()));
    let shell_had_error = shell
        .map(|output| {
            output.exit_code.is_some_and(|code| code != 0)
                || output.interrupted
                || output.error.is_some()
        })
        .unwrap_or(false);

    AgentRuntimeExecutionRecord {
        session_id: deps.session_id().to_string(),
        agent_id: deps.agent_id().unwrap_or("main").to_string(),
        parent_agent_id: deps.parent_agent_id().map(str::to_string),
        agent_role: deps.agent_type().map(|s| s.to_string()),
        tool: normalize_runtime_tool_name(&exec_result.tool_name, shell_like),
        tool_use_id: Some(exec_result.tool_use_id.clone()),
        command,
        cwd,
        exit_code,
        stdout_digest,
        stderr_digest,
        retry_count: turn_context.retry_count,
        model: turn_context.model.clone(),
        fallback_used: turn_context.fallback_used,
        permission_decision: exec_result.permission_decision,
        duration_ms: exec_result.duration_ms,
        had_error: exec_result.is_error || shell_had_error,
        schema_version: 1,
    }
}

fn normalize_runtime_tool_name(tool_name: &str, shell_like: bool) -> String {
    if shell_like || is_shell_tool_name(tool_name) {
        return "shell".to_string();
    }

    match tool_name {
        "Read" | "FileRead" | "read" | "read_file" => "read".to_string(),
        "Write" | "FileWrite" | "write" | "write_file" => "write".to_string(),
        "Edit" | "FileEdit" | "edit" | "edit_file" => "edit".to_string(),
        "MultiEdit" | "FileMultiEdit" => "multi_edit".to_string(),
        "NotebookEdit" => "notebook_edit".to_string(),
        "WebFetch" => "web_fetch".to_string(),
        "WebSearch" => "web_search".to_string(),
        "Agent" | "agent" => "agent".to_string(),
        other => other.to_string(),
    }
}

fn is_shell_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "Bash" | "bash" | "PowerShell" | "powershell" | "pwsh" | "Pwsh"
    )
}

fn input_string_field(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| input.get(*key))
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn is_coordinator_parent(deps: &Arc<dyn QueryDeps>) -> bool {
    if !features::enabled(Feature::Coordinator) {
        return false;
    }
    deps.get_app_state()
        .team_context
        .as_ref()
        .is_none_or(|team_context| allthecodes_types::teams::is_team_lead(Some(team_context)))
}

fn background_agent_message(
    agent: &crate::agent_runtime::CompletedBackgroundAgent,
    coordinator_parent: bool,
) -> Message {
    if coordinator_parent && is_worker_subagent(agent.agent_type.as_deref()) {
        return Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "user".to_string(),
            content: crate::types::message::MessageContent::Text(background_task_notification(
                agent,
            )),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
    }

    Message::System(crate::types::message::SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        subtype: crate::types::message::SystemSubtype::Informational {
            level: crate::types::message::InfoLevel::Info,
        },
        content: background_agent_system_content(agent),
    })
}

fn is_worker_subagent(subagent_type: Option<&str>) -> bool {
    subagent_type
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("worker"))
}

fn background_agent_system_content(
    agent: &crate::agent_runtime::CompletedBackgroundAgent,
) -> String {
    if agent.had_error {
        format!(
            "[Background agent '{}' (id: {}) failed after {:.1}s]\n\n{}",
            agent.description,
            agent.agent_id,
            agent.duration.as_secs_f64(),
            agent.result_text,
        )
    } else {
        format!(
            "[Background agent '{}' (id: {}) completed in {:.1}s]\n\n{}",
            agent.description,
            agent.agent_id,
            agent.duration.as_secs_f64(),
            agent.result_text,
        )
    }
}

fn background_task_notification(agent: &crate::agent_runtime::CompletedBackgroundAgent) -> String {
    let status = agent.completion_status.as_str();
    let summary = format!("Agent \"{}\" {}", agent.description, status);
    let mut xml = String::from("<task-notification>");
    push_xml_tag(&mut xml, "task-id", &agent.agent_id);
    push_xml_tag(&mut xml, "status", status);
    push_xml_tag(&mut xml, "summary", &summary);
    push_xml_tag(&mut xml, "result", &agent.result_text);
    xml.push_str("<usage>");
    push_xml_tag(
        &mut xml,
        "duration_ms",
        &agent.duration.as_millis().to_string(),
    );
    if let Some(total_tokens) = agent.total_tokens {
        push_xml_tag(&mut xml, "total_tokens", &total_tokens.to_string());
    }
    if let Some(tool_uses) = agent.tool_uses {
        push_xml_tag(&mut xml, "tool_uses", &tool_uses.to_string());
    }
    xml.push_str("</usage>");
    xml.push_str("</task-notification>");
    xml
}

fn push_xml_tag(xml: &mut String, tag: &str, value: &str) {
    xml.push('<');
    xml.push_str(tag);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push('>');
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod task_notification_tests {
    use super::*;

    #[test]
    fn task_notification_background_message_is_user_role_for_coordinator() {
        let agent = crate::agent_runtime::CompletedBackgroundAgent {
            agent_id: "agent-bg".to_string(),
            description: "collect data".to_string(),
            agent_type: Some("worker".to_string()),
            result_text: "finished".to_string(),
            had_error: false,
            completion_status: allthecodes_types::agent_events::AgentCompletionStatus::Completed,
            duration: std::time::Duration::from_millis(25),
            total_tokens: Some(99),
            tool_uses: Some(2),
        };

        let message = background_agent_message(&agent, true);

        match message {
            Message::User(user) => {
                assert!(user.is_meta);
                let text = match user.content {
                    crate::types::message::MessageContent::Text(text) => text,
                    _ => String::new(),
                };
                assert!(text.contains("<task-notification>"));
                assert!(text.contains("<task-id>agent-bg</task-id>"));
                assert!(text.contains("<status>completed</status>"));
                assert!(text.contains("<duration_ms>25</duration_ms>"));
                assert!(text.contains("<total_tokens>99</total_tokens>"));
                assert!(text.contains("<tool_uses>2</tool_uses>"));
            }
            other => panic!("expected user notification, got {other:?}"),
        }
    }

    #[test]
    fn normal_background_message_stays_system_role() {
        let agent = crate::agent_runtime::CompletedBackgroundAgent {
            agent_id: "agent-bg".to_string(),
            description: "collect data".to_string(),
            agent_type: Some("worker".to_string()),
            result_text: "finished".to_string(),
            had_error: false,
            completion_status: allthecodes_types::agent_events::AgentCompletionStatus::Completed,
            duration: std::time::Duration::from_millis(25),
            total_tokens: None,
            tool_uses: None,
        };

        assert!(matches!(
            background_agent_message(&agent, false),
            Message::System(_)
        ));
    }

    #[test]
    fn coordinator_background_non_worker_stays_system_role() {
        let agent = crate::agent_runtime::CompletedBackgroundAgent {
            agent_id: "agent-bg".to_string(),
            description: "collect data".to_string(),
            agent_type: Some("researcher".to_string()),
            result_text: "finished".to_string(),
            had_error: false,
            completion_status: allthecodes_types::agent_events::AgentCompletionStatus::Completed,
            duration: std::time::Duration::from_millis(25),
            total_tokens: None,
            tool_uses: None,
        };

        assert!(matches!(
            background_agent_message(&agent, true),
            Message::System(_)
        ));
    }

    #[test]
    fn killed_background_worker_notification_uses_killed_status() {
        let agent = crate::agent_runtime::CompletedBackgroundAgent {
            agent_id: "agent-bg".to_string(),
            description: "collect data".to_string(),
            agent_type: Some("worker".to_string()),
            result_text: "(Agent cancelled before producing text output)".to_string(),
            had_error: true,
            completion_status: allthecodes_types::agent_events::AgentCompletionStatus::Killed,
            duration: std::time::Duration::from_millis(25),
            total_tokens: None,
            tool_uses: None,
        };

        let message = background_agent_message(&agent, true);

        let Message::User(user) = message else {
            panic!("expected user notification for coordinator worker");
        };
        let crate::types::message::MessageContent::Text(text) = user.content else {
            panic!("expected text notification");
        };
        assert!(text.contains("<status>killed</status>"));
        assert!(text.contains("Agent &quot;collect data&quot; killed"));
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
