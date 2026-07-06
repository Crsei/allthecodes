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

use crate::bootstrap::SessionId;
use crate::codex_exec;
use crate::input_processing;
use crate::result;
use crate::session::record_replay::types::{
    MessageRecord, RecordItem, TurnFinishStatus, TurnFinishedRecord, TurnStartedRecord,
};
use crate::types::config::{QueryParams, QuerySource, SubmitContextMode, SubmitMessageOverrides};
use allthecodes_engine::query::loop_impl;
use allthecodes_types::sdk::*;

use super::deps::QueryEngineDeps;
use super::QueryEngine;

mod command_handling;
mod memory_recall;
mod stream_handler;
mod system_prompt_build;
mod transaction;

use command_handling::{bash_mode_result_message, handle_parsed_command, skill_args_from_prompt};
use stream_handler::{
    account_goal_runtime_message, check_budget, maybe_stage_background_review_after_turn,
    prime_goal_runtime_for_session, process_stream_item, QueryTurnEvent, StreamAction,
    StreamContext,
};
use system_prompt_build::build_submit_system_prompt;
use transaction::{SubmitTransaction, SubmitTransactionOutcome};

type SessionRecorderSlot =
    Arc<parking_lot::Mutex<Option<crate::session::record_replay::SessionRecorderHandle>>>;

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

fn normalize_submit_overrides(mut overrides: SubmitMessageOverrides) -> SubmitMessageOverrides {
    overrides.model = overrides
        .model
        .and_then(|value| non_empty_string(value.as_str()));
    overrides.effort = overrides
        .effort
        .and_then(|value| non_empty_string(value.as_str()));
    overrides.allowed_tools = overrides
        .allowed_tools
        .map(|tools| unique_non_empty_strings(tools.into_iter()));
    overrides.skill_ids = overrides
        .skill_ids
        .map(|skills| unique_non_empty_strings(skills.into_iter()));
    overrides.system_prompt_append_parts =
        unique_non_empty_strings(overrides.system_prompt_append_parts.into_iter());
    overrides
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn unique_non_empty_strings(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .filter_map(|value| non_empty_string(value.as_str()))
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn filter_tools_for_submit_overrides(
    tools: crate::types::tool::Tools,
    allowed_tools: Option<&Vec<String>>,
) -> crate::types::tool::Tools {
    let Some(allowed_tools) = allowed_tools else {
        return tools;
    };
    let allowed: std::collections::HashSet<String> = allowed_tools
        .iter()
        .map(|tool| tool.to_ascii_lowercase())
        .collect();
    tools
        .into_iter()
        .filter(|tool| allowed.contains(&tool.name().to_ascii_lowercase()))
        .collect()
}

fn selected_skill_instruction_parts(skill_ids: &[String], session_id: Option<&str>) -> Vec<String> {
    skill_ids
        .iter()
        .filter_map(|skill_id| allthecodes_skills::find_skill(skill_id))
        .map(|skill| {
            format!(
                "# Skill: {}\n\n{}",
                skill.display_name(),
                skill.expand_prompt("", session_id)
            )
        })
        .collect()
}

fn prompt_summary(prompt: &str) -> Option<String> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(200).collect())
}

async fn record_items_best_effort(
    recorder_ref: &SessionRecorderSlot,
    config: &crate::types::config::QueryEngineConfig,
    session_id: &SessionId,
    items: Vec<RecordItem>,
    context: &str,
) {
    if let Err(error) = super::record_session_items(recorder_ref, config, session_id, items).await {
        warn!(session_id = %session_id, %error, context, "failed to record session items");
    }
}

async fn flush_record_best_effort(recorder_ref: &SessionRecorderSlot, session_id: &SessionId) {
    let handle = recorder_ref.lock().clone();
    if let Some(handle) = handle {
        if let Err(error) = handle.flush().await {
            warn!(session_id = %session_id, %error, "failed to flush session record");
        }
    }
}

fn turn_finished_item(status: TurnFinishStatus, error: Option<String>) -> RecordItem {
    turn_finished_item_with_review_ids(status, error, Vec::new())
}

fn turn_finished_item_with_review_ids(
    status: TurnFinishStatus,
    error: Option<String>,
    review_proposal_ids: Vec<String>,
) -> RecordItem {
    RecordItem::TurnFinished(TurnFinishedRecord {
        status,
        abort_reason: None,
        error,
        usage: None,
        review_proposal_ids,
    })
}

