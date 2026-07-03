//! ACP runtime -- main I/O loop, method dispatch, initialization.
//!
//! The runtime owns the per-crate lifecycle: read JSON-RPC frames from stdin,
//! dispatch to method handlers, write responses and notifications to stdout.

use std::collections::HashMap;
use std::sync::Arc;

use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v2;
use tokio::sync::{mpsc, oneshot};

use crate::engine_factory::AcpEngineFactory;
use crate::jsonrpc::{self, InboundBatchEntry, InboundMessage};
use crate::session::AcpSessionManager;
use crate::transport::{spawn_sink_writer, AcpSink, AcpStdioReader};
use crate::updates::{state_idle_update, state_running_update, AcpUpdateMapper};

type PendingRequests = Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<()>>>>;

/// Configuration passed into the runtime.
pub struct RuntimeContext {
    pub model: String,
    pub cwd: std::path::PathBuf,
    pub tools: allthecodes_engine::types::tool::Tools,
    pub app_state_template: allthecodes_engine::types::app_state::AppState,
    pub merged_config: allthecodes_config::settings::EffectiveSettings,
    pub cli_overrides: crate::AcpCliOverrides,
    pub engine_factory: Arc<dyn AcpEngineFactory>,
}

/// ACP capability set built from implemented features.
#[derive(Debug, Clone)]
pub struct AcpCapabilities {
    pub session: bool,
    pub session_prompt: bool,
    pub session_delete: bool,
    pub session_mcp: bool,
    pub auth: bool,
}

impl AcpCapabilities {
    /// Build the baseline capabilities that are implemented.
    pub fn baseline() -> Self {
        Self {
            session: true,
            session_prompt: true,
            session_delete: false,
            session_mcp: false,
            auth: true,
        }
    }
}

/// Result of dispatching a JSON-RPC request.
pub enum DispatchOutcome {
    Response(Result<serde_json::Value, v2::Error>),
    ResponseThenStart(PendingPromptStart),
}

/// Accepted prompt turn that must not start streaming until its response is sent.
pub struct PendingPromptStart {
    response: Result<serde_json::Value, v2::Error>,
    start_tx: oneshot::Sender<()>,
    session: Arc<crate::session::AcpSession>,
}

impl PendingPromptStart {
    fn new(
        response: serde_json::Value,
        start_tx: oneshot::Sender<()>,
        session: Arc<crate::session::AcpSession>,
    ) -> Self {
        Self {
            response: Ok(response),
            start_tx,
            session,
        }
    }

    async fn cancel_before_start(self) -> Result<serde_json::Value, v2::Error> {
        {
            let mut turn = self.session.active_turn.lock().await;
            *turn = None;
        }
        drop(self.start_tx);
        Err(v2::Error::request_cancelled())
    }
}

fn request_id_key(id: &RequestId) -> String {
    format!("{id:?}")
}

fn cancel_requested(cancel_rx: &mut oneshot::Receiver<()>) -> bool {
    matches!(cancel_rx.try_recv(), Ok(()))
}

