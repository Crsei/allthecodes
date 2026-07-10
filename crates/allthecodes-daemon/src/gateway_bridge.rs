//! Bridge between the remote-control gateway runner and daemon worker protocol.
//!
//! The gateway crate owns run policy and durable run metadata. This module is
//! the daemon-side adapter that maps gateway-neutral commands onto existing
//! worker command files without making `gateway` depend on `allthecodes`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use allthecodes_gateway::{
    GatewayCommand, GatewayCommandKind, GatewayCommandReceipt, GatewayCommandSink,
    GatewayDiagnostic, GatewayError, GatewayStore, RunEventKind, RunId, RunStatus,
    SessionKeyPolicy,
};
use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionRequestPayload, PermissionResponsePayload,
};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tokio_stream::StreamExt;

use crate::protocol::{self, DaemonCommandKind, DaemonEventKind};
use allthecodes_engine::bootstrap::SessionId;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::config::{QueryEngineConfig, QuerySource};
use allthecodes_types::brief::BriefMessagePayload;
use allthecodes_types::sdk::SdkMessage;

use super::gateway_run_events::{
    append_gateway_event, append_gateway_sdk_event, update_gateway_status,
};
use super::routes::sdk_message_to_sse;
use super::supervisor::ASSISTANT_WORKER_ID;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayDaemonBridge {
    target_worker_id: String,
    bridge_session_id: Option<String>,
    assistant_session_id: Option<String>,
}

impl GatewayDaemonBridge {
    pub fn assistant_worker() -> Self {
        Self {
            target_worker_id: ASSISTANT_WORKER_ID.to_string(),
            bridge_session_id: None,
            assistant_session_id: None,
        }
    }

    pub fn for_worker(target_worker_id: impl Into<String>) -> Self {
        Self {
            target_worker_id: target_worker_id.into(),
            bridge_session_id: None,
            assistant_session_id: None,
        }
    }

    pub fn for_worker_with_session(
        target_worker_id: impl Into<String>,
        bridge_session_id: Option<String>,
        assistant_session_id: Option<String>,
    ) -> Self {
        Self {
            target_worker_id: target_worker_id.into(),
            bridge_session_id,
            assistant_session_id,
        }
    }

    pub fn target_worker_id(&self) -> &str {
        &self.target_worker_id
    }
}

impl GatewayCommandSink for GatewayDaemonBridge {
    fn dispatch(&self, command: GatewayCommand) -> Result<GatewayCommandReceipt, GatewayError> {
        let kind = daemon_kind(command.kind);
        let payload = daemon_payload(
            &command,
            self.bridge_session_id.as_deref(),
            self.assistant_session_id.as_deref(),
        );
        if command.kind == GatewayCommandKind::Submit && should_wake_sleep_for_payload(&payload) {
            crate::process_state::clear_sleep_state().map_err(|error| {
                GatewayError::new(
                    GatewayDiagnostic::new(
                        "daemon_sleep_clear_failed",
                        "The daemon bridge could not clear proactive sleep before dispatching work.",
                        "Check daemon state directory permissions and retry the request.",
                    )
                    .with_context(format!("run_id={}, error={error:#}", command.run_id)),
                )
            })?;
            let _ = mirror_automation_state_from_run_id(&command.run_id, &self.target_worker_id);
        }
        let queued = super::protocol_store()
            .enqueue_command(
                &self.target_worker_id,
                kind,
                payload,
                command.idempotency_key.clone(),
            )
            .map_err(|error| enqueue_error(&command, error))?;

        Ok(GatewayCommandReceipt {
            command_id: queued.command_id,
            target: queued.target_worker_id,
        })
    }
}

fn should_wake_sleep_for_payload(payload: &serde_json::Value) -> bool {
    payload.get("source").and_then(serde_json::Value::as_str) != Some("proactive_tick")
}

fn daemon_kind(kind: GatewayCommandKind) -> DaemonCommandKind {
    match kind {
        GatewayCommandKind::Submit => DaemonCommandKind::Submit,
        GatewayCommandKind::Abort => DaemonCommandKind::Abort,
        GatewayCommandKind::PermissionResponse => DaemonCommandKind::PermissionResponse,
        GatewayCommandKind::AskUserResponse => DaemonCommandKind::AskUserResponse,
    }
}

fn daemon_payload(
    command: &GatewayCommand,
    bridge_session_id: Option<&str>,
    assistant_session_id: Option<&str>,
) -> Value {
    match command.kind {
        GatewayCommandKind::Submit => {
            let text = command
                .payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut payload = command.payload.clone();
            if let Some(object) = payload.as_object_mut() {
                object.insert("text".to_string(), json!(text));
                object.insert(
                    "idempotencyKey".to_string(),
                    json!(command.idempotency_key.clone()),
                );
                object.insert(
                    "gateway".to_string(),
                    gateway_context(command, bridge_session_id, assistant_session_id),
                );
                payload
            } else {
                json!({
                    "text": text,
                    "idempotencyKey": command.idempotency_key.clone(),
                    "gateway": gateway_context(command, bridge_session_id, assistant_session_id),
                })
            }
        }
        GatewayCommandKind::Abort | GatewayCommandKind::AskUserResponse => {
            let mut payload = command.payload.clone();
            if let Some(object) = payload.as_object_mut() {
                if command.kind == GatewayCommandKind::AskUserResponse {
                    if let Some(question_id) = payload_string(
                        &command.payload,
                        &["request_id", "questionId", "question_id", "id"],
                    ) {
                        object.insert("request_id".to_string(), json!(question_id));
                    }
                    if let Some(answer) =
                        payload_string(&command.payload, &["answer", "response", "text"])
                    {
                        object.insert("answer".to_string(), json!(answer));
                    }
                }
                object.insert(
                    "gateway".to_string(),
                    gateway_context(command, bridge_session_id, assistant_session_id),
                );
                payload
            } else {
                json!({
                    "value": payload,
                    "gateway": gateway_context(command, bridge_session_id, assistant_session_id),
                })
            }
        }
        GatewayCommandKind::PermissionResponse => {
            let mut payload = command.payload.clone();
            if let Some(object) = payload.as_object_mut() {
                if let Some(tool_use_id) = payload_string(
                    &command.payload,
                    &["tool_use_id", "toolUseId", "request_id", "id"],
                ) {
                    object.insert("tool_use_id".to_string(), json!(tool_use_id));
                }
                let decision =
                    payload_string(&command.payload, &["decision"]).unwrap_or_else(|| {
                        if command
                            .payload
                            .get("approved")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            "allow".to_string()
                        } else {
                            "deny".to_string()
                        }
                    });
                object.insert("decision".to_string(), json!(decision));
                object.insert(
                    "gateway".to_string(),
                    gateway_context(command, bridge_session_id, assistant_session_id),
                );
                payload
            } else {
                json!({
                    "value": payload,
                    "gateway": gateway_context(command, bridge_session_id, assistant_session_id),
                })
            }
        }
    }
}

