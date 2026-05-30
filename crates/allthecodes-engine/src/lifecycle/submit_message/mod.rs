//! `QueryEngine::submit_message` -- the main conversation turn pipeline.
//!
//! Phase A: Input Processing
//! Phase B: System Prompt Build
//! Phase C: Pre-Query Setup (SystemInit, local-command fast path)
//! Phase D: Query Loop -- full message dispatch
//! Phase E: Result Generation (SdkResult)

use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use futures::Stream;
use tracing::{info, warn};
use uuid::Uuid;

use crate::codex_exec;
use crate::input_processing;
use crate::result;
use crate::session::transcript;
use crate::types::config::{QueryParams, QuerySource};
use allthecodes_engine::query::loop_impl;
use allthecodes_types::sdk::*;

use super::deps::QueryEngineDeps;
use super::QueryEngine;

mod command_handling;
mod memory_recall;
mod stream_handler;
mod system_prompt_build;

use command_handling::{bash_mode_result_message, handle_parsed_command, skill_args_from_prompt};
use stream_handler::{
    account_goal_runtime_message, check_budget, process_stream_item, StreamAction, StreamContext,
};
use system_prompt_build::build_submit_system_prompt;

#[cfg(feature = "telemetry")]
type SubmitTelemetrySpan = Option<crate::telemetry_bridge::SpanId>;
#[cfg(not(feature = "telemetry"))]
type SubmitTelemetrySpan = Option<u64>;

struct SubmitTurnState {
    started_at: Instant,
    last_stop_reason: Option<String>,
    structured_output: Option<serde_json::Value>,
    turn_count_this_submit: usize,
    collected_errors: Vec<String>,
}

impl SubmitTurnState {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            last_stop_reason: None,
            structured_output: None,
            turn_count_this_submit: 0,
            collected_errors: Vec::new(),
        }
    }

    fn duration_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

#[cfg(feature = "telemetry")]
fn start_submit_telemetry(
    session_id: &str,
    submit_id: &str,
) -> Option<crate::telemetry_bridge::SpanId> {
    crate::telemetry_bridge::with_bridge(|bridge| bridge.start_submit(session_id, submit_id))
}

#[cfg(not(feature = "telemetry"))]
fn start_submit_telemetry(_session_id: &str, _submit_id: &str) -> Option<u64> {
    None
}

#[cfg(feature = "telemetry")]
fn finish_submit_telemetry(
    span_id: &mut Option<crate::telemetry_bridge::SpanId>,
    model: &str,
    usage: &UsageTracking,
) {
    if let Some(span_id) = span_id.take() {
        let _ = crate::telemetry_bridge::with_bridge(|bridge| {
            bridge.end_submit(
                span_id,
                model,
                usage.total_input_tokens,
                usage.total_output_tokens,
            )
        });
    }
}

#[cfg(not(feature = "telemetry"))]
fn finish_submit_telemetry(_span_id: &mut Option<u64>, _model: &str, _usage: &UsageTracking) {}

#[cfg(feature = "telemetry")]
fn start_hook_telemetry(hook_name: &str) -> Option<crate::telemetry_bridge::SpanId> {
    crate::telemetry_bridge::with_bridge(|bridge| bridge.start_hook(hook_name))
}

#[cfg(not(feature = "telemetry"))]
fn start_hook_telemetry(_hook_name: &str) -> Option<u64> {
    None
}

#[cfg(feature = "telemetry")]
fn finish_hook_telemetry(span_id: Option<crate::telemetry_bridge::SpanId>, result: &str) {
    if let Some(span_id) = span_id {
        let _ = crate::telemetry_bridge::with_bridge(|bridge| bridge.end_hook(span_id, result));
    }
}

#[cfg(not(feature = "telemetry"))]
fn finish_hook_telemetry(_span_id: Option<u64>, _result: &str) {}