async fn resolve_dispatch_outcome(
    outcome: DispatchOutcome,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> (
    Result<serde_json::Value, v2::Error>,
    Option<oneshot::Sender<()>>,
) {
    match outcome {
        DispatchOutcome::Response(result) => (result, None),
        DispatchOutcome::ResponseThenStart(pending) => {
            if cancel_requested(cancel_rx) {
                (pending.cancel_before_start().await, None)
            } else {
                (pending.response, Some(pending.start_tx))
            }
        }
    }
}

/// Send a request response and release any deferred prompt start after enqueueing it.
pub async fn send_dispatch_outcome(
    id: &RequestId,
    outcome: DispatchOutcome,
    sink: &AcpSink,
    cancel_rx: &mut oneshot::Receiver<()>,
) {
    let (result, start_tx) = resolve_dispatch_outcome(outcome, cancel_rx).await;
    sink.send(jsonrpc::build_response(id, result));
    if let Some(start_tx) = start_tx {
        let _ = start_tx.send(());
    }
}

/// Run the ACP stdio runtime.
pub async fn run_runtime(ctx: RuntimeContext) -> anyhow::Result<()> {
    let (sink_tx, sink_rx) = mpsc::unbounded_channel();
    let sink = AcpSink::new(sink_tx);
    let _sink_handle = spawn_sink_writer(sink_rx);

    let engine_factory = ctx.engine_factory.clone();
    let session_manager = Arc::new(AcpSessionManager::new(engine_factory));

    let capabilities = AcpCapabilities::baseline();

    let mut reader = AcpStdioReader::new();

    // Track in-flight request ids for $/cancel_request support
    let pending_requests: PendingRequests = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    loop {
        let line = match reader.read_line().await {
            Some(line) => line,
            None => break,
        };

        let msg = match jsonrpc::parse_frame(&line) {
            Ok(Some(msg)) => msg,
            Ok(None) => continue,
            Err(err) => {
                let error_resp = jsonrpc::build_response::<()>(&RequestId::Null, Err(err));
                sink.send(error_resp);
                continue;
            }
        };

        match msg {
            InboundMessage::Request { id, method, params } => {
                let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
                {
                    pending_requests
                        .lock()
                        .await
                        .insert(request_id_key(&id), cancel_tx);
                }

                let outcome = dispatch_request(
                    &method,
                    params.as_deref(),
                    &id,
                    &capabilities,
                    &session_manager,
                    &sink,
                    &ctx,
                )
                .await;

                if let Some(outcome) = outcome {
                    send_dispatch_outcome(&id, outcome, &sink, &mut cancel_rx).await;
                }

                pending_requests.lock().await.remove(&request_id_key(&id));
            }
            InboundMessage::Notification { method, params } => {
                handle_notification(
                    &method,
                    params.as_deref(),
                    &session_manager,
                    &sink,
                    &pending_requests,
                )
                .await;
            }
            InboundMessage::Batch(entries) => {
                handle_batch(
                    entries,
                    &capabilities,
                    &session_manager,
                    &sink,
                    &ctx,
                    &pending_requests,
                )
                .await;
            }
        }
    }

    // Graceful shutdown: cancel all pending permissions
    // (sessions are cleaned up when the process exits)

    Ok(())
}

/// Dispatch a single request to the appropriate handler.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_request(
    method: &str,
    params: Option<&serde_json::value::RawValue>,
    _id: &RequestId,
    capabilities: &AcpCapabilities,
    session_manager: &Arc<AcpSessionManager>,
    sink: &AcpSink,
    ctx: &RuntimeContext,
) -> Option<DispatchOutcome> {
    match method {
        "initialize" => {
            let result = handle_initialize(params, capabilities, ctx).await;
            Some(DispatchOutcome::Response(result))
        }
        "auth/login" => {
            if !capabilities.auth {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            let result = handle_auth_login(params).await;
            Some(DispatchOutcome::Response(result))
        }
        "auth/logout" => {
            if !capabilities.auth {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            let result = handle_auth_logout().await;
            Some(DispatchOutcome::Response(result))
        }
        "session/new" | "session/load" | "session/resume" | "session/list" | "session/close" => {
            if !capabilities.session {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            let result = handle_session_method(method, params, session_manager, sink, ctx).await;
            Some(DispatchOutcome::Response(result))
        }
        "session/set_config_option" => {
            if !capabilities.session {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            let result = handle_set_config_option(params, session_manager).await;
            Some(DispatchOutcome::Response(result))
        }
        "session/prompt" => {
            if !capabilities.session || !capabilities.session_prompt {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            match handle_session_prompt(params, session_manager, sink, ctx).await {
                Ok(pending) => Some(DispatchOutcome::ResponseThenStart(pending)),
                Err(err) => Some(DispatchOutcome::Response(Err(err))),
            }
        }
        "session/delete" => {
            if !capabilities.session || !capabilities.session_delete {
                return Some(DispatchOutcome::Response(
                    Err(v2::Error::method_not_found()),
                ));
            }
            let result = handle_session_delete(params, session_manager, sink).await;
            Some(DispatchOutcome::Response(result))
        }
        _ => Some(DispatchOutcome::Response(Err(v2::Error::method_not_found(
        )
        .data(format!("unknown method: {method}"))))),
    }
}

/// Handle a notification (fire-and-forget).
async fn handle_notification(
    method: &str,
    params: Option<&serde_json::value::RawValue>,
    session_manager: &Arc<AcpSessionManager>,
    _sink: &AcpSink,
    pending_requests: &PendingRequests,
) {
    match method {
        "$/cancel_request" => {
            // Cancel a specific in-flight request by id
            if let Some(raw) = params {
                if let Ok(notif) = serde_json::from_str::<v2::CancelRequestNotification>(raw.get())
                {
                    let id_str = request_id_key(&notif.request_id);
                    if let Some(cancel_tx) = pending_requests.lock().await.remove(&id_str) {
                        let _ = cancel_tx.send(());
                    }
                }
            }
        }
        "session/cancel" => {
            // Cancel an active turn in a session
            if let Some(raw) = params {
                if let Ok(notif) = serde_json::from_str::<v2::CancelSessionNotification>(raw.get())
                {
                    let sid = notif.session_id.0.to_string();
                    if let Some(session) = session_manager.get_session(&sid).await {
                        if let Some(ref mut turn) = *session.active_turn.lock().await {
                            turn.cancel_requested = true;
                        }
                        session.engine.abort();
                        tracing::info!(session_id = %sid, "acp session/cancel: engine aborted");
                    }
                }
            }
        }
        _ => {}
    }
}

/// Handle a batch of requests/notifications.
#[allow(clippy::too_many_arguments)]
async fn handle_batch(
    entries: Vec<InboundBatchEntry>,
    capabilities: &AcpCapabilities,
    session_manager: &Arc<AcpSessionManager>,
    sink: &AcpSink,
    ctx: &RuntimeContext,
    pending_requests: &PendingRequests,
) {
    struct PendingBatchOutcome {
        id_key: String,
        outcome: DispatchOutcome,
        cancel_rx: oneshot::Receiver<()>,
    }

    let mut pending_outcomes = Vec::new();
    for entry in &entries {
        match entry {
            InboundBatchEntry::Request { id, method, params } => {
                let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
                let id_key = request_id_key(id);
                pending_requests
                    .lock()
                    .await
                    .insert(id_key.clone(), cancel_tx);

                let outcome = dispatch_request(
                    method,
                    params.as_deref(),
                    id,
                    capabilities,
                    session_manager,
                    sink,
                    ctx,
                )
                .await;
                pending_outcomes.push(PendingBatchOutcome {
                    id_key,
                    outcome: outcome
                        .unwrap_or(DispatchOutcome::Response(Err(v2::Error::internal_error()))),
                    cancel_rx,
                });
            }
            InboundBatchEntry::Notification { method, params } => {
                handle_notification(
                    method,
                    params.as_deref(),
                    session_manager,
                    sink,
                    pending_requests,
                )
                .await;
            }
        }
    }

    let mut results = Vec::new();
    let mut start_txs = Vec::new();
    for mut pending in pending_outcomes {
        pending_requests.lock().await.remove(&pending.id_key);
        let (result, start_tx) =
            resolve_dispatch_outcome(pending.outcome, &mut pending.cancel_rx).await;
        results.push(result);
        if let Some(start_tx) = start_tx {
            start_txs.push(start_tx);
        }
    }

    if let Some(batch_resp) = jsonrpc::build_batch_response(&entries, results) {
        sink.send(batch_resp);
        for start_tx in start_txs {
            let _ = start_tx.send(());
        }
    }
}

fn send_session_update(sink: &AcpSink, session_id: v2::SessionId, update: v2::SessionUpdate) {
    let notification = v2::UpdateSessionNotification::new(session_id, update);
    sink.send(jsonrpc::build_agent_notification(
        v2::AgentNotification::UpdateSessionNotification(Box::new(notification)),
    ));
}

// ---------------------------------------------------------------------------
// Initialization handler
// ---------------------------------------------------------------------------

async fn handle_initialize(
    params: Option<&serde_json::value::RawValue>,
    capabilities: &AcpCapabilities,
    _ctx: &RuntimeContext,
) -> Result<serde_json::Value, v2::Error> {
    let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
    let req: v2::InitializeRequest = serde_json::from_str(raw.get())
        .map_err(|e| v2::Error::invalid_params().data(format!("invalid initialize params: {e}")))?;

    if req.protocol_version != agent_client_protocol_schema::ProtocolVersion::V2 {
        return Err(v2::Error::invalid_params()
            .data("unsupported protocol version; only version 2 is accepted"));
    }

    let mut agent_caps = v2::AgentCapabilities::default();

    if capabilities.session {
        let session_caps = v2::SessionCapabilities::default();
        agent_caps = agent_caps.session(Some(session_caps));
    }

    if capabilities.auth {
        agent_caps = agent_caps.auth(Some(v2::AgentAuthCapabilities::default()));
    }

    let auth_methods = crate::auth::build_auth_methods();

    let response = v2::InitializeResponse::new(
        agent_client_protocol_schema::ProtocolVersion::V2,
        v2::Implementation::new("allthecodes", env!("CARGO_PKG_VERSION")),
    )
    .capabilities(agent_caps)
    .auth_methods(auth_methods);

    serde_json::to_value(response)
        .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

async fn handle_auth_login(
    params: Option<&serde_json::value::RawValue>,
) -> Result<serde_json::Value, v2::Error> {
    let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
    let req: v2::LoginAuthRequest = serde_json::from_str(raw.get())
        .map_err(|e| v2::Error::invalid_params().data(format!("invalid auth/login params: {e}")))?;

    let response = crate::auth::handle_login(req)?;

    serde_json::to_value(response)
        .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
}

async fn handle_auth_logout() -> Result<serde_json::Value, v2::Error> {
    let response = crate::auth::handle_logout()?;

    serde_json::to_value(response)
        .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Session method handlers
// ---------------------------------------------------------------------------

async fn handle_session_method(
    method: &str,
    params: Option<&serde_json::value::RawValue>,
    session_manager: &Arc<AcpSessionManager>,
    sink: &AcpSink,
    _ctx: &RuntimeContext,
) -> Result<serde_json::Value, v2::Error> {
    match method {
        "session/new" => {
            let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
            let req: v2::NewSessionRequest = serde_json::from_str(raw.get()).map_err(|e| {
                v2::Error::invalid_params().data(format!("invalid session/new params: {e}"))
            })?;

            let cwd = std::path::PathBuf::from(&req.cwd);
            AcpSessionManager::validate_cwd(&cwd)
                .map_err(|e| v2::Error::invalid_params().data(e))?;

            let additional = req.additional_directories.clone();
            AcpSessionManager::validate_additional_dirs(&additional)
                .map_err(|e| v2::Error::invalid_params().data(e))?;
            if !req.mcp_servers.is_empty() {
                return Err(v2::Error::invalid_params()
                    .data("ACP MCP server connections are not yet supported"));
            }

            let sid =
                agent_client_protocol_schema::v2::SessionId::new(uuid::Uuid::new_v4().to_string());

            let session = session_manager
                .create_session(sid.clone(), cwd, additional, None)
                .await
                .map_err(|e| v2::Error::internal_error().data(e.to_string()))?;

            let config_entries = crate::config_options::build_config_options(&session);
            let config_options: Vec<v2::SessionConfigOption> = config_entries
                .into_iter()
                .map(|e| e.config_option)
                .collect();

            let resp = v2::NewSessionResponse::new(sid.clone()).config_options(config_options);

            // Send available_commands_update after session creation
            if let Some(cmd_update) = crate::commands::build_commands_update() {
                send_session_update(
                    sink,
                    sid.clone(),
                    v2::SessionUpdate::AvailableCommandsUpdate(cmd_update),
                );
            }

            serde_json::to_value(resp)
                .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
        }
        "session/load" => {
            let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
            let req: v2::LoadSessionRequest = serde_json::from_str(raw.get()).map_err(|e| {
                v2::Error::invalid_params().data(format!("invalid session/load params: {e}"))
            })?;

            let cwd = std::path::PathBuf::from(&req.cwd);
            AcpSessionManager::validate_cwd(&cwd)
                .map_err(|e| v2::Error::invalid_params().data(e))?;

            let additional = req.additional_directories.clone();
            AcpSessionManager::validate_additional_dirs(&additional)
                .map_err(|e| v2::Error::invalid_params().data(e))?;
            if !req.mcp_servers.is_empty() {
                return Err(v2::Error::invalid_params()
                    .data("ACP MCP server connections are not yet supported"));
            }

            let resumed =
                allthecodes_session::resume::resume_session_detail(&req.session_id.0.to_string())
                    .map_err(|_| {
                    v2::Error::resource_not_found(Some(format!("session:{}", req.session_id)))
                })?;
            let replay_messages = resumed.messages.clone();

            let sid = req.session_id;
            let session = session_manager
                .create_session(sid.clone(), cwd.clone(), additional, Some(resumed.messages))
                .await
                .map_err(|e| v2::Error::internal_error().data(e.to_string()))?;

            crate::session::replay_loaded_messages(sid.clone(), &replay_messages, &cwd, sink)
                .map_err(|e| v2::Error::internal_error().data(e.to_string()))?;

            let config_entries = crate::config_options::build_config_options(&session);
            let config_options: Vec<v2::SessionConfigOption> = config_entries
                .into_iter()
                .map(|e| e.config_option)
                .collect();

            let resp = v2::LoadSessionResponse::new().config_options(config_options);

            // Send available_commands_update after session load
            if let Some(cmd_update) = crate::commands::build_commands_update() {
                send_session_update(
                    sink,
                    sid.clone(),
                    v2::SessionUpdate::AvailableCommandsUpdate(cmd_update),
                );
            }

            serde_json::to_value(resp)
                .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
        }
        "session/resume" => {
            let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
            let req: v2::ResumeSessionRequest = serde_json::from_str(raw.get()).map_err(|e| {
                v2::Error::invalid_params().data(format!("invalid session/resume params: {e}"))
            })?;

            let cwd = std::path::PathBuf::from(&req.cwd);
            AcpSessionManager::validate_cwd(&cwd)
                .map_err(|e| v2::Error::invalid_params().data(e))?;

            let resumed =
                allthecodes_session::resume::resume_session_detail(&req.session_id.0.to_string())
                    .map_err(|_| {
                    v2::Error::resource_not_found(Some(format!("session:{}", req.session_id)))
                })?;

            let sid = req.session_id;
            let session = session_manager
                .create_session(sid.clone(), cwd, Vec::new(), Some(resumed.messages))
                .await
                .map_err(|e| v2::Error::internal_error().data(e.to_string()))?;

            let config_entries = crate::config_options::build_config_options(&session);
            let config_options: Vec<v2::SessionConfigOption> = config_entries
                .into_iter()
                .map(|e| e.config_option)
                .collect();

            let resp = v2::ResumeSessionResponse::new().config_options(config_options);

            // Send available_commands_update after session resume
            if let Some(cmd_update) = crate::commands::build_commands_update() {
                send_session_update(
                    sink,
                    sid.clone(),
                    v2::SessionUpdate::AvailableCommandsUpdate(cmd_update),
                );
            }

            serde_json::to_value(resp)
                .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
        }
        "session/list" => {
            let req: Option<v2::ListSessionsRequest> = if let Some(raw) = params {
                Some(serde_json::from_str(raw.get()).map_err(|e| {
                    v2::Error::invalid_params().data(format!("invalid session/list params: {e}"))
                })?)
            } else {
                None
            };

            serde_json::to_value(crate::session::list_sessions(req.as_ref())?)
                .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
        }
        "session/close" => {
            let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
            let req: v2::CloseSessionRequest = serde_json::from_str(raw.get()).map_err(|e| {
                v2::Error::invalid_params().data(format!("invalid session/close params: {e}"))
            })?;

            let session = session_manager
                .get_session(&req.session_id.0.to_string())
                .await
                .ok_or_else(|| {
                    v2::Error::resource_not_found(Some(format!("session:{}", req.session_id)))
                })?;

            crate::session::close_session(&session, session_manager, sink).await;

            serde_json::to_value(v2::CloseSessionResponse::new())
                .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
        }
        _ => Err(v2::Error::method_not_found().data(format!("unknown session method: {method}"))),
    }
}

// ---------------------------------------------------------------------------
// Session delete handler
// ---------------------------------------------------------------------------

async fn handle_session_delete(
    params: Option<&serde_json::value::RawValue>,
    session_manager: &Arc<AcpSessionManager>,
    sink: &AcpSink,
) -> Result<serde_json::Value, v2::Error> {
    let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
    let req: v2::DeleteSessionRequest = serde_json::from_str(raw.get()).map_err(|e| {
        v2::Error::invalid_params().data(format!("invalid session/delete params: {e}"))
    })?;

    let sid_str = req.session_id.0.to_string();

    // If session is active in memory, close it first
    if let Some(session) = session_manager.get_session(&sid_str).await {
        crate::session::close_session(&session, session_manager, sink).await;
    }

    // Archive the session in storage (delete is an archive operation)
    let _ = allthecodes_session::storage::archive_session(&sid_str);

    serde_json::to_value(v2::DeleteSessionResponse::new())
        .map_err(|e| v2::Error::internal_error().data(e.to_string()))
}

// ---------------------------------------------------------------------------
// Config option handler
// ---------------------------------------------------------------------------

async fn handle_set_config_option(
    params: Option<&serde_json::value::RawValue>,
    session_manager: &Arc<AcpSessionManager>,
) -> Result<serde_json::Value, v2::Error> {
    let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
    let req: v2::SetSessionConfigOptionRequest = serde_json::from_str(raw.get()).map_err(|e| {
        v2::Error::invalid_params().data(format!("invalid session/set_config_option params: {e}"))
    })?;

    let sid_str = req.session_id.0.to_string();
    let session = session_manager.get_session(&sid_str).await.ok_or_else(|| {
        v2::Error::resource_not_found(Some(format!("session:{}", req.session_id)))
    })?;

    let config_id = req.config_id.0.to_string();
    let value = req.value.0.to_string();

    match config_id.as_str() {
        "model" => {
            if value.trim().is_empty() {
                return Err(v2::Error::invalid_params().data("model value cannot be empty"));
            }
        }
        "thought_level" => {
            if !matches!(value.as_str(), "low" | "medium" | "high") {
                return Err(v2::Error::invalid_params()
                    .data(format!("invalid thought_level value: {value}")));
            }
        }
        "mode" => {
            if !matches!(value.as_str(), "default" | "auto" | "bypass") {
                return Err(
                    v2::Error::invalid_params().data(format!("invalid mode value: {value}"))
                );
            }
        }
        _ => {
            return Err(
                v2::Error::invalid_params().data(format!("unknown config option: {config_id}"))
            );
        }
    }

    // Update the session engine AppState based on config_id
    session.engine.update_app_state(|state| {
        match config_id.as_str() {
            "model" => {
                state.main_loop_model = value.clone();
            }
            "thought_level" => {
                state.effort_value = Some(value.clone());
            }
            "mode" => {
                // Mode changes update permission context
                let new_mode = value.clone();
                state.tool_permission_context.mode =
                    allthecodes_types::permissions::PermissionMode::parse(&new_mode);
            }
            _ => unreachable!("config_id was validated above"),
        }
    });

    // Build the updated config options list for the response
    let config_entries = crate::config_options::build_config_options(&session);
    let config_options: Vec<v2::SessionConfigOption> = config_entries
        .into_iter()
        .map(|e| e.config_option)
        .collect();

    let resp = v2::SetSessionConfigOptionResponse::new(config_options);

    serde_json::to_value(resp)
        .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))
}

// ---------------------------------------------------------------------------
// Session prompt handler
// ---------------------------------------------------------------------------

async fn handle_session_prompt(
    params: Option<&serde_json::value::RawValue>,
    session_manager: &Arc<AcpSessionManager>,
    sink: &AcpSink,
    _ctx: &RuntimeContext,
) -> Result<PendingPromptStart, v2::Error> {
    let raw = params.ok_or_else(|| v2::Error::invalid_params().data("missing params"))?;
    let req: v2::PromptRequest = serde_json::from_str(raw.get()).map_err(|e| {
        v2::Error::invalid_params().data(format!("invalid session/prompt params: {e}"))
    })?;

    let sid_str = req.session_id.0.to_string();
    let session = session_manager.get_session(&sid_str).await.ok_or_else(|| {
        v2::Error::resource_not_found(Some(format!("session:{}", req.session_id)))
    })?;

    // Check for active turn
    {
        let mut turn = session.active_turn.lock().await;
        if turn.is_some() {
            return Err(v2::Error::invalid_params().data("session already has an active turn"));
        }
        *turn = Some(crate::session::AcpTurnHandle {
            cancel_requested: false,
        });
    }

    // Convert content blocks to a prompt string
    let prompt = crate::content::convert_prompt_blocks(&req.prompt)
        .map_err(|e| v2::Error::invalid_params().data(e.to_string()))?;

    let prompt_response = serde_json::to_value(v2::PromptResponse::new())
        .map_err(|e| v2::Error::internal_error().data(format!("serialization error: {e}")))?;

    // Spawn a background task, but keep it paused until the ACK is enqueued.
    let (start_tx, start_rx) = oneshot::channel::<()>();
    let sink_clone = sink.clone();
    let session_clone = session.clone();
    let session_manager_clone = session_manager.clone();
    let sid = req.session_id.clone();

    tokio::spawn(async move {
        if start_rx.await.is_err() {
            let mut turn = session_clone.active_turn.lock().await;
            *turn = None;
            return;
        }

        let sink = sink_clone;
        let session = session_clone;
        let _session_mgr = session_manager_clone;
        let mut mapper = AcpUpdateMapper::new(sid.clone(), session.cwd.clone());

        // Send state running before polling
        send_session_update(&sink, sid.clone(), state_running_update());

        session.engine.reset_abort();
        let mut stream = session.engine.submit_message_with_overrides(
            &prompt,
            allthecodes_engine::types::config::QuerySource::Sdk,
            allthecodes_engine::types::config::SubmitMessageOverrides::default(),
        );

        let mut terminal_idle_sent = false;
        while let Some(sdk_msg) = futures::StreamExt::next(&mut stream).await {
            let updates = if let allthecodes_types::sdk::SdkMessage::Result(result) = &sdk_msg {
                terminal_idle_sent = true;
                let turn_cancelled = {
                    let turn = session.active_turn.lock().await;
                    turn.as_ref()
                        .map(|handle| handle.cancel_requested)
                        .unwrap_or(false)
                };
                if turn_cancelled || session.engine.abort_reason().is_some() {
                    mapper.map_result(result, Some(v2::StopReason::Cancelled))
                } else {
                    mapper.map_result(result, None)
                }
            } else {
                mapper.map_message(&sdk_msg)
            };
            for update in updates {
                send_session_update(&sink, sid.clone(), update);
            }
        }

        let turn_cancelled = {
            let turn = session.active_turn.lock().await;
            turn.as_ref()
                .map(|handle| handle.cancel_requested)
                .unwrap_or(false)
        };
        if turn_cancelled && !terminal_idle_sent {
            // Send cancelled idle state
            send_session_update(
                &sink,
                sid.clone(),
                state_idle_update(Some(v2::StopReason::Cancelled)),
            );
        }

        // Clear active turn
        let mut turn = session.active_turn.lock().await;
        *turn = None;
    });

    Ok(PendingPromptStart::new(prompt_response, start_tx, session))
}
