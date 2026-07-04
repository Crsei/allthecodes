//! Route handlers for the KAIROS daemon HTTP API.
//!
//! Groups:
//! - **API routes** (`/api/*`) -- query submission, abort, status, attach/detach
//! - **Webhook routes** (`/webhook/*`) -- Phase-3 stubs for GitHub/Slack/generic
//! - **Health** (`/health`) -- simple liveness probe

use allthecodes_engine::command_runtime::{CommandContext, CommandResult};
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::app_state::AppState;
use allthecodes_server::RootProbeResponse;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::automation_state::{AutomationState, AutomationStatus};
use crate::protocol::{DaemonCommandKind, DaemonEvent, DaemonEventKind};
use allthecodes_types::message::CompactMetadata;
use allthecodes_types::plan_workflow::PlanWorkflowRecord;
use allthecodes_types::sdk::SdkMessage;

use super::process_state::{self, DaemonStatusSnapshot, DaemonWorkerSummary};
use super::state::{DaemonState, SseEvent};
use super::supervisor::ASSISTANT_WORKER_ID;
use super::team_memory_proxy;

fn sync_command_app_state(engine: &QueryEngine, command_state: &AppState) {
    let permission_context = command_state.tool_permission_context.clone();
    let team_context = command_state.team_context.clone();
    let plan_workflow = command_state.plan_workflow.clone();
    engine.update_app_state(|state| {
        state.tool_permission_context = permission_context;
        state.team_context = team_context;
        state.plan_workflow = plan_workflow;
    });
}

fn plan_workflow_event_payload(
    record: &PlanWorkflowRecord,
    event: &str,
    summary: &str,
) -> serde_json::Value {
    serde_json::to_value(allthecodes_types::plan_workflow::event_payload(
        record, event, summary,
    ))
    .unwrap_or(serde_json::Value::Null)
}

fn public_compact_metadata(
    metadata: &Option<CompactMetadata>,
    internal_metadata_hidden: bool,
) -> Option<Value> {
    metadata.as_ref().map(|metadata| {
        json!({
            "pre_compact_token_count": metadata.pre_compact_token_count,
            "post_compact_token_count": metadata.post_compact_token_count,
            "internal_metadata_hidden": internal_metadata_hidden
                || metadata.has_internal_metadata(),
        })
    })
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    pub text: String,
    pub id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub raw: String,
}

#[derive(Debug, Deserialize)]
pub struct PermissionRequest {
    pub tool_use_id: String,
    pub decision: String,
}

#[derive(Debug, Deserialize)]
pub struct ResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub kairos_active: bool,
    pub proactive: bool,
    pub query_running: bool,
    pub clients_connected: usize,
    pub sleeping: bool,
    pub daemon_sleep_until: Option<String>,
    pub daemon_sleep_reason: Option<String>,
    pub automation_state: AutomationState,
    pub permission_mode: String,
    pub plan_workflow: Option<PlanWorkflowRecord>,
    pub supervisor_status: String,
    pub supervisor_pid: Option<u32>,
    pub health_url: Option<String>,
    pub workers: Vec<DaemonWorkerSummary>,
    pub command_root: String,
    pub assistant_event_log: String,
}

