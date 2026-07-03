//! ACP permission bridge.
//!
//! Installs per-session callbacks on each ACP session engine that convert
//! internal permission requests into ACP `session/request_permission` client
//! requests, and maps client responses back to `PermissionResponsePayload`.
//!
//! The permission request is sent as a separate JSON-RPC request to the client
//! (not wrapped inside a SessionUpdate). The wrapper sends `state_update
//! requires_action` before sending the request, and `state_update running`
//! after the response is received.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v2::{
    AgentRequest, ContentBlock, MessageId, PermissionOption, PermissionOptionId,
    PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, RequiresActionStateUpdate, SessionUpdate, StateUpdate, TextContent,
};
use allthecodes_types::callbacks::{AskUserRequestPayload, PermissionEventPayload};
use tokio::sync::Mutex;

use crate::session::AcpSession;
use crate::transport::AcpSink;
use crate::PermissionRequestPayload;
use crate::PermissionResponsePayload;

/// Pending permission request state.
#[derive(Debug)]
pub struct PendingPermission {
    pub request_id: String,
    pub session_id: String,
    pub tool_use_id: String,
    pub resolve_tx: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
}

#[derive(Default)]
struct PendingPermissionState {
    by_request_id: HashMap<String, PendingPermission>,
    by_tool: HashMap<(String, String), String>,
}

/// Manages in-flight permission requests for all ACP sessions.
pub struct AcpPermissionManager {
    pending: Mutex<PendingPermissionState>,
    next_request: AtomicU64,
}

impl AcpPermissionManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(PendingPermissionState::default()),
            next_request: AtomicU64::new(0),
        }
    }

    /// Run one ACP permission transaction and return the engine-facing response.
    pub async fn request_permission(
        &self,
        session: &AcpSession,
        sink: &AcpSink,
        payload: PermissionRequestPayload,
    ) -> PermissionResponsePayload {
        let (resolve_tx, resolve_rx) = tokio::sync::oneshot::channel();
        let request_id = self.register(session, &payload, resolve_tx).await;
        let acp_request = build_permission_request(session, &payload, &payload.options);

        if !send_session_update(
            sink,
            session.session_id.clone(),
            SessionUpdate::StateUpdate(StateUpdate::RequiresAction(
                RequiresActionStateUpdate::new(),
            )),
        ) {
            self.cancel_request_id_str(&request_id).await;
            return PermissionResponsePayload::deny();
        }

        let request_frame = crate::jsonrpc::build_agent_request(
            &RequestId::Str(request_id.clone()),
            AgentRequest::RequestPermissionRequest(Box::new(acp_request)),
        );
        if !sink.send(request_frame) {
            self.cancel_request_id_str(&request_id).await;
            return PermissionResponsePayload::deny();
        }

        let response = resolve_rx
            .await
            .unwrap_or_else(|_| PermissionResponsePayload::deny());

        let _ = send_session_update(
            sink,
            session.session_id.clone(),
            crate::updates::state_running_update(),
        );

        response
    }

    /// Register a pending permission request and return its ACP request id.
    async fn register(
        &self,
        session: &AcpSession,
        payload: &PermissionRequestPayload,
        resolve_tx: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
    ) -> String {
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        let session_id = session.session_id.0.to_string();
        let tool_use_id = payload.tool_use_id.clone();
        let request_id = format!("allthecodes-permission-{session_id}-{sequence}");
        let tool_key = (session_id.clone(), tool_use_id.clone());

        let mut state = self.pending.lock().await;
        state.by_tool.insert(tool_key, request_id.clone());
        state.by_request_id.insert(
            request_id.clone(),
            PendingPermission {
                request_id: request_id.clone(),
                session_id,
                tool_use_id,
                resolve_tx,
            },
        );
        request_id
    }

    /// Remove and return a pending permission, or None if already resolved.
    pub async fn take(&self, session_id: &str, tool_use_id: &str) -> Option<PendingPermission> {
        let mut state = self.pending.lock().await;
        let tool_key = (session_id.to_string(), tool_use_id.to_string());
        let request_id = state.by_tool.remove(&tool_key)?;
        state.by_request_id.remove(&request_id)
    }

    /// Resolve a JSON-RPC permission response from the ACP client.
    pub async fn resolve_client_response(
        &self,
        request_id: &RequestId,
        result: serde_json::Value,
    ) -> bool {
        let Ok(response) = serde_json::from_value::<RequestPermissionResponse>(result) else {
            return self.cancel_request_id(request_id).await;
        };
        self.resolve_request_id(request_id, map_permission_outcome(&response.outcome))
            .await
    }

    pub async fn resolve_request_id(
        &self,
        request_id: &RequestId,
        response: PermissionResponsePayload,
    ) -> bool {
        let Some(pending) = self.take_by_request_id(request_id).await else {
            return false;
        };
        let _ = pending.resolve_tx.send(response);
        true
    }

    /// Cancel a pending ACP-originated request id and resolve it as deny.
    pub async fn cancel_request_id(&self, request_id: &RequestId) -> bool {
        self.cancel_request_id_str(&request_id_key(request_id))
            .await
    }

    async fn cancel_request_id_str(&self, request_id: &str) -> bool {
        let Some(pending) = self.take_by_request_id_str(request_id).await else {
            return false;
        };
        let _ = pending.resolve_tx.send(PermissionResponsePayload::deny());
        true
    }

    async fn take_by_request_id(&self, request_id: &RequestId) -> Option<PendingPermission> {
        self.take_by_request_id_str(&request_id_key(request_id))
            .await
    }

    async fn take_by_request_id_str(&self, request_id: &str) -> Option<PendingPermission> {
        let mut state = self.pending.lock().await;
        let pending = state.by_request_id.remove(request_id)?;
        state
            .by_tool
            .remove(&(pending.session_id.clone(), pending.tool_use_id.clone()));
        Some(pending)
    }

    /// Cancel all pending permissions for a given session (e.g. on session/close).
    pub async fn cancel_all(&self, session_id: &str) {
        let request_ids = {
            let state = self.pending.lock().await;
            state
                .by_request_id
                .values()
                .filter(|pending| pending.session_id == session_id)
                .map(|pending| pending.request_id.clone())
                .collect::<Vec<_>>()
        };
        for request_id in request_ids {
            self.cancel_request_id_str(&request_id).await;
        }
    }

    /// Cancel all pending permissions for all sessions (e.g. ACP stdin EOF).
    pub async fn cancel_all_sessions(&self) {
        let request_ids = {
            let state = self.pending.lock().await;
            state.by_request_id.keys().cloned().collect::<Vec<_>>()
        };
        for request_id in request_ids {
            self.cancel_request_id_str(&request_id).await;
        }
    }

    pub async fn has_pending(&self) -> bool {
        !self.pending.lock().await.by_request_id.is_empty()
    }
}