fn gateway_context(
    command: &GatewayCommand,
    bridge_session_id: Option<&str>,
    assistant_session_id: Option<&str>,
) -> Value {
    let mut context = json!({
        "runId": command.run_id.clone(),
        "sessionKey": command.session_key.clone(),
    });
    if let Some(object) = context.as_object_mut() {
        if let Some(bridge_session_id) = bridge_session_id {
            object.insert("bridgeSessionId".to_string(), json!(bridge_session_id));
        }
        if let Some(assistant_session_id) = assistant_session_id {
            object.insert(
                "assistantSessionId".to_string(),
                json!(assistant_session_id),
            );
        }
    }
    context
}

fn enqueue_error(command: &GatewayCommand, error: anyhow::Error) -> GatewayError {
    GatewayError::new(
        GatewayDiagnostic::new(
            "daemon_command_enqueue_failed",
            "The daemon bridge could not enqueue the gateway command.",
            "Check daemon command directory permissions and worker state.",
        )
        .with_context(format!(
            "run_id={}, kind={:?}, error={:#}",
            command.run_id, command.kind, error
        )),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionDelivery {
    Delivered,
    StoredForReplay,
}

#[derive(Default)]
struct AssistantInteractionState {
    inner: Mutex<AssistantInteractionStateInner>,
}

#[derive(Default)]
struct AssistantInteractionStateInner {
    pending_permissions: HashMap<String, oneshot::Sender<PermissionResponsePayload>>,
    replay_permissions: HashMap<String, PermissionResponsePayload>,
    pending_questions: HashMap<String, oneshot::Sender<String>>,
    replay_questions: HashMap<String, String>,
}

impl AssistantInteractionState {
    async fn request_permission(
        &self,
        worker_id: &str,
        request: PermissionRequestPayload,
    ) -> PermissionResponsePayload {
        let tool_use_id = request.tool_use_id.clone();
        let _ = super::protocol_store().append_event(
            worker_id,
            None,
            DaemonEventKind::PermissionRequest.as_str(),
            json!({
                "request_id": tool_use_id,
                "tool_use_id": request.tool_use_id,
                "tool_name": request.tool_name,
                "input": request.tool_input,
                "message": request.message,
                "options": request.options,
                "operation": request.operation,
            }),
        );

        let (tx, rx) = oneshot::channel();
        let replay = {
            let mut inner = self.inner.lock();
            if let Some(response) = inner.replay_permissions.remove(&tool_use_id) {
                Some(response)
            } else {
                inner.pending_permissions.insert(tool_use_id.clone(), tx);
                None
            }
        };
        if let Some(response) = replay {
            return response;
        }

        rx.await
            .unwrap_or_else(|_| PermissionResponsePayload::deny())
    }

    async fn ask_user(&self, worker_id: &str, request: AskUserRequestPayload) -> String {
        let question_id = uuid::Uuid::new_v4().to_string();
        let _ = super::protocol_store().append_event(
            worker_id,
            None,
            DaemonEventKind::AskUserQuestion.as_str(),
            json!({
                "request_id": question_id,
                "id": question_id,
                "question": request.question,
                "choices": request.choices,
                "allow_free_text": request.allow_free_text,
            }),
        );

        let (tx, rx) = oneshot::channel();
        let replay = {
            let mut inner = self.inner.lock();
            if let Some(answer) = inner.replay_questions.remove(&question_id) {
                Some(answer)
            } else {
                inner.pending_questions.insert(question_id.clone(), tx);
                None
            }
        };
        if let Some(answer) = replay {
            return answer;
        }

        rx.await.unwrap_or_default()
    }

    fn complete_permission(
        &self,
        tool_use_id: String,
        response: PermissionResponsePayload,
    ) -> InteractionDelivery {
        let mut inner = self.inner.lock();
        if let Some(tx) = inner.pending_permissions.remove(&tool_use_id) {
            match tx.send(response) {
                Ok(()) => InteractionDelivery::Delivered,
                Err(response) => {
                    inner.replay_permissions.insert(tool_use_id, response);
                    InteractionDelivery::StoredForReplay
                }
            }
        } else {
            inner.replay_permissions.insert(tool_use_id, response);
            InteractionDelivery::StoredForReplay
        }
    }

    fn complete_question(&self, question_id: String, answer: String) -> InteractionDelivery {
        let mut inner = self.inner.lock();
        if let Some(tx) = inner.pending_questions.remove(&question_id) {
            match tx.send(answer) {
                Ok(()) => InteractionDelivery::Delivered,
                Err(answer) => {
                    inner.replay_questions.insert(question_id, answer);
                    InteractionDelivery::StoredForReplay
                }
            }
        } else {
            inner.replay_questions.insert(question_id, answer);
            InteractionDelivery::StoredForReplay
        }
    }
}

pub async fn handle_worker_command(
    worker_id: &str,
    runtime: &AssistantWorkerRuntime,
    command: protocol::DaemonCommand,
) -> Result<bool> {
    let store = super::protocol_store();
    match command.kind {
        protocol::DaemonCommandKind::Submit => {
            if let Err(err) = runtime.execute_submit(worker_id, &command).await {
                append_gateway_event(
                    &command,
                    RunEventKind::Diagnostic {
                        diagnostic: GatewayDiagnostic::new(
                            "daemon_command_failed",
                            "The daemon worker failed while executing the gateway run.",
                            "Inspect daemon worker events and retry if the run is recoverable.",
                        )
                        .with_context(format!("command_id={}, error={err:#}", command.command_id)),
                    },
                )?;
                update_gateway_status(&command, RunStatus::Failed)?;
                store.append_event(
                    worker_id,
                    Some(&command.command_id),
                    "command_failed",
                    json!({
                        "kind": "submit",
                        "error": err.to_string(),
                    }),
                )?;
                let command = store.mark_command_failed(command, err.to_string())?;
                mirror_automation_state(&command, worker_id)?;
            } else {
                let command = store.mark_command_handled(command)?;
                mirror_automation_state(&command, worker_id)?;
            }
            Ok(false)
        }
        protocol::DaemonCommandKind::Abort => {
            runtime.abort();
            let command = store.mark_command_handled(command)?;
            store.append_event(
                worker_id,
                Some(&command.command_id),
                "abort_ack",
                json!({ "handled": true }),
            )?;
            Ok(false)
        }
        protocol::DaemonCommandKind::PermissionResponse
        | protocol::DaemonCommandKind::Resize
        | protocol::DaemonCommandKind::ReloadConfig => {
            if command.kind == protocol::DaemonCommandKind::PermissionResponse {
                runtime.handle_permission_response(&command)?;
            }
            let command = store.mark_command_handled(command)?;
            store.append_event(
                worker_id,
                Some(&command.command_id),
                "command_handled",
                json!({ "kind": command.kind.as_str() }),
            )?;
            Ok(false)
        }
        protocol::DaemonCommandKind::AskUserResponse => {
            runtime.handle_ask_user_response(&command)?;
            let command = store.mark_command_handled(command)?;
            store.append_event(
                worker_id,
                Some(&command.command_id),
                "command_handled",
                json!({ "kind": command.kind.as_str() }),
            )?;
            Ok(false)
        }
        protocol::DaemonCommandKind::Shutdown => {
            let command = store.mark_command_handled(command)?;
            store.append_event(
                worker_id,
                Some(&command.command_id),
                "worker_shutdown_ack",
                json!({ "handled": true }),
            )?;
            Ok(true)
        }
    }
}

#[derive(Clone)]
pub struct AssistantWorkerRuntime {
    engine: Arc<QueryEngine>,
    interactions: Arc<AssistantInteractionState>,
}

impl AssistantWorkerRuntime {
    pub fn new(cwd: &Path) -> Result<Self> {
        crate::runtime::init_plugins()?;
        let tools = crate::runtime::active_tools()?;
        allthecodes_tools::runtime::tool_search::install_runtime_tool_catalog(&tools);
        let command_names = crate::runtime::command_names()?;
        let mut engine = QueryEngine::new(QueryEngineConfig {
            cwd: cwd.to_string_lossy().into_owned(),
            tools,
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: command_names,
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: true,
            resolved_model: None,
            auto_save_session: true,
            agent_context: None,
        });
        engine.set_hook_runner(Arc::new(allthecodes_tools::hooks::ShellHookRunner::new()));
        engine.set_command_dispatcher(crate::runtime::command_dispatcher()?);
        engine.set_command_executor(crate::runtime::command_executor()?);
        let interactions = Arc::new(AssistantInteractionState::default());
        install_interaction_callbacks(&engine, ASSISTANT_WORKER_ID, interactions.clone());

        Ok(Self {
            engine: Arc::new(engine),
            interactions,
        })
    }

    fn handle_permission_response(&self, command: &protocol::DaemonCommand) -> Result<()> {
        let tool_use_id = payload_string(
            &command.payload,
            &["tool_use_id", "toolUseId", "request_id", "id"],
        )
        .context("permission response payload missing tool_use_id")?;
        let decision =
            payload_string(&command.payload, &["decision"]).unwrap_or_else(|| "deny".to_string());
        let feedback = payload_string(&command.payload, &["feedback", "reason"]);
        let delivery = self.interactions.complete_permission(
            tool_use_id.clone(),
            PermissionResponsePayload::new(decision, feedback),
        );
        super::protocol_store().append_event(
            &command.target_worker_id,
            Some(&command.command_id),
            "permission_response",
            json!({
                "tool_use_id": tool_use_id,
                "delivery": interaction_delivery_name(delivery),
            }),
        )?;
        Ok(())
    }

    fn handle_ask_user_response(&self, command: &protocol::DaemonCommand) -> Result<()> {
        let question_id = payload_string(&command.payload, &["request_id", "id", "question_id"])
            .context("ask-user response payload missing request_id")?;
        let answer = payload_string(&command.payload, &["text", "answer"]).unwrap_or_default();
        let delivery = self
            .interactions
            .complete_question(question_id.clone(), answer);
        super::protocol_store().append_event(
            &command.target_worker_id,
            Some(&command.command_id),
            "ask_user_response",
            json!({
                "request_id": question_id,
                "delivery": interaction_delivery_name(delivery),
            }),
        )?;
        Ok(())
    }

    fn apply_assistant_session_context(
        &self,
        worker_id: &str,
        command: &protocol::DaemonCommand,
    ) -> Result<()> {
        let bridge_session_id =
            gateway_payload_string(command, &["bridgeSessionId", "bridge_session_id"]);
        let requested_session_id =
            gateway_payload_string(command, &["assistantSessionId", "assistant_session_id"]);

        let active_session_id = if let Some(requested_session_id) = requested_session_id {
            match allthecodes_session::resume::resume_session_detail(&requested_session_id) {
                Ok(resumed) => {
                    self.engine.replace_messages(resumed.messages);
                    self.engine
                        .set_current_session_id(SessionId::from_string(&requested_session_id));
                    requested_session_id
                }
                Err(error) => {
                    let replacement = self.engine.start_new_session();
                    super::protocol_store().append_event(
                        worker_id,
                        Some(&command.command_id),
                        "assistant_session_resume_fallback",
                        json!({
                            "requested_session_id": requested_session_id,
                            "replacement_session_id": replacement.to_string(),
                            "error": error.to_string(),
                        }),
                    )?;
                    replacement.to_string()
                }
            }
        } else {
            self.engine.current_session_id().to_string()
        };

        if let Some(bridge_session_id) = bridge_session_id {
            persist_bridge_assistant_session_id(&bridge_session_id, &active_session_id)?;
        }
        Ok(())
    }

    async fn execute_submit(
        &self,
        worker_id: &str,
        command: &protocol::DaemonCommand,
    ) -> Result<()> {
        let text = command
            .payload
            .get("text")
            .and_then(|value| value.as_str())
            .context("submit command payload missing text")?;
        let message_id = command
            .payload
            .get("message_id")
            .and_then(|value| value.as_str())
            .unwrap_or(&command.command_id);
        let query_source = query_source_from_submit_payload(&command.payload);
        let query_source_label = query_source.as_label();

        self.apply_assistant_session_context(worker_id, command)?;

        super::protocol_store().append_event(
            worker_id,
            Some(&command.command_id),
            "submit_started",
            json!({
                "message_id": message_id,
                "source": command.payload.get("source").cloned().unwrap_or(Value::Null),
                "query_source": query_source_label,
                "gateway": command.payload.get("gateway").cloned(),
            }),
        )?;
        append_gateway_event(
            command,
            RunEventKind::Custom {
                name: "submit_started".to_string(),
                payload: json!({ "messageId": message_id }),
            },
        )?;
        update_gateway_status(command, RunStatus::Running)?;
        mirror_automation_state(command, worker_id)?;

        self.engine.wake_up();
        let stream = self.engine.submit_message(text, query_source);
        tokio::pin!(stream);
        while let Some(sdk_msg) = stream.next().await {
            append_brief_memory_log_if_needed(&sdk_msg);
            if let Some(event) = sdk_message_to_sse(&sdk_msg, message_id) {
                append_gateway_sdk_event(command, &event.event_type, event.data.clone())?;
                super::protocol_store().append_event(
                    worker_id,
                    Some(&command.command_id),
                    &event.event_type,
                    event.data,
                )?;
            }
        }

        super::protocol_store().append_event(
            worker_id,
            Some(&command.command_id),
            "submit_completed",
            json!({ "message_id": message_id }),
        )?;
        append_gateway_event(
            command,
            RunEventKind::Custom {
                name: "submit_completed".to_string(),
                payload: json!({ "messageId": message_id }),
            },
        )?;
        update_gateway_status(command, RunStatus::Completed)?;
        Ok(())
    }

    pub(crate) fn abort(&self) {
        self.engine.abort();
    }
}

fn append_brief_memory_log_if_needed(sdk_msg: &SdkMessage) {
    if let SdkMessage::BriefMessage(brief) = sdk_msg {
        super::memory_log::append_log_entry(&brief_memory_log_entry(brief));
    }
}

fn brief_memory_log_entry(brief: &BriefMessagePayload) -> String {
    let mut meta = vec![format!("status={}", brief.status.as_str())];
    if let Some(level) = brief.level {
        meta.push(format!("level={}", level.as_str()));
    }
    if let Some(session_id) = brief
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        meta.push(format!("session={session_id}"));
    }
    if let Some(tool_use_id) = brief
        .tool_use_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        meta.push(format!("tool_use={tool_use_id}"));
    }

    let mut entry = format!("brief output [{}]: {}", meta.join(", "), brief.message);
    if !brief.attachments.is_empty() {
        entry.push_str("\nattachments: ");
        entry.push_str(&brief.attachments.join(", "));
    }
    entry
}