#[derive(Debug, Deserialize)]
pub struct AttachRequest {
    pub client_id: String,
    pub last_seen_event: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DetachRequest {
    pub client_id: String,
}

// ---------------------------------------------------------------------------
// SdkMessage ->SseEvent mapping
// ---------------------------------------------------------------------------

/// Convert an [`SdkMessage`] into an [`SseEvent`] suitable for broadcasting
/// over SSE.
///
/// Returns `None` only for message variants that are internal to the SDK
/// stream and do not have a daemon-facing contract.
pub fn sdk_message_to_sse(msg: &SdkMessage, message_id: &str) -> Option<SseEvent> {
    let (event_type, data) = match msg {
        SdkMessage::SystemInit(init) => (
            "stream_start".to_string(),
            json!({
                "message_id": message_id,
                "tools": init.tools,
                "model": init.model,
                "session_id": init.session_id,
            }),
        ),
        SdkMessage::StreamEvent(se) => (
            "stream_delta".to_string(),
            json!({
                "message_id": message_id,
                "event": se.event,
                "session_id": se.session_id,
            }),
        ),
        SdkMessage::Assistant(am) => (
            "assistant_message".to_string(),
            json!({
                "message_id": message_id,
                "message": am.message,
                "session_id": am.session_id,
            }),
        ),
        SdkMessage::UserReplay(ur) => (
            "user_replay".to_string(),
            json!({
                "message_id": message_id,
                "content": ur.content,
                "session_id": ur.session_id,
            }),
        ),
        SdkMessage::Result(r) => (
            "stream_end".to_string(),
            json!({
                "message_id": message_id,
                "subtype": r.subtype,
                "is_error": r.is_error,
                "duration_ms": r.duration_ms,
                "result": r.result,
                "session_id": r.session_id,
            }),
        ),
        SdkMessage::Tombstone(t) => (
            "tombstone".to_string(),
            json!({
                "message_id": message_id,
                "assistant_id": t.message.uuid,
                "session_id": t.session_id,
            }),
        ),
        SdkMessage::ApiRetry(retry) => (
            "api_retry".to_string(),
            json!({
                "message_id": message_id,
                "attempt": retry.attempt,
                "max_retries": retry.max_retries,
                "retry_delay_ms": retry.retry_delay_ms,
                "error_status": retry.error_status,
                "error": retry.error,
                "session_id": retry.session_id,
            }),
        ),
        SdkMessage::CompactBoundary(boundary) => (
            "compact_boundary".to_string(),
            json!({
                "message_id": message_id,
                "compact_metadata": public_compact_metadata(
                    &boundary.compact_metadata,
                    boundary.internal_metadata_hidden,
                ),
                "session_id": boundary.session_id,
            }),
        ),
        SdkMessage::ToolUseSummary(summary) => (
            "tool_use_summary".to_string(),
            json!({
                "message_id": message_id,
                "summary": summary.summary,
                "preceding_tool_use_ids": summary.preceding_tool_use_ids,
                "session_id": summary.session_id,
            }),
        ),
        SdkMessage::GoalUpdated(update) => (
            "goal_updated".to_string(),
            json!({
                "message_id": message_id,
                "event": update.event,
                "goal": update.goal,
                "session_id": update.session_id,
            }),
        ),
    };

    Some(SseEvent {
        id: String::new(), // filled by broadcast()
        event_type,
        data,
    })
}

// ---------------------------------------------------------------------------
// API routes
// ---------------------------------------------------------------------------

/// Returns a [`Router`] containing all `/api/*` endpoints.
pub fn api_routes() -> Router<DaemonState> {
    Router::new()
        .route("/api/submit", post(submit))
        .route("/api/abort", post(abort))
        .route("/api/command", post(command))
        .route("/api/permission", post(permission))
        .route("/api/status", get(status))
        .route("/api/attach", post(attach))
        .route("/api/detach", post(detach))
        .route("/api/resize", post(resize))
        .route("/api/history", get(history))
        .route(
            "/api/account-auth/login/start",
            post(crate::account_auth::login_start),
        )
        .route(
            "/api/account-auth/login/complete",
            post(crate::account_auth::login_complete),
        )
        .route("/api/account-auth/status", get(crate::account_auth::status))
        .route(
            "/api/account-auth/refresh",
            post(crate::account_auth::refresh),
        )
        .route(
            "/api/account-auth/logout",
            post(crate::account_auth::logout),
        )
        .route(
            "/api/account-auth/billing",
            get(crate::account_auth::billing_snapshot),
        )
        .route(
            "/api/account-auth/billing/ledger",
            get(crate::account_auth::billing_ledger),
        )
        .route(
            "/api/account-auth/billing/orders/{id}",
            get(crate::account_auth::billing_order),
        )
}

fn require_control_token(headers: &HeaderMap) -> Result<(), Json<Value>> {
    let Some(candidate) = extract_control_token(headers) else {
        return Err(Json(json!({
            "status": "unauthorized",
            "message": "missing daemon control token",
        })));
    };
    match process_state::verify_control_token(candidate) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Json(json!({
            "status": "unauthorized",
            "message": "invalid daemon control token",
        }))),
        Err(err) => Err(Json(json!({
            "status": "error",
            "message": err.to_string(),
        }))),
    }
}

