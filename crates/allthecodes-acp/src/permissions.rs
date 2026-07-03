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

use agent_client_protocol_schema::v2::{
    PermissionOption, PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, ToolCallUpdate,
};
use tokio::sync::RwLock;

use crate::session::AcpSession;
use crate::PermissionRequestPayload;
use crate::PermissionResponsePayload;

/// Pending permission request state.
#[derive(Debug)]
pub struct PendingPermission {
    pub session_id: String,
    pub tool_use_id: String,
    pub resolve_tx: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
}

impl PendingPermission {
    /// Take the resolve sender, replacing it with a no-op sender.
    pub fn take_resolve(&mut self) -> tokio::sync::oneshot::Sender<PermissionResponsePayload> {
        let (tx, _rx) = tokio::sync::oneshot::channel();
        std::mem::replace(&mut self.resolve_tx, tx)
    }
}

/// Manages in-flight permission requests for all ACP sessions.
pub struct AcpPermissionManager {
    pending: RwLock<std::collections::HashMap<String, PendingPermission>>,
}

impl AcpPermissionManager {
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Register a pending permission request and return its notification key.
    pub async fn register(
        &self,
        session_id: &str,
        tool_use_id: &str,
        resolve_tx: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
    ) -> String {
        let key = format!("{session_id}:{tool_use_id}");
        self.pending.write().await.insert(
            key.clone(),
            PendingPermission {
                session_id: session_id.to_string(),
                tool_use_id: tool_use_id.to_string(),
                resolve_tx,
            },
        );
        key
    }

    /// Remove and return a pending permission, or None if already resolved.
    pub async fn take(&self, session_id: &str, tool_use_id: &str) -> Option<PendingPermission> {
        let key = format!("{session_id}:{tool_use_id}");
        self.pending.write().await.remove(&key)
    }

    /// Cancel all pending permissions for a given session (e.g. on session/close).
    pub async fn cancel_all(&self, session_id: &str) {
        let mut map = self.pending.write().await;
        map.retain(|_k, p| {
            if p.session_id == session_id {
                let _ = p.take_resolve().send(PermissionResponsePayload::deny());
                false
            } else {
                true
            }
        });
    }
}

/// Build an ACP `session/request_permission` client request from an internal
/// `PermissionRequestPayload`.
pub fn build_permission_request(
    session: &AcpSession,
    payload: &PermissionRequestPayload,
    _options: &[String],
) -> RequestPermissionRequest {
    let acp_options: Vec<PermissionOption> = _options
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
        ToolCallUpdate::new(payload.tool_use_id.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;

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