fn persist_bridge_assistant_session_id(
    bridge_session_id: &str,
    assistant_session_id: &str,
) -> Result<()> {
    let Some(mut state) = crate::process_state::read_bridge_session_state(bridge_session_id)?
    else {
        return Ok(());
    };
    state.assistant_session_id = Some(assistant_session_id.to_string());
    crate::process_state::write_bridge_session_state(&state)?;
    Ok(())
}

fn mirror_automation_state(command: &protocol::DaemonCommand, worker_id: &str) -> Result<()> {
    let patch = automation_state_patch();
    let Some(run_id) = gateway_payload_string(command, &["runId", "run_id"]) else {
        super::protocol_store().append_event(
            worker_id,
            Some(&command.command_id),
            "automation_state",
            patch,
        )?;
        return Ok(());
    };
    merge_automation_state_metadata(&run_id, patch)
}

fn mirror_automation_state_from_run_id(run_id: &str, worker_id: &str) -> Result<()> {
    let patch = automation_state_patch();
    if run_id.trim().is_empty() {
        super::protocol_store().append_event(worker_id, None, "automation_state", patch)?;
        return Ok(());
    }
    merge_automation_state_metadata(run_id, patch)
}

fn automation_state_patch() -> Value {
    let automation = crate::automation_state::snapshot_from_process_state();
    json!({
        "automation_state": automation.external_metadata()
    })
}