fn extract_control_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-allthecodes-daemon-token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
}

/// `POST /api/submit` -- enqueue a user message for the assistant worker.
async fn submit(
    State(state): State<DaemonState>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> Json<Value> {
    if let Err(response) = require_control_token(&headers) {
        return response;
    }
    submit_authorized(state, body).await
}

async fn submit_authorized(state: DaemonState, body: SubmitRequest) -> Json<Value> {
    let text = body.text;
    let message_id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let command = match super::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::Submit,
        json!({
            "text": text.clone(),
            "message_id": message_id.clone(),
            "source": "http",
        }),
        Some(body.idempotency_key.unwrap_or_else(|| message_id.clone())),
    ) {
        Ok(command) => command,
        Err(err) => {
            return Json(json!({
                "status": "error",
                "message": err.to_string(),
            }));
        }
    };

    info!(message_id, text_len = text.len(), "submit received");
    super::memory_log::append_log_entry(&format!("user submit: {}", &text));
    state.broadcast(SseEvent {
        id: String::new(),
        event_type: "daemon_command".to_string(),
        data: json!({
            "command_id": command.command_id,
            "worker_id": command.target_worker_id,
            "kind": "submit",
        }),
    });

    Json(json!({
        "status": "ok",
        "message_id": message_id,
        "command_id": command.command_id,
    }))
}

/// `POST /api/abort` -- abort the currently running query.
async fn abort(State(_state): State<DaemonState>, headers: HeaderMap) -> Json<Value> {
    if let Err(response) = require_control_token(&headers) {
        return response;
    }
    info!("abort request received");
    let command = match super::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::Abort,
        json!({ "source": "http" }),
        None,
    ) {
        Ok(command) => command,
        Err(err) => {
            return Json(json!({
                "status": "error",
                "message": err.to_string(),
            }));
        }
    };
    Json(json!({ "status": "ok", "command_id": command.command_id }))
}