async fn commit_submit_transaction_best_effort(
    transaction: SubmitTransaction,
    state_ref: &Arc<parking_lot::RwLock<super::QueryEngineState>>,
    session_recorder: &SessionRecorderSlot,
    config: &crate::types::config::QueryEngineConfig,
    session_id: &SessionId,
) -> Vec<SdkMessage> {
    let outcome = transaction.commit(state_ref, session_id, config);
    commit_submit_transaction_outcome_best_effort(outcome, session_recorder, config, session_id)
        .await
}

async fn commit_submit_transaction_outcome_best_effort(
    outcome: SubmitTransactionOutcome,
    session_recorder: &SessionRecorderSlot,
    config: &crate::types::config::QueryEngineConfig,
    session_id: &SessionId,
) -> Vec<SdkMessage> {
    let SubmitTransactionOutcome {
        emitted_events,
        terminal_result,
        record_items,
        record_context,
        flush_recorder,
    } = outcome;

    if !record_items.is_empty() {
        record_items_best_effort(
            session_recorder,
            config,
            session_id,
            record_items,
            record_context.unwrap_or("submit_transaction"),
        )
        .await;
    }
    if flush_recorder {
        flush_record_best_effort(session_recorder, session_id).await;
    }

    let mut messages = emitted_events;
    if let Some(result) = terminal_result {
        messages.push(SdkMessage::Result(result));
    }
    messages
}

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
        self.submit_message_with_overrides(prompt, query_source, SubmitMessageOverrides::default())
    }

    pub fn submit_message_with_overrides(
        &self,
        prompt: &str,
        query_source: QuerySource,
        overrides: SubmitMessageOverrides,
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
        let overrides = normalize_submit_overrides(overrides);

        let state_ref = self.state.clone();
        let runtime_services = self.runtime_services.clone();
        let active_session_id_ref = self.active_session_id.clone();
        let aborted_ref = self.aborted.clone();
        let active_steer_state = self.active_steer_state.clone();
        let pending_bg_results = self.pending_bg_results.clone();
        let hook_runner = self.hook_runner.clone();
        let command_dispatcher = self.command_dispatcher.clone();
        let command_executor = self.command_executor.clone();
        let auto_classifier_fn = self.auto_classifier_fn.clone();
        let session_recorder = self.session_recorder.clone();

        let stream = async_stream::stream! {
            let _active_steer_guard = super::ActiveSteerGuard::activate(active_steer_state.clone());
            let mut submit_turn = SubmitTurnState::new();
            let submit_id = Uuid::new_v4().to_string();
            let mut telemetry_submit_span =
                start_submit_telemetry(session_id.as_str(), &submit_id);

            // Emit submit.received audit event
            {
                use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                let ctx = state_ref.read().runtime.audit_ctx.with_submit();
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
                                let mut transaction = SubmitTransaction::new();
                                transaction.record_items(
                                    "user_prompt_submit_blocked",
                                    vec![
                                        RecordItem::TurnStarted(TurnStartedRecord {
                                            user_message_uuid: None,
                                            input_summary: prompt_summary(&prompt),
                                        }),
                                        RecordItem::TurnFinished(TurnFinishedRecord {
                                            status: TurnFinishStatus::Interrupted,
                                            abort_reason: Some(reason.clone()),
                                            error: None,
                                            usage: None,
                                            review_proposal_ids: Vec::new(),
                                        }),
                                    ],
                                );
                                transaction.flush_recorder_after_commit();
                                transaction.terminate(SdkResult {
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
                                for message in commit_submit_transaction_best_effort(
                                    transaction,
                                    &state_ref,
                                    &session_recorder,
                                    &config,
                                    &session_id,
                                )
                                .await
                                {
                                    yield message;
                                }
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
            state_ref.write().tools.discovered_skill_names.clear();
            if matches!(query_source, QuerySource::ProactiveTick)
                && state_ref.read().app_state.tool_permission_context.mode
                    == crate::types::tool::PermissionMode::Plan
            {
                allthecodes_types::proactive_context::set_context_blocked(true, "plan_mode");
                let result =
                    "Proactive tick blocked while permission mode is plan_mode.".to_string();
                let telemetry_model = config.user_specified_model.clone().unwrap_or_else(|| {
                    state_ref.read().app_state.main_loop_model.clone()
                });
                finish_submit_telemetry(
                    &mut telemetry_submit_span,
                    &telemetry_model,
                    &UsageTracking::default(),
                );
                let mut transaction = SubmitTransaction::new();
                transaction.record_items(
                    "turn_finished",
                    vec![turn_finished_item(
                        TurnFinishStatus::Errored,
                        Some(result.clone()),
                    )],
                );
                transaction.flush_recorder_after_commit();
                transaction.terminate(SdkResult {
                    subtype: ResultSubtype::ErrorDuringExecution,
                    is_error: true,
                    duration_ms: submit_turn.duration_ms(),
                    duration_api_ms: 0,
                    num_turns: 0,
                    result: result.clone(),
                    stop_reason: Some("context_blocked".to_string()),
                    session_id: session_id.to_string(),
                    total_cost_usd: 0.0,
                    usage: UsageTracking::default(),
                    permission_denials: vec![],
                    structured_output: None,
                    uuid: Uuid::new_v4(),
                    errors: vec![result],
                });
                for message in commit_submit_transaction_best_effort(
                    transaction,
                    &state_ref,
                    &session_recorder,
                    &config,
                    &session_id,
                )
                .await
                {
                    yield message;
                }
                return;
            }

            // A.2: Process user input (delegate to input_processing module)
            let current_msgs_snapshot = state_ref.read().transcript.messages.clone();
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

            // A.3/A.4: Append and persist processed messages through the submit transaction.
            if !processed.messages.is_empty() {
                let mut transaction = SubmitTransaction::new();
                for message in &processed.messages {
                    transaction.append_message(message.clone());
                    transaction.persist(message.clone());
                }
                let mut record_items = vec![RecordItem::TurnStarted(TurnStartedRecord {
                    user_message_uuid: processed
                        .messages
                        .first()
                        .map(|message| message.uuid().to_string()),
                    input_summary: prompt_summary(&prompt),
                })];
                record_items.extend(processed.messages.iter().map(|message| {
                    RecordItem::Message(MessageRecord::from_message(message))
                }));
                transaction.record_items(
                    "processed_input",
                    record_items,
                );
                for message in commit_submit_transaction_best_effort(
                    transaction,
                    &state_ref,
                    &session_recorder,
                    &config,
                    &session_id,
                )
                .await
                {
                    yield message;
                }
            }

            let (tools_snapshot, model_name, backend_name, app_settings) = {
                let s = state_ref.read();
                let tools = s.tools.registry.clone();
                let model = config
                    .user_specified_model
                    .clone()
                    .unwrap_or_else(|| {
                        overrides
                            .model
                            .clone()
                            .unwrap_or_else(|| s.app_state.main_loop_model.clone())
                    });
                let backend = s.app_state.main_loop_backend.clone();
                let settings = s.app_state.settings.clone();
                (tools, model, backend, settings)
            };
            let capability_tools_snapshot =
                allthecodes_tools::media::filter_tools_for_model_capabilities(
                    tools_snapshot,
                    &app_settings,
                    &model_name,
                );
            let settings_tools_snapshot =
                allthecodes_tools::registry::filter_tools_for_runtime_settings(
                    capability_tools_snapshot,
                    &app_settings,
                );
            let session_tools_snapshot = allthecodes_tools::registry::dedupe_tools_by_name(
                allthecodes_tools::registry::filter_tools_for_session_gates(
                    settings_tools_snapshot,
                    allthecodes_tools::registry::ToolSessionGates {
                        non_interactive: query_source.is_non_interactive(),
                        subagent: query_source.starts_with_agent(),
                    },
                ),
            );
            let query_gates = crate::types::config::QueryGates::from_env(
                state_ref.read().app_state.fast_mode,
            );
            let execution_tools_snapshot = filter_tools_for_submit_overrides(
                session_tools_snapshot.clone(),
                overrides.allowed_tools.as_ref(),
            );
            let execution_tools_snapshot =
                allthecodes_tools::registry::dedupe_tools_by_name(execution_tools_snapshot);
            let prompt_tools_snapshot = if query_gates.deferred_tool_loading {
                let messages = state_ref.read().transcript.messages.clone();
                allthecodes_tools::deferred_tools::filter_tools_for_deferred_request(
                    execution_tools_snapshot.clone(),
                    &messages,
                    session_id.as_str(),
                )
            } else {
                execution_tools_snapshot.clone()
            };
            let prompt_tools_snapshot =
                allthecodes_tools::registry::dedupe_tools_by_name(prompt_tools_snapshot);

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
                let mut transaction = SubmitTransaction::new();
                transaction.record_items(
                    "turn_finished",
                    vec![turn_finished_item(
                        if local_command.is_error {
                            TurnFinishStatus::Errored
                        } else {
                            TurnFinishStatus::Completed
                        },
                        local_command.is_error.then(|| local_text.clone()),
                    )],
                );
                transaction.flush_recorder_after_commit();
                transaction.terminate(SdkResult {
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
                for message in commit_submit_transaction_best_effort(
                    transaction,
                    &state_ref,
                    &session_recorder,
                    &config,
                    &session_id,
                )
                .await
                {
                    yield message;
                }
                return;
            }

            // ================================================================
            // PHASE B: System Prompt Build
            // ================================================================

            let mut prompt_build = build_submit_system_prompt(
                &prompt,
                &config,
                &session_id,
                &state_ref,
                &hook_runner,
                &prompt_tools_snapshot,
                &model_name,
                &backend_name,
                &runtime_services,
            )
            .await;
            if let Some(skill_ids) = overrides.skill_ids.as_ref() {
                prompt_build
                    .system_prompt_parts
                    .extend(selected_skill_instruction_parts(skill_ids, Some(session_id.as_str())));
            }
            prompt_build
                .system_prompt_parts
                .extend(overrides.system_prompt_append_parts.clone());

            // ================================================================
            // PHASE D: Query Loop -- full message dispatch
            // ================================================================

            let current_messages = match overrides.context_mode.unwrap_or(SubmitContextMode::Inherit) {
                SubmitContextMode::Inherit => state_ref.read().transcript.messages.clone(),
                SubmitContextMode::Compact => {
                    let messages = state_ref.read().transcript.messages.clone();
                    let compacted =
                        crate::compact::pipeline::try_reactive_compact(messages.clone(), &model_name)
                        .await
                        .map(|result| result.messages);
                    if compacted.is_some() {
                        allthecodes_types::proactive_context::set_context_blocked(
                            false,
                            "context_ready",
                        );
                    }
                    compacted.unwrap_or(messages)
                }
                SubmitContextMode::Isolated => processed.messages.clone(),
            };

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
                runtime_services.model_client_factory.client_for_backend(Some(&backend_name));
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
                let mut transaction = SubmitTransaction::new();
                transaction.record_items(
                    "turn_finished",
                    vec![turn_finished_item(
                        TurnFinishStatus::Errored,
                        Some(result.clone()),
                    )],
                );
                transaction.flush_recorder_after_commit();
                transaction.terminate(SdkResult {
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
                for message in commit_submit_transaction_best_effort(
                    transaction,
                    &state_ref,
                    &session_recorder,
                    &config,
                    &session_id,
                )
                .await
                {
                    yield message;
                }
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
            let permission_callback = state_ref.read().permissions.permission_callback.clone();
            let permission_event_callback = state_ref.read().permissions.permission_event_callback.clone();
            let bg_agent_tx = state_ref.read().runtime.bg_agent_tx.clone();
            let tool_progress_callback = state_ref.read().runtime.tool_progress_callback.clone();
            let submit_audit_ctx = state_ref.read().runtime.audit_ctx.with_submit();
            let deps = Arc::new(QueryEngineDeps {
                aborted: aborted_ref.clone(),
                state: state_ref.clone(),
                runtime_services: runtime_services.clone(),
                cwd: config.cwd.clone(),
                session_id: session_id.to_string(),
                query_source: query_source.clone(),
                audit_ctx: submit_audit_ctx,
                langfuse_trace: submit_langfuse_trace.clone(),
                api_client,
                session_recorder: session_recorder.clone(),
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
                submit_overrides: overrides.clone(),
                submit_tools: None,
            });

            prime_goal_runtime_for_session(session_id.as_str(), &state_ref);

            // Run the query loop
            let inner_stream = loop_impl::query(params, deps);

            use futures::StreamExt;
            let mut inner_stream = std::pin::pin!(inner_stream);

            let api_started_at = Instant::now();
            let replay_user_messages = query_source == QuerySource::Sdk;
            let mut current_request_event: Option<crate::types::message::RequestStartEvent> = None;

            while let Some(item) = inner_stream.next().await {
                let turn_event = QueryTurnEvent::from(item);
                let record_items =
                    turn_event.record_items(&backend_name, &model_name);
                if !record_items.is_empty() {
                    record_items_best_effort(
                        &session_recorder,
                        &config,
                        &session_id,
                        record_items,
                        "query_yield",
                    )
                    .await;
                }
                if let QueryTurnEvent::RequestStart(request_event) = &turn_event {
                    current_request_event = Some(request_event.clone());
                }

                let mut stream_ctx = StreamContext {
                    config: &config,
                    state_ref: &state_ref,
                    session_id: &session_id,
                    submit_turn: &mut submit_turn,
                    replay_user_messages,
                    submit_langfuse_trace: &mut submit_langfuse_trace,
                    telemetry_submit_span: &mut telemetry_submit_span,
                    model_name: &model_name,
                    backend_name: &backend_name,
                    request_event: current_request_event.as_ref(),
                    api_started_at,
                };

                let mut terminated = false;
                for action in process_stream_item(turn_event, &mut stream_ctx) {
                    match action {
                        StreamAction::Yield(message) => yield message,
                        StreamAction::Terminate(result) => {
                            let mut transaction = SubmitTransaction::new();
                            transaction.record_items(
                                "turn_finished",
                                vec![turn_finished_item(
                                    if result.is_error {
                                        TurnFinishStatus::Errored
                                    } else {
                                        TurnFinishStatus::Completed
                                    },
                                    result.is_error.then(|| result.result.clone()),
                                )],
                            );
                            transaction.flush_recorder_after_commit();
                            transaction.terminate(result);
                            for message in commit_submit_transaction_best_effort(
                                transaction,
                                &state_ref,
                                &session_recorder,
                                &config,
                                &session_id,
                            )
                            .await
                            {
                                yield message;
                            }
                            terminated = true;
                            break;
                        }
                    }
                }

                if terminated {
                    return;
                }

                if let Some(stop) = check_budget(&mut stream_ctx) {
                    let mut transaction = SubmitTransaction::new();
                    if let Some(goal_update) = stop.goal_update {
                        transaction.emit(goal_update);
                    }
                    transaction.record_items(
                        "turn_finished",
                        vec![turn_finished_item(
                            if stop.result.is_error {
                                TurnFinishStatus::Errored
                            } else {
                                TurnFinishStatus::Completed
                            },
                            stop.result.is_error.then(|| stop.result.result.clone()),
                        )],
                    );
                    transaction.flush_recorder_after_commit();
                    transaction.terminate(stop.result);
                    for message in commit_submit_transaction_best_effort(
                        transaction,
                        &state_ref,
                        &session_recorder,
                        &config,
                        &session_id,
                    )
                    .await
                    {
                        yield message;
                    }
                    return;
                }
            } // end while let Some(item)

            // ================================================================
            // PHASE E: Result Generation
            // ================================================================

            let final_messages = state_ref.read().transcript.messages.clone();

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
                (s.transcript.usage.clone(), s.permissions.denials.clone())
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
                let ctx = state_ref.read().runtime.audit_ctx.clone();
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

            let mut transaction = SubmitTransaction::new();
            if let Some(goal_update) =
                account_goal_runtime_message(session_id.as_str(), &state_ref, None)
            {
                transaction.emit(goal_update);
            }
            let review_proposal_ids = maybe_stage_background_review_after_turn(
                &config,
                &state_ref,
                &session_id,
                submit_turn.turn_count_this_submit,
                &text_result,
                !is_success,
            );
            transaction.record_items(
                "turn_finished",
                vec![turn_finished_item_with_review_ids(
                    if is_success {
                        TurnFinishStatus::Completed
                    } else {
                        TurnFinishStatus::Errored
                    },
                    (!is_success).then(|| text_result.clone()),
                    review_proposal_ids,
                )],
            );
            transaction.flush_recorder_after_commit();
            transaction.terminate(SdkResult {
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
            for message in commit_submit_transaction_best_effort(
                transaction,
                &state_ref,
                &session_recorder,
                &config,
                &session_id,
            )
            .await
            {
                yield message;
            }
        };
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_submit_overrides_keeps_system_prompt_append_parts_separate() {
        let overrides = normalize_submit_overrides(SubmitMessageOverrides {
            system_prompt_append_parts: vec![
                "  <mode>Focus</mode>  ".to_string(),
                "".to_string(),
                "<mode>Focus</mode>".to_string(),
                "<mode>Review</mode>".to_string(),
            ],
            ..Default::default()
        });

        assert_eq!(
            overrides.system_prompt_append_parts,
            vec![
                "<mode>Focus</mode>".to_string(),
                "<mode>Review</mode>".to_string()
            ]
        );
    }
}
