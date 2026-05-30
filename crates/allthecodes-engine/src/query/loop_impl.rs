/// Core query loop -- the heart of the system.
///
/// Corresponds to TypeScript: query.ts's query() async generator.
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

use crate::types::config::QueryParams;
use crate::types::message::QueryYield;
use crate::types::message::{
    AssistantMessage, Attachment, AttachmentMessage, ContentBlock, Message, RequestStartEvent,
    StreamEvent, TombstoneMessage, ToolUseSummaryMessage, Usage,
};
use crate::types::state::{BudgetTracker, TokenBudgetDecision};
use crate::types::transitions::Continue;

use crate::services::tool_use_summary::{self, ToolInfo};

use super::deps::QueryDeps;
use super::loop_helpers::{
    backfill_observable_tool_inputs, classify_model_call_failure, execute_tool_calls,
    handle_max_output_tokens, handle_prompt_too_long, is_stream_progress_event, make_abort_message,
    make_error_message, make_tool_result_user_message, make_user_message,
    merge_tool_results_by_tool_use_order, stream_idle_timeout, stream_stall_timeout,
    strip_fallback_signature_blocks, MaxTokensRecovery, ModelCallFailureRecovery,
    ModelCallFailureStage, PromptRecovery, StreamingToolExecutor,
};
use super::stop_hooks::{self, StopHookResult};
use super::token_budget::check_token_budget;
use super::turn_context::{prepare_model_request, QueryRunContext};

/// query() -- core query loop.
///
/// Takes query parameters and dependency injection, returns a Stream yielding `QueryYield`.
/// The caller (QueryEngine) consumes this stream to drive UI updates and message collection.
pub fn query(params: QueryParams, deps: Arc<dyn QueryDeps>) -> impl Stream<Item = QueryYield> {
    stream! {
        // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
        // Initialization
        // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

        let (turn_context, mut state) = QueryRunContext::from_params(params);
        let mut budget_tracker = BudgetTracker::new();
        let mut cumulative_usage = Usage::default();

        // Main loop
        'query_loop: loop {
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 1: SETUP
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

            let turn_count = state.turn_count;
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
                yield QueryYield::Message(Message::Assistant(make_abort_message(
                    &deps,
                    "AbortedStreaming",
                )));
                break;
            }

            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 1b: Inject completed background agent results
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

            let completed_agents = deps.drain_background_results();
            for agent in &completed_agents {
                let content = if agent.had_error {
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
                };

                let sys_msg = Message::System(crate::types::message::SystemMessage {
                    uuid: Uuid::new_v4(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    subtype: crate::types::message::SystemSubtype::Informational {
                        level: crate::types::message::InfoLevel::Info,
                    },
                    content,
                });
                yield QueryYield::Message(sys_msg.clone());
                state.messages.push(sys_msg);
            }

            for steer_msg in drain_steer_messages(&deps) {
                yield QueryYield::Message(steer_msg.clone());
                state.messages.push(steer_msg);
            }

            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 2: CONTEXT -- microcompact + autocompact
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

            let prepared_request =
                prepare_model_request(&deps, &mut state, &turn_context).await;

            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 3: API CALL -- streaming model call
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

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
            let req_audit_ctx = turn_audit_ctx.with_request();
            let mut attempt_params = call_params.clone();
            let mut fallback_used = false;

            use futures::StreamExt;
            let (assistant_message, streaming_tool_executor) = loop {
                yield QueryYield::RequestStart(RequestStartEvent);

                let attempt_model = attempt_params
                    .model
                    .clone()
                    .unwrap_or_else(|| deps.get_app_state().main_loop_model.clone());
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

                break (assistant_message, streaming_tool_executor);
            };

            // Accumulate usage
            if let Some(ref usage) = assistant_message.usage {
                cumulative_usage.input_tokens += usage.input_tokens;
                cumulative_usage.output_tokens += usage.output_tokens;
                cumulative_usage.cache_read_input_tokens += usage.cache_read_input_tokens;
                cumulative_usage.cache_creation_input_tokens += usage.cache_creation_input_tokens;
            }

            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 4: POST-STREAMING -- check abort, pending summary
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

            if deps.is_aborted() {
                info!("aborted after streaming");
                let observable_assistant =
                    backfill_observable_tool_inputs(&assistant_message, &tools).into_owned();
                yield QueryYield::Message(Message::Assistant(observable_assistant));
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

            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
            // STEP 5 vs 6: Branch -- tool calls or not
            // 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

            let tool_uses = stop_hooks::extract_tool_uses(&assistant_message);

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

                // 鈹€鈹€ TERMINAL CHECK (no tool calls) 鈹€鈹€

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
                        }
                        break;
                    }
                }
            } else {
                // 鈹€鈹€ STEP 6: TOOL EXECUTION 鈹€鈹€

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
                    break;
                }

                // Convert tool results to user messages
                for exec_result in &tool_results {
                    let user_msg =
                        make_tool_result_user_message(&deps, exec_result, assistant_message.uuid);
                    let msg = Message::User(user_msg);
                    yield QueryYield::Message(msg.clone());
                    state.messages.push(msg);

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

                // 鈹€鈹€ STEP 6b: Generate tool use summary 鈹€鈹€
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

                // 鈹€鈹€ STEP 7: ATTACHMENTS (placeholder) 鈹€鈹€

                // 鈹€鈹€ STEP 8: CONTINUE -- refresh tools, check maxTurns 鈹€鈹€

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

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