/// `POST /api/command` -- execute a slash command.
async fn command(
    State(state): State<DaemonState>,
    headers: HeaderMap,
    Json(body): Json<CommandRequest>,
) -> Json<Value> {
    if let Err(response) = require_control_token(&headers) {
        return response;
    }
    let raw = body.raw.trim().to_string();
    let dispatcher = match crate::runtime::command_dispatcher() {
        Ok(dispatcher) => dispatcher,
        Err(err) => {
            return Json(json!({ "status": "error", "message": err.to_string() }));
        }
    };
    let command_executor = match crate::runtime::command_executor() {
        Ok(executor) => executor,
        Err(err) => {
            return Json(json!({ "status": "error", "message": err.to_string() }));
        }
    };
    let cwd = std::path::PathBuf::from(state.engine.cwd());
    let Some(parsed) = dispatcher.parse_command_input_for_cwd(&raw, &cwd) else {
        return Json(json!({ "status": "error", "message": format!("unknown command: {raw}") }));
    };
    let command_name = dispatcher
        .command_name_for_cwd(parsed.index, &cwd)
        .unwrap_or_else(|| raw.trim_start_matches('/').to_string());

    let original_app_state = state.engine.app_state();
    let original_plan = original_app_state.plan_workflow.clone();
    let original_mode = original_app_state.tool_permission_context.mode.clone();

    let mut ctx = CommandContext {
        messages: state.engine.messages(),
        cwd,
        app_state: original_app_state,
        session_id: state.engine.current_session_id(),
    };

    match command_executor
        .execute(parsed, command_name.clone(), &mut ctx)
        .await
    {
        Ok(result) => {
            let plan_changed = ctx.app_state.plan_workflow != original_plan
                || ctx.app_state.tool_permission_context.mode != original_mode;
            sync_command_app_state(&state.engine, &ctx.app_state);

            if plan_changed {
                if let Some(record) = ctx.app_state.plan_workflow.clone() {
                    state.broadcast(SseEvent {
                        id: String::new(),
                        event_type: "plan_workflow_event".to_string(),
                        data: plan_workflow_event_payload(
                            &record,
                            "slash_command",
                            &allthecodes_types::plan_workflow::summarize(&record),
                        ),
                    });
                }
            }

            match result {
                CommandResult::Output(text) => {
                    state.broadcast(SseEvent {
                        id: String::new(),
                        event_type: "system_info".to_string(),
                        data: json!({ "text": text, "level": "info" }),
                    });
                    Json(json!({
                        "status": "ok",
                        "kind": "output",
                        "permission_mode": state.engine.app_state().tool_permission_context.mode.as_str(),
                        "plan_workflow": state.engine.app_state().plan_workflow,
                    }))
                }
                CommandResult::SwitchSession {
                    session_id,
                    messages,
                    notice,
                } => {
                    state.engine.set_current_session_id(session_id.clone());
                    state.engine.replace_messages(messages);
                    state.broadcast(SseEvent {
                        id: String::new(),
                        event_type: "system_info".to_string(),
                        data: json!({ "text": notice, "level": "info" }),
                    });
                    Json(json!({
                        "status": "ok",
                        "kind": "switch_session",
                        "session_id": session_id.to_string(),
                        "permission_mode": state.engine.app_state().tool_permission_context.mode.as_str(),
                        "plan_workflow": state.engine.app_state().plan_workflow,
                    }))
                }
                CommandResult::Clear => {
                    let session_id = state.engine.start_new_session();
                    Json(json!({
                        "status": "ok",
                        "kind": "clear",
                        "session_id": session_id.to_string()
                    }))
                }
                CommandResult::Exit(text) => {
                    state.broadcast(SseEvent {
                        id: String::new(),
                        event_type: "system_info".to_string(),
                        data: json!({ "text": text, "level": "info" }),
                    });
                    Json(json!({ "status": "ok", "kind": "exit" }))
                }
                CommandResult::Query(_) => Json(json!({
                    "status": "ok",
                    "kind": "query_not_started",
                    "message": "daemon command endpoint does not start query command results yet"
                })),
                CommandResult::None => Json(json!({ "status": "ok", "kind": "none" })),
            }
        }
        Err(err) => Json(json!({ "status": "error", "message": err.to_string() })),
    }
}

/// `POST /api/permission` -- respond to a permission prompt (stub).
async fn permission(
    State(_state): State<DaemonState>,
    headers: HeaderMap,
    Json(body): Json<PermissionRequest>,
) -> Json<Value> {
    if let Err(response) = require_control_token(&headers) {
        return response;
    }
    warn!(
        tool_use_id = %body.tool_use_id,
        decision = %body.decision,
        "permission endpoint queued daemon command"
    );
    let command = match super::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::PermissionResponse,
        json!({
            "tool_use_id": body.tool_use_id.clone(),
            "decision": body.decision.clone(),
            "source": "http",
        }),
        None,
    ) {
        Ok(command) => command,
        Err(err) => {
            return Json(json!({
                "status": "error",
                "message": err.to_string(),
            }));
        }
    };
    Json(json!({ "status": "queued", "command_id": command.command_id }))
}