/// Build an ACP `session/request_permission` client request from an internal
/// `PermissionRequestPayload`.
pub fn build_permission_request(
    session: &AcpSession,
    payload: &PermissionRequestPayload,
    options: &[String],
) -> RequestPermissionRequest {
    let acp_options: Vec<PermissionOption> = options
        .iter()
        .map(|opt| {
            let (option_id, kind) = match opt.as_str() {
                "allow" => ("allow_once", PermissionOptionKind::AllowOnce),
                "always_allow" => ("allow_always", PermissionOptionKind::AllowAlways),
                "deny" => ("deny_once", PermissionOptionKind::RejectOnce),
                "always_deny" => ("deny_always", PermissionOptionKind::RejectAlways),
                "auto_review" => (
                    "_allthecodes_auto_review",
                    PermissionOptionKind::Other("_allthecodes_auto_review".to_string()),
                ),
                other => (other, PermissionOptionKind::Other(other.to_string())),
            };
            PermissionOption::new(PermissionOptionId::new(option_id), opt.to_string(), kind)
        })
        .collect();

    RequestPermissionRequest::new(
        session.session_id.clone(),
        crate::tool_calls::build_tool_call_update(
            payload.tool_use_id.clone(),
            &payload.tool_name,
            &payload.tool_input,
            &session.cwd,
        ),
        acp_options,
    )
}

/// Map an ACP `RequestPermissionOutcome` to an allthecodes `PermissionResponsePayload`.
pub fn map_permission_outcome(outcome: &RequestPermissionOutcome) -> PermissionResponsePayload {
    match outcome {
        RequestPermissionOutcome::Cancelled => PermissionResponsePayload::deny(),
        RequestPermissionOutcome::Selected(selected) => {
            let option_id = selected.option_id.0.to_string();
            match option_id.as_str() {
                "allow_once" => PermissionResponsePayload::decision("allow"),
                "allow_always" => PermissionResponsePayload::decision("always_allow"),
                "deny_once" => PermissionResponsePayload::deny(),
                "deny_always" => PermissionResponsePayload::decision("always_deny"),
                "_allthecodes_auto_review" => PermissionResponsePayload::auto_review(),
                _ => PermissionResponsePayload::deny(),
            }
        }
        _ => PermissionResponsePayload::deny(),
    }
}