impl QueryEngine {
    /// Submit a user message and return a stream of `SdkMessage` items.
    ///
    /// This is the primary entry point for driving a conversation turn.
    /// The caller should consume the entire stream; every invocation ends
    /// with exactly one `SdkMessage::Result`.
    pub fn submit_message(
        &self,
        prompt: &str,
        query_source: QuerySource,
    ) -> Pin<Box<dyn Stream<Item = SdkMessage> + Send>> {
        let session_id = self.current_session_id();
        info!(
            prompt_len = prompt.len(),
            source = ?query_source,
            session = %session_id,
            "submit_message: starting"
        );

        // Capture owned/cloned references for the async stream closure.
        let config = self.config.clone();
        let prompt = prompt.to_string();

        let state_ref = self.state.clone();
        let active_session_id_ref = self.active_session_id.clone();
        let aborted_ref = self.aborted.clone();
        let active_steer_state = self.active_steer_state.clone();
        let pending_bg_results = self.pending_bg_results.clone();
        let hook_runner = self.hook_runner.clone();
        let command_dispatcher = self.command_dispatcher.clone();
        let command_executor = self.command_executor.clone();
        let auto_classifier_fn = self.auto_classifier_fn.clone();

        let stream = async_stream::stream! {
            let _active_steer_guard = super::ActiveSteerGuard::activate(active_steer_state.clone());
            let mut submit_turn = SubmitTurnState::new();
            let submit_id = Uuid::new_v4().to_string();
            let mut telemetry_submit_span =
                start_submit_telemetry(session_id.as_str(), &submit_id);

            // Emit submit.received audit event
            {
                use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                let ctx = state_ref.read().audit_ctx.with_submit();
                ctx.emit(
                    EventKind::SubmitReceived,
                    Stage::Submit,
                    AuditLevel::Info,
                    Outcome::Started,
                    None,
                    Some(serde_json::json!({
                        "prompt_len": prompt.len(),
                        "source": format!("{:?}", query_source),
                    })),
                );
            }

            // ================================================================
            // PHASE A-pre: Fire UserPromptSubmit hook
            // ================================================================
            {
                let hooks_map = state_ref.read().app_state.hooks.clone();
                let configs = hook_runner.load_hook_configs(&hooks_map, "UserPromptSubmit");
                if !configs.is_empty() {
                    let hook_span = start_hook_telemetry("UserPromptSubmit");
                    let payload = serde_json::json!({
                        "prompt": &prompt,
                    });
                    match hook_runner
                        .run_event_hooks("UserPromptSubmit", &payload, &configs)
                        .await
                    {
                        Ok(output) => {
                            if !output.should_continue {
                                info!("UserPromptSubmit hook blocked prompt");
                                let reason = output.reason
                                    .or(output.stop_reason)
                                    .unwrap_or_else(|| "Blocked by UserPromptSubmit hook".to_string());
                                finish_hook_telemetry(hook_span, "blocked");
                                let telemetry_model = config
                                    .user_specified_model
                                    .clone()
                                    .unwrap_or_else(|| {
                                        state_ref.read().app_state.main_loop_model.clone()
                                    });
                                finish_submit_telemetry(
                                    &mut telemetry_submit_span,
                                    &telemetry_model,
                                    &UsageTracking::default(),
                                );
                                yield SdkMessage::Result(SdkResult {
                                    subtype: ResultSubtype::Success,
                                    is_error: false,
                                    duration_ms: submit_turn.duration_ms(),
                                    duration_api_ms: 0,
                                    num_turns: 0,
                                    result: reason,
                                    stop_reason: None,
                                    session_id: session_id.to_string(),
                                    total_cost_usd: 0.0,
                                    usage: UsageTracking::default(),
                                    permission_denials: vec![],
                                    structured_output: None,
                                    uuid: Uuid::new_v4(),
                                    errors: vec![],
                                });
                                return;
                            }
                            finish_hook_telemetry(hook_span, "success");
                        }
                        Err(e) => {
                            finish_hook_telemetry(hook_span, "error");
                            warn!(error = %e, "UserPromptSubmit hook error, continuing");
                        }
                    }
                }
            }

            // ================================================================
            // PHASE A: Input Processing
            // ================================================================

            // A.1: Clear turn-scoped state
            state_ref.write().discovered_skill_names.clear();

            // A.2: Process user input (delegate to input_processing module)
            let current_msgs_snapshot = state_ref.read().messages.clone();
            let mut processed = input_processing::process_user_input(
                &prompt,
                &current_msgs_snapshot,
                &config.cwd,
                command_dispatcher.as_ref(),
            );

            let mut local_command = handle_parsed_command(
                &mut processed,
                &current_msgs_snapshot,
                &config,
                &state_ref,
                &active_session_id_ref,
                &session_id,
                command_dispatcher.as_ref(),
                command_executor.as_ref(),
            )
            .await;

            if let Some(skill_name) = processed.skill_invocation.clone() {
                match allthecodes_skills::find_skill(&skill_name) {
                    Some(skill) => {
                        let args = skill_args_from_prompt(&prompt, &skill_name);
                        let main_loop_model = state_ref.read().app_state.main_loop_model.clone();
                        let prepared = allthecodes_skills::invocation::prepare_skill_invocation(
                            &skill,
                            &args,
                            &main_loop_model,
                            Some(session_id.as_str()),
                        );
                        processed.messages = match prepared {
                            allthecodes_skills::invocation::PreparedSkillInvocation::Inline {
                                new_messages,
                                ..
                            } => new_messages,
                            allthecodes_skills::invocation::PreparedSkillInvocation::Fork { .. } => {
                                vec![allthecodes_skills::invocation::make_skill_message(
                                    &skill,
                                    &args,
                                    Some(session_id.as_str()),
                                )]
                            }
                        };
                        processed.should_query = true;
                        processed.result_text = None;
                    }
                    None => {
                        local_command.is_error = true;
                        processed.result_text = Some(format!(
                            "Skill /{} is not loaded.",
                            skill_name
                        ));
                        processed.should_query = false;
                        processed.messages.clear();
                    }
                }
            }

            if processed.bash_mode {
                match bash_mode_result_message(&prompt, &config.cwd).await {
                    Ok(message) => {
                        processed.messages = vec![message];
                        processed.should_query = true;
                        processed.result_text = None;
                    }
                    Err(error) => {
                        local_command.is_error = true;
                        processed.result_text = Some(format!(
                            "Bash mode command failed: {}",
                            error
                        ));
                        processed.should_query = false;
                        processed.messages.clear();
                    }
                }
            }

            // A.3: Push processed messages into mutable_messages
            {
                let mut s = state_ref.write();
                for m in &processed.messages {
                    s.messages.push(m.clone());
                }
            }

            // A.4: Persist user message to transcript (fire-and-forget)
            if !processed.messages.is_empty() {
                let _ = transcript::record_transcript(
                    session_id.as_str(),
                    &processed.messages,
                );
            }

            let (tools_snapshot, model_name, backend_name, app_settings) = {
                let s = state_ref.read();
                let tools = s.tools.clone();
                let model = config
                    .user_specified_model
                    .clone()
                    .unwrap_or_else(|| s.app_state.main_loop_model.clone());
                let backend = s.app_state.main_loop_backend.clone();
                let settings = s.app_state.settings.clone();
                (tools, model, backend, settings)
            };
            let capability_tools_snapshot =
                allthecodes_tools::phase5::filter_tools_for_model_capabilities(
                    tools_snapshot,
                    &app_settings,
                    &model_name,
                );
            let session_tools_snapshot =
                allthecodes_tools::registry::filter_tools_for_session_gates(
                    capability_tools_snapshot,
                    allthecodes_tools::registry::ToolSessionGates {
                        non_interactive: query_source.is_non_interactive(),
                        subagent: query_source.starts_with_agent(),
                    },
                );
            let query_gates = crate::types::config::QueryGates::from_env(
                state_ref.read().app_state.fast_mode,
            );
            let prompt_tools_snapshot = if query_gates.deferred_tool_loading {
                let messages = state_ref.read().messages.clone();
                allthecodes_tools::deferred_tools::filter_tools_for_deferred_request(
                    session_tools_snapshot.clone(),
                    &messages,
                    session_id.as_str(),
                )
            } else {
                session_tools_snapshot.clone()
            };

            // ================================================================
            // PHASE C: Pre-Query Setup
            // ================================================================

            // C.1: Yield SystemInit message
            let perm_mode = state_ref
                .read()
                .app_state
                .tool_permission_context
                .mode
                .clone();

            yield SdkMessage::SystemInit(SystemInitMessage {
                tools: prompt_tools_snapshot
                    .iter()
                    .map(|t| t.name().to_string())
                    .collect(),
                model: model_name.clone(),
                permission_mode: format!("{:?}", perm_mode),
                session_id: local_command.session_id.to_string(),
                uuid: Uuid::new_v4(),
            });

            // C.2: If this is a local command, yield result and return immediately.
            if !processed.should_query {
                let local_text = processed
                    .result_text
                    .clone()
                    .unwrap_or_default();
                finish_submit_telemetry(
                    &mut telemetry_submit_span,
                    &model_name,
                    &UsageTracking::default(),
                );

                yield SdkMessage::Result(SdkResult {
                    subtype: if local_command.is_error {
                        ResultSubtype::ErrorDuringExecution
                    } else {
                        ResultSubtype::Success
                    },
                    is_error: local_command.is_error,
                    duration_ms: submit_turn.duration_ms(),
                    duration_api_ms: 0,
                    num_turns: 0,
                    result: local_text.clone(),
                    stop_reason: None,
                    session_id: local_command.session_id.to_string(),
                    total_cost_usd: 0.0,
                    usage: UsageTracking::default(),
                    permission_denials: vec![],
                    structured_output: None,
                    uuid: Uuid::new_v4(),
                    errors: if local_command.is_error {
                        vec![local_text.clone()]
                    } else {
                        vec![]
                    },
                });
                return;
            }

            // ================================================================
            // PHASE B: System Prompt Build
            // ================================================================

            let prompt_build = build_submit_system_prompt(
                &prompt,
                &config,
                &session_id,
                &state_ref,
                &hook_runner,
                &prompt_tools_snapshot,
                &model_name,
                &backend_name,
            )
            .await;

            // ================================================================
            // PHASE D: Query Loop -- full message dispatch
            // ================================================================

            let current_messages = state_ref.read().messages.clone();

            let params = QueryParams {
                messages: current_messages,
                system_prompt: prompt_build.system_prompt_parts,
                user_context: prompt_build.user_context,
                system_context: prompt_build.system_context,
                fallback_model: config.fallback_model.clone(),
                query_source: query_source.clone(),
                max_output_tokens_override: None,
                max_turns: config.max_turns,
                skip_cache_write: None,
                task_budget: config.task_budget.clone(),
                gates: query_gates.clone(),
            };

            // Create API client for the selected backend.
            let mut submit_langfuse_trace = None;
            let api_client: Option<Arc<allthecodes_api::api::client::ApiClient>> =
                allthecodes_api::api::client::ApiClient::from_backend(Some(&backend_name)).map(Arc::new);
            if api_client.is_none() {
                let result = if codex_exec::is_codex_backend(&backend_name) {
                    format!(
                        "Codex backend requires {}. Optionally set {} and {}.",
                        allthecodes_api::api::client::OPENAI_CODEX_TOKEN_ENV,
                        allthecodes_api::api::client::OPENAI_CODEX_BASE_URL_ENV,
                        allthecodes_api::api::client::OPENAI_CODEX_MODEL_ENV
                    )
                } else {
                    "No API provider configured. Set an API key in environment or use /login."
                        .to_string()
                };
                finish_submit_telemetry(
                    &mut telemetry_submit_span,
                    &model_name,
                    &UsageTracking::default(),
                );

                yield SdkMessage::Result(SdkResult {
                    subtype: ResultSubtype::ErrorDuringExecution,
                    is_error: true,
                    duration_ms: submit_turn.duration_ms(),
                    duration_api_ms: 0,
                    num_turns: 0,
                    result: result.clone(),
                    stop_reason: Some("api_error".to_string()),
                    session_id: session_id.to_string(),
                    total_cost_usd: 0.0,
                    usage: UsageTracking::default(),
                    permission_denials: vec![],
                    structured_output: submit_turn.structured_output.clone(),
                    uuid: Uuid::new_v4(),
                    errors: vec![result],
                });
                return;
            }

            if let Some(ref api_client) = api_client {
                let provider = api_client.langfuse_provider_name().to_string();
                submit_langfuse_trace = if let Some(agent_context) = config.agent_context.as_ref() {
                    crate::services::langfuse::create_subagent_trace(
                        &agent_context.langfuse_session_id,
                        agent_context
                            .agent_type
                            .as_deref()
                            .unwrap_or("general-purpose"),
                        &agent_context.agent_id,
                        &model_name,
                        &provider,
                        &prompt,
                    )
                } else {
                    let query_source_label = query_source.as_label();
                    crate::services::langfuse::create_trace(
                        session_id.as_str(),
                        &model_name,
                        &provider,
                        &prompt,
                        Some(query_source_label.as_str()),
                    )
                };
            }

            // Create deps for the inner query loop
            let permission_callback = state_ref.read().permission_callback.clone();
            let permission_event_callback = state_ref.read().permission_event_callback.clone();
            let bg_agent_tx = state_ref.read().bg_agent_tx.clone();
            let tool_progress_callback = state_ref.read().tool_progress_callback.clone();
            let submit_audit_ctx = state_ref.read().audit_ctx.with_submit();
            let deps = Arc::new(QueryEngineDeps {
                aborted: aborted_ref.clone(),
                state: state_ref.clone(),
                cwd: config.cwd.clone(),
                session_id: session_id.to_string(),
                query_source: query_source.clone(),
                audit_ctx: submit_audit_ctx,
                langfuse_trace: submit_langfuse_trace.clone(),
                api_client,
                agent_context: config.agent_context.clone(),
                permission_callback,
                permission_event_callback,
                bg_agent_tx,
                tool_progress_callback,
                pending_bg_results: pending_bg_results.clone(),
                active_steer_state: active_steer_state.clone(),
                hook_runner: hook_runner.clone(),
                command_dispatcher: command_dispatcher.clone(),
                auto_classifier_fn: auto_classifier_fn.clone(),
            });

            // Run the query loop
            let inner_stream = loop_impl::query(params, deps);

            use futures::StreamExt;
            let mut inner_stream = std::pin::pin!(inner_stream);

            let api_started_at = Instant::now();
            let replay_user_messages = query_source == QuerySource::Sdk;

            while let Some(item) = inner_stream.next().await {
                let mut stream_ctx = StreamContext {
                    config: &config,
                    state_ref: &state_ref,
                    session_id: &session_id,
                    submit_turn: &mut submit_turn,
                    replay_user_messages,
                    submit_langfuse_trace: &mut submit_langfuse_trace,
                    telemetry_submit_span: &mut telemetry_submit_span,
                    model_name: &model_name,
                    api_started_at,
                };

                let mut terminated = false;
                for action in process_stream_item(item, &mut stream_ctx) {
                    match action {
                        StreamAction::Yield(message) => yield message,
                        StreamAction::Terminate(result) => {
                            yield SdkMessage::Result(result);
                            terminated = true;
                            break;
                        }
                    }
                }

                if terminated {
                    return;
                }

                if let Some(result) = check_budget(&mut stream_ctx) {
                    yield SdkMessage::Result(result);
                    return;
                }
            } // end while let Some(item)

            // ================================================================
            // PHASE E: Result Generation
            // ================================================================

            let final_messages = state_ref.read().messages.clone();

            let terminal_msg =
                result::find_terminal_message(&final_messages);
            let is_success = result::is_result_successful(
                terminal_msg,
                submit_turn.last_stop_reason.as_deref(),
            );
            let (text_result, is_api_error) =
                result::extract_text_result(&final_messages);

            let (usage_snap, denials_snap) = {
                let s = state_ref.read();
                (s.usage.clone(), s.permission_denials.clone())
            };

            let subtype = if is_success {
                ResultSubtype::Success
            } else {
                ResultSubtype::ErrorDuringExecution
            };

            let mut errors = std::mem::take(&mut submit_turn.collected_errors);
            if is_api_error {
                errors.push(text_result.clone());
            }

            // Record API duration in global ProcessState
            let api_duration_ms = api_started_at.elapsed().as_millis() as u64;
            crate::bootstrap::PROCESS_STATE
                .read()
                .api_duration.record(api_duration_ms);

            // Emit submit.completed audit event
            {
                use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                let ctx = state_ref.read().audit_ctx.clone();
                let outcome = if is_success { Outcome::Completed } else { Outcome::Failed };
                ctx.emit(
                    EventKind::SubmitCompleted,
                    Stage::Submit,
                    AuditLevel::Info,
                    outcome,
                    Some(submit_turn.duration_ms()),
                    Some(serde_json::json!({
                        "num_turns": submit_turn.turn_count_this_submit,
                        "cost_usd": usage_snap.total_cost_usd,
                        "is_error": !is_success,
                    })),
                );
                ctx.flush();
            }

            crate::services::langfuse::end_trace(
                submit_langfuse_trace.take(),
                Some(&text_result),
                if is_success {
                    None
                } else {
                    Some(crate::services::langfuse::TraceStatus::Error)
                },
            );
            finish_submit_telemetry(&mut telemetry_submit_span, &model_name, &usage_snap);

            if let Some(goal_update) =
                account_goal_runtime_message(session_id.as_str(), &usage_snap)
            {
                yield goal_update;
            }

            yield SdkMessage::Result(SdkResult {
                subtype,
                is_error: !is_success,
                duration_ms: submit_turn.duration_ms(),
                duration_api_ms: api_started_at.elapsed().as_millis() as u64,
                num_turns: submit_turn.turn_count_this_submit,
                result: text_result,
                stop_reason: submit_turn.last_stop_reason,
                session_id: session_id.to_string(),
                total_cost_usd: usage_snap.total_cost_usd,
                usage: usage_snap,
                permission_denials: denials_snap,
                structured_output: submit_turn.structured_output,
                uuid: Uuid::new_v4(),
                errors,
            });
        };
        Box::pin(stream)
    }
}