/// `GET /api/status` -- return daemon status.
async fn status(State(state): State<DaemonState>) -> Json<StatusResponse> {
    let app_state = state.engine.app_state();
    let automation_state = crate::automation_state::snapshot(&state);
    let (supervisor_status, supervisor_pid, health_url, workers) =
        match process_state::status_snapshot() {
            Ok(DaemonStatusSnapshot::Running(process)) => (
                "running".to_string(),
                Some(process.pid),
                Some(process.health_url),
                process.workers,
            ),
            Ok(DaemonStatusSnapshot::Stale(process)) => (
                "stale".to_string(),
                Some(process.pid),
                Some(process.health_url),
                process.workers,
            ),
            Ok(DaemonStatusSnapshot::Stopped) => ("stopped".to_string(), None, None, Vec::new()),
            Err(err) => (format!("error: {err}"), None, None, Vec::new()),
        };
    Json(StatusResponse {
        kairos_active: state.features.kairos,
        proactive: state.features.proactive,
        query_running: automation_state.query_running,
        clients_connected: state.clients.read().len(),
        sleeping: automation_state.status == AutomationStatus::Sleeping,
        daemon_sleep_until: automation_state
            .sleeping_until
            .as_ref()
            .map(|until| until.to_rfc3339()),
        daemon_sleep_reason: automation_state.reason.clone(),
        automation_state,
        permission_mode: app_state.tool_permission_context.mode.as_str().to_string(),
        plan_workflow: app_state.plan_workflow,
        supervisor_status,
        supervisor_pid,
        health_url,
        workers,
        command_root: super::protocol_store().commands_dir().display().to_string(),
        assistant_event_log: super::protocol_store()
            .worker_events_path(ASSISTANT_WORKER_ID)
            .display()
            .to_string(),
    })
}

fn history_snapshots_from_events(events: &[DaemonEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.event_type == DaemonEventKind::HistorySnapshot.as_str())
        .map(|event| event.data.clone())
        .collect()
}

fn latest_history_messages(history_snapshots: &[Value]) -> Vec<Value> {
    history_snapshots
        .last()
        .and_then(|snapshot| snapshot.get("messages"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `POST /api/attach` -- re-attach a client and return missed events.
async fn attach(State(state): State<DaemonState>, Json(body): Json<AttachRequest>) -> Json<Value> {
    info!(client_id = body.client_id, "client attach");
    let missed: Vec<SseEvent> = body
        .last_seen_event
        .as_deref()
        .map(|id| state.events_since(id))
        .unwrap_or_default();

    Json(json!({ "status": "ok", "missed_events": missed }))
}

/// `POST /api/detach` -- remove a client from the SSE registry.
async fn detach(State(state): State<DaemonState>, Json(body): Json<DetachRequest>) -> Json<Value> {
    info!(client_id = body.client_id, "client detach");
    state.clients.write().remove(&body.client_id);
    Json(json!({ "status": "ok" }))
}

/// `POST /api/resize` -- enqueue a terminal resize notification for the assistant worker.
async fn resize(headers: HeaderMap, Json(body): Json<ResizeRequest>) -> Json<Value> {
    if let Err(response) = require_control_token(&headers) {
        return response;
    }
    let command = match super::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::Resize,
        json!({
            "cols": body.cols,
            "rows": body.rows,
            "source": "http",
        }),
        None,
    ) {
        Ok(command) => command,
        Err(err) => {
            return Json(json!({
                "status": "error",
                "message": err.to_string(),
            }));
        }
    };
    Json(json!({ "status": "queued", "command_id": command.command_id }))
}

/// `GET /api/history` -- return worker-owned conversation history and durable events.
async fn history(State(state): State<DaemonState>) -> Json<Value> {
    let daemon_events = super::protocol_store()
        .read_worker_events(ASSISTANT_WORKER_ID)
        .unwrap_or_default();
    let history_snapshots = history_snapshots_from_events(&daemon_events);
    let history = latest_history_messages(&history_snapshots);
    Json(json!({
        "history": history,
        "history_snapshots": history_snapshots,
        "sse_events": state.events_since("0"),
        "daemon_events": daemon_events,
    }))
}

// ---------------------------------------------------------------------------
// Webhook routes
// ---------------------------------------------------------------------------

/// Returns a [`Router`] containing all `/webhook/*` endpoints.
pub fn webhook_routes() -> Router<DaemonState> {
    Router::new()
        .route("/webhook/github", post(super::webhook::webhook_github))
        .route("/webhook/slack", post(super::webhook::webhook_slack))
        .route("/webhook/generic", post(super::webhook::webhook_generic))
        .route(
            "/remote-control/v1/webhooks/{route_id}",
            post(super::webhook::webhook_declarative),
        )
}

// ---------------------------------------------------------------------------
// Team memory proxy route
// ---------------------------------------------------------------------------

/// Returns a [`Router`] containing the team memory proxy route.
pub fn team_memory_routes() -> Router<DaemonState> {
    Router::new().route(
        "/api/claude_code/team_memory",
        get(team_memory_proxy::proxy_team_memory).put(team_memory_proxy::proxy_team_memory),
    )
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// `GET /health` -- simple liveness probe.
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn healthz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("healthz", "daemon"))
}

pub async fn readyz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("readyz", "daemon"))
}