/// Install ACP-facing callbacks on a session engine.
pub fn install_permission_callbacks(
    session: &Arc<AcpSession>,
    sink: AcpSink,
    permission_manager: Arc<AcpPermissionManager>,
) {
    let permission_session = session.clone();
    let permission_sink = sink.clone();
    let permission_manager_for_cb = permission_manager.clone();
    let callback: allthecodes_engine::types::tool::PermissionCallback =
        Arc::new(move |request: PermissionRequestPayload| {
            let session = permission_session.clone();
            let sink = permission_sink.clone();
            let manager = permission_manager_for_cb.clone();
            Box::pin(async move { manager.request_permission(&session, &sink, request).await })
        });
    session.engine.set_permission_callback(callback);

    let ask_session = session.clone();
    let ask_sink = sink.clone();
    let ask_callback: allthecodes_engine::types::tool::AskUserCallback =
        Arc::new(move |request: AskUserRequestPayload| {
            let session_id = ask_session.session_id.clone();
            let sink = ask_sink.clone();
            Box::pin(async move {
                let text = if request.question.trim().is_empty() {
                    "User input requested".to_string()
                } else {
                    request.question
                };
                let _ = send_session_update(
                    &sink,
                    session_id,
                    SessionUpdate::AgentThought(
                        agent_client_protocol_schema::v2::AgentThought::new(MessageId::new(
                            format!("ask-user-{}", uuid::Uuid::new_v4()),
                        ))
                        .content(vec![ContentBlock::Text(TextContent::new(text))]),
                    ),
                );
                String::new()
            })
        });
    session.engine.set_ask_user_callback(ask_callback);

    let event_session_id = session.session_id.clone();
    let event_sink = sink.clone();
    let event_callback: allthecodes_engine::types::tool::PermissionEventCallback =
        Arc::new(move |event: PermissionEventPayload| {
            let text = permission_event_text(&event);
            let mut meta = serde_json::Map::new();
            meta.insert("kind".into(), serde_json::json!("permission_event"));
            let update = SessionUpdate::AgentThought(
                agent_client_protocol_schema::v2::AgentThought::new(MessageId::new(format!(
                    "permission-event-{}",
                    uuid::Uuid::new_v4()
                )))
                .content(vec![ContentBlock::Text(TextContent::new(text))])
                .meta(meta),
            );
            let _ = send_session_update(&event_sink, event_session_id.clone(), update);
        });
    session.engine.set_permission_event_callback(event_callback);
}

pub fn tool_progress_text(data: &serde_json::Value) -> String {
    data.get("message")
        .and_then(|value| value.as_str())
        .or_else(|| data.get("output").and_then(|value| value.as_str()))
        .or_else(|| data.get("text").and_then(|value| value.as_str()))
        .map(str::to_string)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| data.to_string())
}

fn permission_event_text(event: &PermissionEventPayload) -> String {
    match event {
        PermissionEventPayload::HookDecision { event } => {
            format!("Permission hook decision: {}", event.decision)
        }
        PermissionEventPayload::DecisionDebug { event } => {
            format!("Permission decision: {}: {}", event.behavior, event.reason)
        }
        PermissionEventPayload::AutoReview { event } => {
            format!("Permission auto-review: {}", event.status)
        }
    }
}

fn send_session_update(
    sink: &AcpSink,
    session_id: agent_client_protocol_schema::v2::SessionId,
    update: SessionUpdate,
) -> bool {
    let notification =
        agent_client_protocol_schema::v2::UpdateSessionNotification::new(session_id, update);
    sink.send(crate::jsonrpc::build_agent_notification(
        agent_client_protocol_schema::v2::AgentNotification::UpdateSessionNotification(Box::new(
            notification,
        )),
    ))
}

pub fn request_id_key(id: &RequestId) -> String {
    match id {
        RequestId::Str(value) => value.clone(),
        RequestId::Number(value) => value.to_string(),
        RequestId::Null => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol_schema::v2::ToolCallUpdate;

    #[test]
    fn allow_once_maps_to_allow() {
        let outcome = RequestPermissionOutcome::Selected(
            agent_client_protocol_schema::v2::SelectedPermissionOutcome::new(
                PermissionOptionId::new("allow_once"),
            ),
        );
        let resp = map_permission_outcome(&outcome);
        assert_eq!(resp.decision, "allow");
    }

    #[test]
    fn cancelled_maps_to_deny() {
        let outcome = RequestPermissionOutcome::Cancelled;
        let resp = map_permission_outcome(&outcome);
        assert_eq!(resp.decision, "deny");
    }

    #[test]
    fn deny_once_maps_to_deny() {
        let outcome = RequestPermissionOutcome::Selected(
            agent_client_protocol_schema::v2::SelectedPermissionOutcome::new(
                PermissionOptionId::new("deny_once"),
            ),
        );
        let resp = map_permission_outcome(&outcome);
        assert_eq!(resp.decision, "deny");
    }

    #[test]
    fn always_allow_maps_to_always_allow() {
        let outcome = RequestPermissionOutcome::Selected(
            agent_client_protocol_schema::v2::SelectedPermissionOutcome::new(
                PermissionOptionId::new("allow_always"),
            ),
        );
        let resp = map_permission_outcome(&outcome);
        assert_eq!(resp.decision, "always_allow");
    }

    #[test]
    fn permission_request_has_required_fields() {
        let session_id = agent_client_protocol_schema::v2::SessionId::new("test-session");
        let acp_options: Vec<PermissionOption> = vec![
            PermissionOption::new(
                PermissionOptionId::new("allow_once"),
                "Allow",
                PermissionOptionKind::AllowOnce,
            ),
            PermissionOption::new(
                PermissionOptionId::new("deny_once"),
                "Deny",
                PermissionOptionKind::RejectOnce,
            ),
        ];

        let req =
            RequestPermissionRequest::new(session_id, ToolCallUpdate::new("call-1"), acp_options);

        assert_eq!(req.options.len(), 2);
    }
}