fn merge_automation_state_metadata(run_id: &str, patch: Value) -> Result<()> {
    let run_id = RunId::from_string(run_id.to_string()).map_err(|error| anyhow::anyhow!(error))?;
    GatewayStore::default_with_policy(SessionKeyPolicy::default())
        .merge_run_metadata(&run_id, patch)
        .context("merge gateway automation metadata")?;
    Ok(())
}

fn install_interaction_callbacks(
    engine: &QueryEngine,
    worker_id: &str,
    interactions: Arc<AssistantInteractionState>,
) {
    let permission_worker_id = worker_id.to_string();
    let permission_interactions = interactions.clone();
    engine.set_permission_callback(Arc::new(move |request| {
        let worker_id = permission_worker_id.clone();
        let interactions = permission_interactions.clone();
        Box::pin(async move { interactions.request_permission(&worker_id, request).await })
    }));

    let question_worker_id = worker_id.to_string();
    let question_interactions = interactions;
    engine.set_ask_user_callback(Arc::new(move |request| {
        let worker_id = question_worker_id.clone();
        let interactions = question_interactions.clone();
        Box::pin(async move { interactions.ask_user(&worker_id, request).await })
    }));
}

fn payload_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        payload
            .get(*key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn gateway_payload_string(command: &protocol::DaemonCommand, keys: &[&str]) -> Option<String> {
    command
        .payload
        .get("gateway")
        .and_then(|gateway| payload_string(gateway, keys))
}

fn query_source_from_submit_payload(payload: &Value) -> QuerySource {
    match payload.get("source").and_then(Value::as_str) {
        Some("proactive_tick") => QuerySource::ProactiveTick,
        Some("scheduled_task") => QuerySource::ScheduledTask,
        Some("webhook_event") => QuerySource::WebhookEvent,
        Some("channel_notification") => QuerySource::ChannelNotification,
        Some("channel") => query_source_from_channel_payload(payload),
        Some("http") | Some("worker") | None => QuerySource::ReplMainThread,
        Some(_) => QuerySource::ReplMainThread,
    }
}

fn query_source_from_channel_payload(payload: &Value) -> QuerySource {
    match payload
        .get("channel")
        .and_then(|channel| channel.get("origin"))
        .and_then(|origin| origin.get("type"))
        .and_then(Value::as_str)
    {
        Some("webhook") => QuerySource::WebhookEvent,
        Some("mcp") | None => QuerySource::ChannelNotification,
        Some(_) => QuerySource::ChannelNotification,
    }
}

fn interaction_delivery_name(delivery: InteractionDelivery) -> &'static str {
    match delivery {
        InteractionDelivery::Delivered => "delivered",
        InteractionDelivery::StoredForReplay => "stored_for_replay",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_gateway::{GatewayStore, SessionKeyPolicy};
    use allthecodes_types::message::{Message, MessageContent, UserMessage};
    use chrono::Utc;
    use serial_test::serial;
    use std::path::Path;
    use std::sync::Arc;
    use uuid::Uuid;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct FeatureGuard;

    impl FeatureGuard {
        fn enable_proactive() -> Self {
            allthecodes_config::features::set_runtime_override(
                allthecodes_config::features::FeatureFlags {
                    proactive: true,
                    ..Default::default()
                },
            );
            Self
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            allthecodes_config::features::clear_runtime_override();
        }
    }

    fn bridge_state(
        cwd: &Path,
        session_id: &str,
    ) -> crate::process_state::DaemonBridgeSessionState {
        let identity = crate::bridge_session::BridgeSessionIdentity {
            cwd: cwd.to_path_buf(),
            account_id: None,
            profile: None,
            terminal_id: None,
            remote_session_key: None,
        };
        crate::process_state::DaemonBridgeSessionState {
            schema_version: 2,
            session_id: session_id.to_string(),
            account_id: None,
            profile: None,
            cwd: cwd.to_path_buf(),
            assistant_worker_id: "assistant-session-1".to_string(),
            last_poll_cursor: None,
            last_ack_at: None,
            updated_at: Utc::now(),
            workspace_key: crate::bridge_session::derive_workspace_key(&identity).unwrap(),
            terminal_id: None,
            remote_session_key: None,
            assistant_session_id: None,
            last_run_id: None,
            lease_owner: None,
            lease_expires_at: None,
        }
    }

    fn test_runtime(cwd: &Path) -> AssistantWorkerRuntime {
        let engine = QueryEngine::new(QueryEngineConfig {
            cwd: cwd.to_string_lossy().into_owned(),
            tools: Vec::new(),
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: Vec::new(),
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        });
        AssistantWorkerRuntime {
            engine: Arc::new(engine),
            interactions: Arc::new(AssistantInteractionState::default()),
        }
    }

    fn submit_command(payload: Value) -> protocol::DaemonCommand {
        let now = Utc::now();
        protocol::DaemonCommand {
            schema_version: protocol::SCHEMA_VERSION,
            command_id: "cmd-1".to_string(),
            idempotency_key: None,
            target_worker_id: "assistant-session-1".to_string(),
            kind: DaemonCommandKind::Submit,
            payload,
            status: protocol::DaemonCommandStatus::Acked,
            created_at: now,
            updated_at: now,
            acked_at: Some(now),
            handled_at: None,
            error: None,
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn submit_payload_source_maps_proactive_tick() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "source": "proactive_tick" })),
            QuerySource::ProactiveTick
        );
    }

    #[test]
    fn submit_payload_source_maps_scheduled_task() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "source": "scheduled_task" })),
            QuerySource::ScheduledTask
        );
    }

    #[test]
    fn submit_payload_source_maps_explicit_webhook_event() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "source": "webhook_event" })),
            QuerySource::WebhookEvent
        );
    }

    #[test]
    fn submit_payload_source_maps_explicit_channel_notification() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "source": "channel_notification" })),
            QuerySource::ChannelNotification
        );
    }

    #[test]
    fn submit_payload_source_maps_mcp_channel_notification() {
        assert_eq!(
            query_source_from_submit_payload(&json!({
                "source": "channel",
                "channel": {
                    "origin": { "type": "mcp", "server_name": "slack-mcp" }
                }
            })),
            QuerySource::ChannelNotification
        );
    }

    #[test]
    fn submit_payload_source_maps_webhook_channel_event() {
        assert_eq!(
            query_source_from_submit_payload(&json!({
                "source": "channel",
                "channel": {
                    "origin": { "type": "webhook", "endpoint": "/hooks/github" }
                }
            })),
            QuerySource::WebhookEvent
        );
    }

    #[test]
    fn submit_payload_source_maps_unknown_channel_as_channel_notification() {
        assert_eq!(
            query_source_from_submit_payload(&json!({
                "source": "channel",
                "channel": {
                    "origin": { "type": "other" }
                }
            })),
            QuerySource::ChannelNotification
        );
    }

    #[test]
    fn submit_payload_source_maps_unknown_top_level_source_to_repl_main_thread() {
        assert_eq!(
            query_source_from_submit_payload(&json!({
                "source": "unexpected_source"
            })),
            QuerySource::ReplMainThread
        );
    }

    #[test]
    fn submit_payload_source_maps_channel_without_origin_type_to_channel_notification() {
        assert_eq!(
            query_source_from_submit_payload(&json!({
                "source": "channel",
                "channel": {
                    "origin": {}
                }
            })),
            QuerySource::ChannelNotification
        );
    }

    #[test]
    fn submit_payload_source_maps_http_as_interactive() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "source": "http" })),
            QuerySource::ReplMainThread
        );
    }

    #[test]
    fn submit_payload_source_maps_missing_source_as_interactive() {
        assert_eq!(
            query_source_from_submit_payload(&json!({ "text": "hello" })),
            QuerySource::ReplMainThread
        );
    }

    #[test]
    fn brief_memory_log_entry_includes_structured_context() {
        let entry = brief_memory_log_entry(&BriefMessagePayload {
            message: "Build finished.".to_string(),
            status: allthecodes_types::brief::BriefMessageStatus::Proactive,
            attachments: vec!["target/report.txt".to_string()],
            level: Some(allthecodes_types::brief::BriefMessageLevel::Warning),
            source_tool_name: Some("Brief".to_string()),
            tool_use_id: Some("toolu_brief".to_string()),
            session_id: Some("session-1".to_string()),
            timestamp: Some(100),
        });

        assert!(entry.contains("brief output"));
        assert!(entry.contains("status=proactive"));
        assert!(entry.contains("level=warning"));
        assert!(entry.contains("session=session-1"));
        assert!(entry.contains("tool_use=toolu_brief"));
        assert!(entry.contains("Build finished."));
        assert!(entry.contains("attachments: target/report.txt"));
    }

    #[test]
    #[serial]
    fn daemon_submit_producer_payloads_resolve_to_expected_query_sources() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureGuard::enable_proactive();

        let tick = crate::tick::enqueue_proactive_tick_once(chrono::Local::now(), false)
            .unwrap()
            .expect("proactive tick should enqueue");
        assert_eq!(
            query_source_from_submit_payload(&tick.payload),
            QuerySource::ProactiveTick
        );

        let mcp_event = crate::channels::ChannelEvent {
            source: "slack".into(),
            sender: Some("alice".into()),
            content: "triage incident".into(),
            meta: serde_json::Value::Null,
            origin: crate::channels::ChannelOrigin::Mcp {
                server_name: "slack-mcp".into(),
            },
        };
        assert_eq!(
            query_source_from_submit_payload(&crate::channels::channel_submit_payload(&mcp_event)),
            QuerySource::ChannelNotification
        );

        let webhook_event = crate::channels::ChannelEvent {
            source: "github".into(),
            sender: None,
            content: "pull request opened".into(),
            meta: serde_json::Value::Null,
            origin: crate::channels::ChannelOrigin::Webhook {
                endpoint: "/hooks/github".into(),
            },
        };
        assert_eq!(
            query_source_from_submit_payload(&crate::channels::channel_submit_payload(
                &webhook_event
            )),
            QuerySource::WebhookEvent
        );
    }

    #[tokio::test]
    #[serial]
    async fn execute_submit_records_mapped_query_source_for_proactive_tick() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let runtime = test_runtime(temp.path());
        let command = submit_command(json!({
            "text": "<tick_tag>\nLocal time: 2026-07-05 12:00:00\n</tick_tag>",
            "message_id": "proactive-tick-test",
            "source": "proactive_tick"
        }));

        runtime
            .execute_submit("assistant-session-1", &command)
            .await
            .unwrap();

        let events = crate::protocol_store()
            .read_worker_events("assistant-session-1")
            .unwrap();
        let submit_started = events
            .iter()
            .find(|event| event.event_type == "submit_started")
            .expect("submit_started event should be recorded");

        assert_eq!(submit_started.data["source"], "proactive_tick");
        assert_eq!(submit_started.data["query_source"], "proactive_tick");
    }

    #[tokio::test]
    #[serial]
    async fn execute_submit_records_null_source_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let runtime = test_runtime(temp.path());
        let command = submit_command(json!({
            "text": "hello"
        }));

        runtime
            .execute_submit("assistant-session-1", &command)
            .await
            .unwrap();

        let events = crate::protocol_store()
            .read_worker_events("assistant-session-1")
            .unwrap();
        let submit_started = events
            .iter()
            .find(|event| event.event_type == "submit_started")
            .expect("submit_started event should be recorded");

        assert!(submit_started.data["source"].is_null());
        assert_eq!(submit_started.data["query_source"], "repl_main_thread");
    }

    #[test]
    #[serial]
    fn bridge_enqueues_submit_with_gateway_context() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let bridge = GatewayDaemonBridge::for_worker("assistant-session-1");

        let receipt = bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::Submit,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({ "text": "hello" }),
                idempotency_key: Some("delivery-1".to_string()),
            })
            .unwrap();

        let command = crate::protocol_store()
            .read_command("assistant-session-1", &receipt.command_id)
            .unwrap()
            .unwrap();
        assert_eq!(command.payload["text"], "hello");
        assert_eq!(command.payload["idempotencyKey"], "delivery-1");
        assert_eq!(command.payload["gateway"]["runId"], "run_bridge123");
    }

    #[test]
    #[serial]
    fn bridge_enqueues_submit_with_assistant_session_context() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let bridge = GatewayDaemonBridge::for_worker_with_session(
            "assistant-session-1",
            Some("bridge-session-1".to_string()),
            Some("assistant-session-prev".to_string()),
        );

        let receipt = bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::Submit,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({ "text": "hello" }),
                idempotency_key: Some("delivery-1".to_string()),
            })
            .unwrap();

        let command = crate::protocol_store()
            .read_command("assistant-session-1", &receipt.command_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            command.payload["gateway"]["bridgeSessionId"],
            "bridge-session-1"
        );
        assert_eq!(
            command.payload["gateway"]["assistantSessionId"],
            "assistant-session-prev"
        );
    }

    #[test]
    #[serial]
    fn bridge_submit_clears_active_sleep_for_non_proactive_payload() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        crate::process_state::write_sleep_state(300, "waiting").unwrap();
        let bridge = GatewayDaemonBridge::for_worker("assistant-session-1");

        bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::Submit,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({ "text": "wake up", "source": "remote_user" }),
                idempotency_key: Some("delivery-1".to_string()),
            })
            .unwrap();

        assert!(crate::process_state::active_sleep_state()
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn bridge_submit_preserves_active_sleep_for_proactive_tick_payload() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        crate::process_state::write_sleep_state(300, "waiting").unwrap();
        let bridge = GatewayDaemonBridge::for_worker("assistant-session-1");

        bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::Submit,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({ "text": "tick", "source": "proactive_tick" }),
                idempotency_key: Some("delivery-1".to_string()),
            })
            .unwrap();

        assert!(crate::process_state::active_sleep_state()
            .unwrap()
            .is_some());
    }

    #[test]
    #[serial]
    fn assistant_context_records_current_session_id_in_bridge_state() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        crate::process_state::write_bridge_session_state(&bridge_state(
            temp.path(),
            "bridge-session-1",
        ))
        .unwrap();
        let runtime = test_runtime(temp.path());
        let current_session_id = runtime.engine.current_session_id().to_string();
        let command = submit_command(json!({
            "text": "hello",
            "gateway": {
                "bridgeSessionId": "bridge-session-1",
                "runId": "run_bridge123",
                "sessionKey": "remote:http:abc"
            }
        }));

        runtime
            .apply_assistant_session_context("assistant-session-1", &command)
            .unwrap();
        let stored = crate::process_state::read_bridge_session_state("bridge-session-1")
            .unwrap()
            .unwrap();

        assert_eq!(
            stored.assistant_session_id.as_deref(),
            Some(current_session_id.as_str())
        );
    }

    #[test]
    #[serial]
    fn assistant_context_resumes_existing_assistant_session() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        crate::process_state::write_bridge_session_state(&bridge_state(
            temp.path(),
            "bridge-session-1",
        ))
        .unwrap();
        allthecodes_session::storage::save_session(
            "assistant-session-prev",
            &[user_message("previous prompt")],
            temp.path().to_str().unwrap(),
        )
        .unwrap();
        let runtime = test_runtime(temp.path());
        let command = submit_command(json!({
            "text": "hello",
            "gateway": {
                "bridgeSessionId": "bridge-session-1",
                "assistantSessionId": "assistant-session-prev",
                "runId": "run_bridge123",
                "sessionKey": "remote:http:abc"
            }
        }));

        runtime
            .apply_assistant_session_context("assistant-session-1", &command)
            .unwrap();
        let stored = crate::process_state::read_bridge_session_state("bridge-session-1")
            .unwrap()
            .unwrap();

        assert_eq!(
            runtime.engine.current_session_id().as_str(),
            "assistant-session-prev"
        );
        assert_eq!(runtime.engine.messages().len(), 1);
        assert_eq!(
            stored.assistant_session_id.as_deref(),
            Some("assistant-session-prev")
        );
    }

    #[test]
    #[serial]
    fn bridge_normalizes_gateway_interaction_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let bridge = GatewayDaemonBridge::for_worker("assistant-session-1");

        bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::PermissionResponse,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({
                    "toolUseId": "toolu_1",
                    "approved": true,
                    "reason": "looks ok",
                }),
                idempotency_key: Some("permission-1".to_string()),
            })
            .unwrap();
        bridge
            .dispatch(GatewayCommand {
                kind: GatewayCommandKind::AskUserResponse,
                run_id: "run_bridge123".to_string(),
                session_key: "remote:http:abc".to_string(),
                payload: json!({
                    "questionId": "question-1",
                    "response": "yes",
                }),
                idempotency_key: Some("ask-1".to_string()),
            })
            .unwrap();

        let commands = crate::protocol_store()
            .read_worker_commands("assistant-session-1")
            .unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].kind, DaemonCommandKind::PermissionResponse);
        assert_eq!(commands[0].payload["tool_use_id"], "toolu_1");
        assert_eq!(commands[0].payload["decision"], "allow");
        assert_eq!(commands[1].kind, DaemonCommandKind::AskUserResponse);
        assert_eq!(commands[1].payload["request_id"], "question-1");
        assert_eq!(commands[1].payload["answer"], "yes");
    }

    #[test]
    #[serial]
    fn bridge_appends_gateway_events_to_durable_run_log() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let store = GatewayStore::default_with_policy(SessionKeyPolicy::default());
        let source = allthecodes_gateway::RemoteSource::new(
            allthecodes_gateway::RemoteTransport::Http,
            "local",
            "F:/AIclassmanager/cc/rust",
            "client",
            "user",
            "thread",
        );
        let created = store
            .create_run(allthecodes_gateway::RunRequest {
                prompt: "hello".to_string(),
                source,
                policy: allthecodes_gateway::RunPolicy::default(),
                idempotency_key: None,
            })
            .unwrap();
        let run_id = created.meta().run_id.clone();
        let command = crate::protocol_store()
            .enqueue_command(
                "assistant-session-1",
                DaemonCommandKind::Submit,
                json!({
                    "text": "hello",
                    "gateway": {
                        "runId": run_id.to_string(),
                        "sessionKey": created.meta().session_key.to_string(),
                    }
                }),
                None,
            )
            .unwrap();

        append_gateway_event(
            &command,
            RunEventKind::Custom {
                name: "stream_delta".to_string(),
                payload: json!({ "text": "partial" }),
            },
        )
        .unwrap();
        update_gateway_status(&command, RunStatus::Running).unwrap();

        let events = store.read_events(&run_id).unwrap();
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            RunEventKind::Custom { name, .. } if name == "stream_delta"
        )));
        assert_eq!(store.load_run(&run_id).unwrap().status, RunStatus::Running);
    }

    #[test]
    #[serial]
    fn bridge_mirrors_automation_metadata_to_run_meta() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let store = GatewayStore::default_with_policy(SessionKeyPolicy::default());
        let source = allthecodes_gateway::RemoteSource::new(
            allthecodes_gateway::RemoteTransport::Http,
            "local",
            "F:/AIclassmanager/cc/rust",
            "client",
            "user",
            "thread",
        );
        let created = store
            .create_run(allthecodes_gateway::RunRequest {
                prompt: "hello".to_string(),
                source,
                policy: allthecodes_gateway::RunPolicy::default(),
                idempotency_key: None,
            })
            .unwrap();
        let run_id = created.meta().run_id.clone();
        let command = submit_command(json!({
            "text": "hello",
            "gateway": {
                "runId": run_id.to_string(),
                "sessionKey": created.meta().session_key.to_string(),
            }
        }));

        mirror_automation_state(&command, "assistant-session-1").unwrap();

        let meta = store.load_run(&run_id).unwrap();
        assert_eq!(meta.metadata["automation_state"]["status"], "standby");
        assert!(meta.metadata["automation_state"]
            .get("next_tick_at")
            .is_some());
    }

    #[test]
    #[serial]
    fn bridge_mirrors_context_blocked_automation_metadata_to_run_meta() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("gateway-test");
        allthecodes_types::proactive_context::set_context_blocked(true, "plan_mode");
        let store = GatewayStore::default_with_policy(SessionKeyPolicy::default());
        let source = allthecodes_gateway::RemoteSource::new(
            allthecodes_gateway::RemoteTransport::Http,
            "local",
            "F:/AIclassmanager/cc/rust",
            "client",
            "user",
            "thread",
        );
        let created = store
            .create_run(allthecodes_gateway::RunRequest {
                prompt: "hello".to_string(),
                source,
                policy: allthecodes_gateway::RunPolicy::default(),
                idempotency_key: None,
            })
            .unwrap();
        let run_id = created.meta().run_id.clone();
        let command = submit_command(json!({
            "text": "hello",
            "gateway": {
                "runId": run_id.to_string(),
                "sessionKey": created.meta().session_key.to_string(),
            }
        }));

        mirror_automation_state(&command, "assistant-session-1").unwrap();

        let meta = store.load_run(&run_id).unwrap();
        allthecodes_types::proactive_context::set_context_blocked(false, "test-cleanup");
        controller.deactivate("test-cleanup");
        assert_eq!(meta.metadata["automation_state"]["status"], "blocked");
        assert_eq!(meta.metadata["automation_state"]["reason"], "plan_mode");
    }

    #[test]
    #[serial]
    fn bridge_appends_automation_state_event_without_run_id() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let command = submit_command(json!({ "text": "hello" }));

        mirror_automation_state(&command, "assistant-session-1").unwrap();

        let events = crate::protocol_store()
            .read_worker_events("assistant-session-1")
            .unwrap();
        let event = events
            .iter()
            .find(|event| event.event_type == "automation_state")
            .expect("automation_state event should be appended");
        assert_eq!(event.data["automation_state"]["status"], "standby");
    }

    #[tokio::test]
    #[serial]
    async fn interaction_state_delivers_live_permission_response() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let interactions = Arc::new(AssistantInteractionState::default());
        let task = {
            let interactions = interactions.clone();
            tokio::spawn(async move {
                interactions
                    .request_permission(
                        "assistant-session-1",
                        PermissionRequestPayload {
                            tool_use_id: "tool-1".to_string(),
                            tool_name: "Bash".to_string(),
                            tool_input: json!({ "command": "cargo test" }),
                            message: "Allow Bash?".to_string(),
                            options: vec!["allow".to_string(), "deny".to_string()],
                            operation: None,
                        },
                    )
                    .await
            })
        };

        wait_until(|| {
            interactions
                .inner
                .lock()
                .pending_permissions
                .contains_key("tool-1")
        })
        .await;

        let delivery = interactions.complete_permission(
            "tool-1".to_string(),
            PermissionResponsePayload::decision("allow"),
        );

        assert_eq!(delivery, InteractionDelivery::Delivered);
        assert_eq!(
            task.await.unwrap(),
            PermissionResponsePayload::decision("allow")
        );
        assert!(crate::protocol_store()
            .read_worker_events("assistant-session-1")
            .unwrap()
            .iter()
            .any(|event| event.event_type == "permission_request"));
    }

    #[tokio::test]
    #[serial]
    async fn interaction_state_replays_permission_response_arriving_before_waiter() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let interactions = AssistantInteractionState::default();

        let delivery = interactions.complete_permission(
            "tool-1".to_string(),
            PermissionResponsePayload::new("deny", Some("not now".to_string())),
        );
        let response = interactions
            .request_permission(
                "assistant-session-1",
                PermissionRequestPayload {
                    tool_use_id: "tool-1".to_string(),
                    tool_name: "Bash".to_string(),
                    tool_input: json!({ "command": "cargo test" }),
                    message: "Allow Bash?".to_string(),
                    options: vec!["allow".to_string(), "deny".to_string()],
                    operation: None,
                },
            )
            .await;

        assert_eq!(delivery, InteractionDelivery::StoredForReplay);
        assert_eq!(
            response,
            PermissionResponsePayload::new("deny", Some("not now".to_string()))
        );
        assert!(interactions.inner.lock().replay_permissions.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn interaction_state_delivers_live_ask_user_response() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let interactions = Arc::new(AssistantInteractionState::default());
        let task = {
            let interactions = interactions.clone();
            tokio::spawn(async move {
                interactions
                    .ask_user(
                        "assistant-session-1",
                        AskUserRequestPayload {
                            question: "Which branch?".to_string(),
                            choices: vec!["main".to_string(), "feature".to_string()],
                            allow_free_text: true,
                        },
                    )
                    .await
            })
        };

        let question_id = wait_for_question_id(&interactions).await;
        let delivery = interactions.complete_question(question_id, "feature".to_string());

        assert_eq!(delivery, InteractionDelivery::Delivered);
        assert_eq!(task.await.unwrap(), "feature");
        assert!(crate::protocol_store()
            .read_worker_events("assistant-session-1")
            .unwrap()
            .iter()
            .any(|event| event.event_type == "ask_user_question"));
    }

    async fn wait_for_question_id(interactions: &AssistantInteractionState) -> String {
        for _ in 0..50 {
            if let Some(id) = interactions
                .inner
                .lock()
                .pending_questions
                .keys()
                .next()
                .cloned()
            {
                return id;
            }
            tokio::task::yield_now().await;
        }
        panic!("question waiter was not registered");
    }

    async fn wait_until(mut predicate: impl FnMut() -> bool) {
        for _ in 0..50 {
            if predicate() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition was not met");
    }
}