pub async fn startupz() -> Json<RootProbeResponse> {
    Json(RootProbeResponse::ok("startupz", "daemon"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use allthecodes_config::features::FeatureFlags;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use uuid::Uuid;

    use super::*;
    use crate::protocol::DaemonEventKind;
    use crate::webhook::webhook_github;
    use allthecodes_types::message::CompactMetadata;
    use allthecodes_types::sdk::{
        SdkApiRetry, SdkCompactBoundary, SdkGoalUpdated, SdkToolUseSummary,
    };
    use axum::body::{to_bytes, Body, Bytes};
    use axum::http::{header, Method, Request, StatusCode};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use tower::ServiceExt;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
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

    fn make_daemon_state() -> DaemonState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: ".".to_string(),
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
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        DaemonState::new(engine, Arc::new(FeatureFlags::all_disabled()), 19836)
    }

    async fn response_json(response: axum::response::Response) -> (StatusCode, Value) {
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        (status, serde_json::from_slice(&body).expect("json body"))
    }

    async fn post_json(app: Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-allthecodes-daemon-token", token)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .expect("request");
        response_json(app.oneshot(request).await.expect("response")).await
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        response_json(app.oneshot(request).await.expect("response")).await
    }

    fn github_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    fn install_test_runtime_adapters() {
        crate::runtime::set_runtime_adapters(crate::runtime::DaemonRuntimeAdapters {
            init_plugins: || {},
            active_tools: Vec::new,
            commands: Vec::new,
            command_dispatcher: || {
                Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new())
            },
            command_executor: || {
                Arc::new(allthecodes_engine::command_runtime::NoopCommandExecutor::new())
            },
            route_github_pr_activity: test_route_github_pr_activity,
        });
    }

    fn test_route_github_pr_activity(
        payload: &Value,
        event: Option<&str>,
        delivery_id: Option<&str>,
    ) -> anyhow::Result<Option<crate::runtime::GithubPrActivityRouteOutcome>> {
        assert_eq!(event, Some("pull_request"));
        assert_eq!(delivery_id, Some("delivery-42"));
        assert_eq!(payload["repository"]["name"], "allthecodes");
        Ok(Some(crate::runtime::GithubPrActivityRouteOutcome {
            matched: 1,
            delivered: 1,
        }))
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn permission_endpoint_queues_worker_permission_response() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let token = crate::process_state::write_control_token().unwrap();
        let app = api_routes().with_state(make_daemon_state());

        let (status, body) = post_json(
            app,
            "/api/permission",
            &token.token,
            json!({
                "tool_use_id": "toolu_1",
                "decision": "allow",
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "queued");
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, DaemonCommandKind::PermissionResponse);
        assert_eq!(commands[0].payload["tool_use_id"], "toolu_1");
        assert_eq!(commands[0].payload["decision"], "allow");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn resize_endpoint_queues_worker_resize_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let token = crate::process_state::write_control_token().unwrap();
        let app = api_routes().with_state(make_daemon_state());

        let (status, body) = post_json(
            app,
            "/api/resize",
            &token.token,
            json!({
                "cols": 120,
                "rows": 40,
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "queued");
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, DaemonCommandKind::Resize);
        assert_eq!(commands[0].payload["cols"], 120);
        assert_eq!(commands[0].payload["rows"], 40);
        assert_eq!(commands[0].payload["source"], "http");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn history_endpoint_returns_worker_history_snapshots() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                DaemonEventKind::HistorySnapshot.as_str(),
                json!({
                    "session_id": "session-1",
                    "messages": [
                        { "role": "user", "content": "hello" },
                        { "role": "assistant", "content": "hi" }
                    ],
                    "cursor": "2",
                }),
            )
            .unwrap();
        let app = api_routes().with_state(make_daemon_state());

        let (status, body) = get_json(app, "/api/history").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["history"],
            json!([
                { "role": "user", "content": "hello" },
                { "role": "assistant", "content": "hi" }
            ])
        );
        assert_eq!(body["history_snapshots"][0]["session_id"], "session-1");
        assert_eq!(body["daemon_events"][0]["event_type"], "history_snapshot");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_endpoint_embeds_automation_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let app = api_routes().with_state(make_daemon_state());

        let (status, body) = get_json(app, "/api/status").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["automation_state"]["status"], "standby");
        assert_eq!(body["automation_state"]["query_running"], false);
        assert_eq!(body["automation_state"]["pending_input"], false);
        assert_eq!(body["automation_state"]["terminal_focus"], false);
        assert_eq!(body["query_running"], false);
        assert_eq!(body["sleeping"], false);
    }

    #[test]
    fn daemon_sse_broadcasts_api_retry_events() {
        let event = sdk_message_to_sse(
            &SdkMessage::ApiRetry(SdkApiRetry {
                attempt: 1,
                max_retries: 3,
                retry_delay_ms: 250,
                error_status: Some(529),
                error: "overloaded".to_string(),
                session_id: "session-1".to_string(),
                uuid: Uuid::new_v4(),
            }),
            "message-1",
        )
        .expect("api retry should be broadcast");

        assert_eq!(event.event_type, "api_retry");
        assert_eq!(event.data["message_id"], "message-1");
        assert_eq!(event.data["attempt"], 1);
        assert_eq!(event.data["max_retries"], 3);
        assert_eq!(event.data["error_status"], 529);
        assert_eq!(event.data["session_id"], "session-1");
    }

    #[test]
    fn daemon_sse_broadcasts_compact_boundaries() {
        let event = sdk_message_to_sse(
            &SdkMessage::CompactBoundary(SdkCompactBoundary {
                session_id: "session-1".to_string(),
                uuid: Uuid::new_v4(),
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 100,
                    post_compact_token_count: 40,
                    preserved_segment: None,
                    pre_compact_discovered_tools: None,
                }),
                internal_metadata_hidden: false,
            }),
            "message-1",
        )
        .expect("compact boundary should be broadcast");

        assert_eq!(event.event_type, "compact_boundary");
        assert_eq!(event.data["message_id"], "message-1");
        assert_eq!(
            event.data["compact_metadata"]["pre_compact_token_count"],
            100
        );
        assert_eq!(
            event.data["compact_metadata"]["post_compact_token_count"],
            40
        );
        assert_eq!(event.data["session_id"], "session-1");
    }

    #[test]
    fn daemon_sse_hides_internal_compact_discovered_tools() {
        let event = sdk_message_to_sse(
            &SdkMessage::CompactBoundary(SdkCompactBoundary {
                session_id: "session-1".to_string(),
                uuid: Uuid::new_v4(),
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 100,
                    post_compact_token_count: 40,
                    preserved_segment: None,
                    pre_compact_discovered_tools: Some(vec![
                        "VaultHttpFetch".to_string(),
                        "LocalMemoryRecall".to_string(),
                    ]),
                }),
                internal_metadata_hidden: true,
            }),
            "message-1",
        )
        .expect("compact boundary should be broadcast");

        let metadata = event
            .data
            .get("compact_metadata")
            .and_then(Value::as_object)
            .expect("compact metadata object");
        assert!(metadata.get("pre_compact_discovered_tools").is_none());
        assert_eq!(metadata["internal_metadata_hidden"], true);
        let serialized = serde_json::to_string(&event.data).unwrap();
        assert!(!serialized.contains("VaultHttpFetch"));
        assert!(!serialized.contains("LocalMemoryRecall"));
    }

    #[test]
    fn daemon_sse_broadcasts_tool_use_summaries() {
        let event = sdk_message_to_sse(
            &SdkMessage::ToolUseSummary(SdkToolUseSummary {
                summary: "Read finished".to_string(),
                preceding_tool_use_ids: vec!["toolu_1".to_string()],
                session_id: "session-1".to_string(),
                uuid: Uuid::new_v4(),
            }),
            "message-1",
        )
        .expect("tool use summary should be broadcast");

        assert_eq!(event.event_type, "tool_use_summary");
        assert_eq!(event.data["message_id"], "message-1");
        assert_eq!(event.data["summary"], "Read finished");
        assert_eq!(event.data["preceding_tool_use_ids"][0], "toolu_1");
        assert_eq!(event.data["session_id"], "session-1");
    }

    #[test]
    fn daemon_sse_broadcasts_goal_updates() {
        let event = sdk_message_to_sse(
            &SdkMessage::GoalUpdated(SdkGoalUpdated {
                event: "budget_limited".to_string(),
                goal: json!({"objective": "ship", "tokens_used": 12}),
                session_id: "session-1".to_string(),
                uuid: Uuid::new_v4(),
            }),
            "message-1",
        )
        .expect("goal update should be broadcast");

        assert_eq!(event.event_type, "goal_updated");
        assert_eq!(event.data["message_id"], "message-1");
        assert_eq!(event.data["event"], "budget_limited");
        assert_eq!(event.data["goal"]["objective"], "ship");
        assert_eq!(event.data["session_id"], "session-1");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn github_webhook_routes_matching_pr_activity_through_adapter() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _github_secret = EnvGuard::set("ALLTHECODES_GITHUB_WEBHOOK_SECRET", "route-secret");
        install_test_runtime_adapters();
        let body = serde_json::to_vec(&json!({
            "action": "opened",
            "repository": {
                "name": "allthecodes",
                "owner": { "login": "AIclassmanager" }
            },
            "pull_request": {
                "number": 42,
                "title": "Phase 4",
                "html_url": "https://github.com/AIclassmanager/allthecodes/pull/42"
            },
            "sender": { "login": "octocat" }
        }))
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", "pull_request".parse().unwrap());
        headers.insert("x-github-delivery", "delivery-42".parse().unwrap());
        headers.insert(
            "x-hub-signature-256",
            github_signature("route-secret", &body).parse().unwrap(),
        );

        let Json(response) = webhook_github(headers, Bytes::from(body)).await;

        assert_eq!(response["status"], "received");
        assert_eq!(response["matched"], 1);
        assert_eq!(response["delivered"], 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn github_webhook_rejects_bad_signature_when_secret_is_configured() {
        let _secret = EnvGuard::set("ALLTHECODES_GITHUB_WEBHOOK_SECRET", "secret");
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            "sha256=0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
        );

        let Json(response) = webhook_github(headers, Bytes::from_static(b"{}")).await;

        assert_eq!(response["status"], "error");
        assert_eq!(response["error"]["code"], "bad_hmac");
    }
}
