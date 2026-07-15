//! Shared state for the web server layer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use allthecodes_config::paths;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_types::callbacks::{PermissionRequestPayload, PermissionResponsePayload};
use allthecodes_types::tool_operation::ToolOperationDisplay;
use allthecodes_web_state::WebUiStore;

use crate::ipc_streams::IpcSessionHub;
use crate::serialization::SerializationLayer;
use crate::ws::terminal::{PtyDiagnostics, TerminalManager};

/// An in-memory queue entry used by the queue/edit/send-now API.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub id: String,
    pub text: String,
    pub session_id: String,
    pub created_at: String,
}

/// Shared state passed to all Axum handlers via State extractor.
///
/// The engine is held behind an `RwLock<Arc<QueryEngine>>` so the web layer
/// can swap it out when the user creates a new session or resumes an older
/// one. Individual handlers snapshot the engine via [`WebState::engine`] and
/// operate on that `Arc` for the duration of the request, so a mid-flight
/// swap cannot disturb an in-progress stream.
#[derive(Clone)]
pub struct WebState {
    control_token: Option<Arc<str>>,
    privileged_token: Option<Arc<str>>,
    listener_authority: Option<Arc<str>>,
    /// Application version reported to Web API clients.
    app_version: Arc<str>,
    /// Current engine, swappable between turns.
    pub engine_slot: Arc<RwLock<Arc<QueryEngine>>>,
    /// Flag: is a query currently in progress?
    pub is_streaming: Arc<AtomicBool>,
    /// Session-scoped engines used by concurrent web chat turns.
    pub session_engines: Arc<RwLock<HashMap<String, Arc<QueryEngine>>>>,
    /// Session ids with an active streaming chat turn.
    pub streaming_sessions: Arc<RwLock<HashSet<String>>>,
    /// Session-scoped IPC WebSocket stream hubs.
    pub ipc_session_hubs: Arc<RwLock<HashMap<String, Arc<IpcSessionHub>>>>,
    /// Pending tool permission prompts owned by `/api/chat` SSE turns.
    chat_permissions: Arc<ChatPermissionStore>,
    /// PTY diagnostics for the active TUI WebSocket connection.
    pub pty_diagnostics: PtyDiagnostics,
    /// Multi-session PTY manager used by the terminal panel.
    pub terminal_manager: TerminalManager,
    /// Request serialization and cross-transport ownership coordination.
    pub serialization: SerializationLayer,
    /// Profile-scoped persistence for browser-only UI state.
    pub web_ui_store: WebUiStore,
    /// In-memory prompt queue for the queue/edit/send-now API.
    pub queue: Arc<RwLock<VecDeque<QueueEntry>>>,
    /// Desktop account auth state shared by account auth endpoints.
    pub account_auth: Arc<Mutex<AccountAuthMemory>>,
}

#[derive(Debug, Clone, Default)]
pub struct AccountAuthMemory {
    pub pending: Option<PendingAccountLogin>,
    pub session: Option<AccountAuthSession>,
}

static PLUGIN_ACCOUNT_AUTH_STATES: LazyLock<Mutex<Vec<Weak<Mutex<AccountAuthMemory>>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone)]
pub struct PendingAccountLogin {
    pub account_site_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAuthSession {
    pub account_site_url: String,
    pub access_token: String,
    pub expires_at: String,
    pub user: Value,
    pub subscription: Value,
    pub entitlements: Value,
    pub credits: Value,
    pub agent_collaboration: Value,
}

impl WebState {
    /// Build a new `WebState` from an initial engine.
    pub fn new(engine: Arc<QueryEngine>, is_streaming: Arc<AtomicBool>) -> Self {
        Self::new_with_version(engine, is_streaming, env!("CARGO_PKG_VERSION"))
    }

    /// Build a new `WebState` with an explicit application version.
    pub fn new_with_version(
        engine: Arc<QueryEngine>,
        is_streaming: Arc<AtomicBool>,
        app_version: impl Into<String>,
    ) -> Self {
        let terminal_manager = TerminalManager::default();
        let current_session_id = engine.current_session_id().to_string();
        let mut session_engines = HashMap::new();
        session_engines.insert(current_session_id, engine.clone());
        let state = Self {
            control_token: None,
            privileged_token: None,
            listener_authority: None,
            app_version: Arc::from(app_version.into()),
            engine_slot: Arc::new(RwLock::new(engine)),
            is_streaming,
            session_engines: Arc::new(RwLock::new(session_engines)),
            streaming_sessions: Arc::new(RwLock::new(HashSet::new())),
            ipc_session_hubs: Arc::new(RwLock::new(HashMap::new())),
            chat_permissions: Arc::new(ChatPermissionStore::default()),
            pty_diagnostics: PtyDiagnostics::new(terminal_manager.clone()),
            terminal_manager,
            serialization: SerializationLayer::new(),
            web_ui_store: WebUiStore::new(paths::data_root().join("web").join("state.db")),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            account_auth: Arc::new(Mutex::new(AccountAuthMemory::default())),
        };
        install_plugin_mcp_hooks();
        state.install_plugin_account_token_provider();
        state
    }

    pub fn control_token(&self) -> Option<&str> {
        self.control_token.as_deref()
    }

    pub fn privileged_token(&self) -> Option<&str> {
        self.privileged_token.as_deref()
    }

    pub fn listener_authority(&self) -> Option<&str> {
        self.listener_authority.as_deref()
    }

    /// Require this explicit secret for protected HTTP and WebSocket routes.
    pub fn with_control_token(mut self, token: impl Into<String>) -> Self {
        self.control_token = Some(Arc::from(token.into()));
        self
    }

    /// Require an independent capability for privileged Web operations.
    pub fn with_privileged_token(mut self, token: impl Into<String>) -> Self {
        self.privileged_token = Some(Arc::from(token.into()));
        self
    }

    /// Pin protected requests to the exact authority configured for the listener.
    pub fn with_listener_authority(mut self, authority: impl Into<String>) -> Self {
        self.listener_authority = Some(Arc::from(authority.into()));
        self
    }

    /// Application version reported to Web clients.
    pub fn app_version(&self) -> &str {
        self.app_version.as_ref()
    }

    /// Snapshot the current engine.
    pub fn engine(&self) -> Arc<QueryEngine> {
        self.engine_slot.read().clone()
    }

    /// Replace the current engine (used by new/resume session flows).
    pub fn replace_engine(&self, engine: Arc<QueryEngine>) {
        let previous_session_id = self.engine().current_session_id().to_string();
        self.clear_chat_permissions_for_session(&previous_session_id);
        let session_id = engine.current_session_id().to_string();
        *self.engine_slot.write() = engine.clone();
        self.session_engines.write().insert(session_id, engine);
    }

    /// Return the cached engine for a session, if one has been built.
    pub fn engine_for_session(&self, session_id: &str) -> Option<Arc<QueryEngine>> {
        self.session_engines.read().get(session_id).cloned()
    }

    /// Cache an engine without making it the foreground UI engine.
    pub fn cache_session_engine(&self, engine: Arc<QueryEngine>) {
        let session_id = engine.current_session_id().to_string();
        self.session_engines.write().insert(session_id, engine);
    }

    /// Check whether a specific session has an active streaming turn.
    pub fn is_session_streaming(&self, session_id: &str) -> bool {
        self.streaming_sessions.read().contains(session_id)
    }

    /// Update a session's streaming state and keep the legacy global flag in sync.
    pub fn set_session_streaming(&self, session_id: &str, streaming: bool) {
        let any_streaming = {
            let mut sessions = self.streaming_sessions.write();
            if streaming {
                sessions.insert(session_id.to_string());
            } else {
                sessions.remove(session_id);
            }
            !sessions.is_empty()
        };
        self.is_streaming
            .store(any_streaming, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn ipc_session_hub(&self, session_id: &str) -> Arc<IpcSessionHub> {
        if let Some(hub) = self.ipc_session_hubs.read().get(session_id).cloned() {
            return hub;
        }

        let mut hubs = self.ipc_session_hubs.write();
        hubs.entry(session_id.to_string())
            .or_insert_with(|| IpcSessionHub::new(session_id))
            .clone()
    }

    pub fn insert_chat_permission(
        &self,
        session_id: &str,
        request: &PermissionRequestPayload,
        operation: &ToolOperationDisplay,
        sender: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
    ) -> Result<ChatPermissionRegistration, ChatPermissionRegisterError> {
        self.chat_permissions.insert(
            session_id,
            request,
            operation,
            sender,
            CHAT_PERMISSION_LIFETIME,
        )
    }

    pub fn resolve_chat_permission(
        &self,
        session_id: &str,
        tool_use_id: &str,
        response_binding: Option<&str>,
        response: PermissionResponsePayload,
    ) -> Result<(), ChatPermissionResolveError> {
        self.chat_permissions
            .resolve(session_id, tool_use_id, response_binding, response)
    }

    pub fn remove_chat_permission(&self, session_id: &str, tool_use_id: &str) {
        self.chat_permissions
            .remove_and_deny(session_id, tool_use_id);
    }

    pub fn clear_chat_permissions_for_session(&self, session_id: &str) {
        self.chat_permissions.remove_session_and_deny(session_id);
    }

    fn install_plugin_account_token_provider(&self) {
        PLUGIN_ACCOUNT_AUTH_STATES
            .lock()
            .push(Arc::downgrade(&self.account_auth));
        allthecodes_plugins::set_plugin_account_token_provider(Some(Arc::new(move || {
            let mut states = PLUGIN_ACCOUNT_AUTH_STATES.lock();
            states.retain(|state| state.strong_count() > 0);
            for state in states.iter().rev().filter_map(Weak::upgrade) {
                if let Some(session) = state.lock().session.clone() {
                    if account_auth_session_expired(&session) {
                        continue;
                    }
                    return Ok(session.access_token);
                }
            }
            drop(states);

            stored_account_access_token()?.ok_or_else(|| anyhow!("no desktop account session"))
        })));
    }
}

fn account_auth_session_expired(session: &AccountAuthSession) -> bool {
    let Ok(expires_at) = DateTime::parse_from_rfc3339(&session.expires_at) else {
        return true;
    };
    Utc::now() >= expires_at.with_timezone(&Utc)
}

#[derive(Debug, Deserialize)]
struct StoredAccountAuthFile {
    auth_mode: String,
    tokens: StoredAccountAuthTokens,
}

#[derive(Debug, Deserialize)]
struct StoredAccountAuthTokens {
    access_token: String,
}

fn stored_account_access_token() -> anyhow::Result<Option<String>> {
    let path = paths::data_root().join("auth.json");
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let auth_file: StoredAccountAuthFile = serde_json::from_str(&raw)?;
    if auth_file.auth_mode != "allthecodes" {
        return Ok(None);
    }

    let token = auth_file.tokens.access_token.trim();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token.to_string()))
    }
}

/// Install plugin MCP discovery plus a startup-safe account token provider.
///
/// `WebState` installs a richer provider later that can use its in-memory
/// account session. During early process startup the MCP manager is created
/// before `WebState`, so it still needs discovery hooks and stored auth.
pub fn install_plugin_runtime_hooks() {
    install_plugin_mcp_hooks();
    install_stored_plugin_account_token_provider();
}

fn install_stored_plugin_account_token_provider() {
    allthecodes_plugins::set_plugin_account_token_provider(Some(Arc::new(|| {
        stored_account_access_token()?.ok_or_else(|| anyhow!("no desktop account session"))
    })));
}

fn install_plugin_mcp_hooks() {
    allthecodes_mcp::discovery::set_plugin_hook(allthecodes_plugins::discover_plugin_mcp_servers);
    allthecodes_mcp::discovery::set_scoped_plugin_hook(
        allthecodes_plugins::discover_plugin_mcp_servers_scoped,
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChatPermissionKey {
    session_id: String,
    tool_use_id: String,
}

const CHAT_PERMISSION_LIFETIME: Duration = Duration::from_secs(10 * 60);
const MAX_CHAT_PERMISSION_BINDING_MISMATCHES: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatPermissionRegistration {
    pub response_binding: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatPermissionRegisterError {
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatPermissionResolveError {
    Stale,
    BindingMismatch,
    ExactApprovalIsSingleUse,
    ResponseReceiverClosed,
}

#[derive(Default)]
struct ChatPermissionStore {
    entries: RwLock<HashMap<ChatPermissionKey, PendingChatPermission>>,
}

struct PendingChatPermission {
    sender: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
    exact_approval: bool,
    _request_fingerprint: [u8; 32],
    response_binding_hash: [u8; 32],
    _created_at: Instant,
    expires_at: Instant,
    binding_mismatches: u8,
}

impl ChatPermissionStore {
    fn insert(
        &self,
        session_id: &str,
        request: &PermissionRequestPayload,
        operation: &ToolOperationDisplay,
        sender: tokio::sync::oneshot::Sender<PermissionResponsePayload>,
        lifetime: Duration,
    ) -> Result<ChatPermissionRegistration, ChatPermissionRegisterError> {
        let key = ChatPermissionKey::new(session_id, &request.tool_use_id);
        let now = Instant::now();
        let response_binding = Uuid::new_v4().to_string();
        let exact_approval = request
            .security
            .as_ref()
            .is_some_and(|security| security.exact_approval);
        let request_fingerprint = request_fingerprint(session_id, request, operation);

        let mut entries = self.entries.write();
        if entries
            .get(&key)
            .is_some_and(|entry| entry.expires_at > now)
        {
            return Err(ChatPermissionRegisterError::Duplicate);
        }
        // Dropping an expired sender is fail-closed for its waiting callback.
        entries.remove(&key);
        entries.insert(
            key,
            PendingChatPermission {
                sender,
                exact_approval,
                _request_fingerprint: request_fingerprint,
                response_binding_hash: digest(response_binding.as_bytes()),
                _created_at: now,
                expires_at: now + lifetime,
                binding_mismatches: 0,
            },
        );

        Ok(ChatPermissionRegistration { response_binding })
    }

    fn resolve(
        &self,
        session_id: &str,
        tool_use_id: &str,
        response_binding: Option<&str>,
        response: PermissionResponsePayload,
    ) -> Result<(), ChatPermissionResolveError> {
        let key = ChatPermissionKey::new(session_id, tool_use_id);
        let decision = response.normalized_decision();
        let now = Instant::now();
        let mut denied_sender = None;
        let mut no_sender_error = ChatPermissionResolveError::BindingMismatch;

        let sender = {
            let mut entries = self.entries.write();
            let Some(entry) = entries.get(&key) else {
                return Err(ChatPermissionResolveError::Stale);
            };

            if entry.expires_at <= now {
                no_sender_error = ChatPermissionResolveError::Stale;
                denied_sender = entries.remove(&key).map(|entry| entry.sender);
                None
            } else if entry.exact_approval && decision == "always_allow" {
                return Err(ChatPermissionResolveError::ExactApprovalIsSingleUse);
            } else {
                let binding_must_match = (entry.exact_approval && decision == "allow")
                    || (!entry.exact_approval && response_binding.is_some());
                let binding_matches = response_binding.is_some_and(|binding| {
                    constant_time_digest_eq(
                        &digest(binding.as_bytes()),
                        &entry.response_binding_hash,
                    )
                });

                if binding_must_match && !binding_matches {
                    let should_deny = if let Some(entry) = entries.get_mut(&key) {
                        entry.binding_mismatches = entry.binding_mismatches.saturating_add(1);
                        entry.binding_mismatches >= MAX_CHAT_PERMISSION_BINDING_MISMATCHES
                    } else {
                        no_sender_error = ChatPermissionResolveError::Stale;
                        false
                    };
                    if should_deny {
                        denied_sender = entries.remove(&key).map(|entry| entry.sender);
                    }
                    None
                } else {
                    entries.remove(&key).map(|entry| entry.sender)
                }
            }
        };

        if let Some(sender) = denied_sender {
            let _ = sender.send(PermissionResponsePayload::deny());
        }

        let Some(sender) = sender else {
            return Err(no_sender_error);
        };

        sender
            .send(response)
            .map_err(|_| ChatPermissionResolveError::ResponseReceiverClosed)
    }

    fn remove_and_deny(&self, session_id: &str, tool_use_id: &str) {
        if let Some(entry) = self
            .entries
            .write()
            .remove(&ChatPermissionKey::new(session_id, tool_use_id))
        {
            let _ = entry.sender.send(PermissionResponsePayload::deny());
        }
    }

    fn remove_session_and_deny(&self, session_id: &str) {
        let removed = {
            let mut entries = self.entries.write();
            let keys = entries
                .keys()
                .filter(|key| key.session_id == session_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| entries.remove(&key))
                .collect::<Vec<_>>()
        };
        for entry in removed {
            let _ = entry.sender.send(PermissionResponsePayload::deny());
        }
    }
}

fn request_fingerprint(
    session_id: &str,
    request: &PermissionRequestPayload,
    operation: &ToolOperationDisplay,
) -> [u8; 32] {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "tool_use_id": request.tool_use_id,
        "tool_name": request.tool_name,
        "tool_input": request.tool_input,
        "operation": operation,
        "security": request.security,
    });
    digest(canonical.to_string().as_bytes())
}

fn digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn constant_time_digest_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut different = 0_u8;
    for (left, right) in left.iter().zip(right.iter()) {
        different |= left ^ right;
    }
    different == 0
}

#[cfg(test)]
mod chat_permission_tests {
    use super::*;
    use allthecodes_types::callbacks::SecurityDecisionDisplay;
    use allthecodes_types::tool_operation::{
        OperationConfidence, OperationKind, OperationRisk, OperationStatus,
    };
    use serde_json::json;

    fn request(session_suffix: &str, exact: bool) -> PermissionRequestPayload {
        PermissionRequestPayload {
            tool_use_id: format!("tool-{session_suffix}"),
            tool_name: "Bash".to_string(),
            tool_input: json!({"command": "cargo test"}),
            message: "run tests".to_string(),
            options: vec!["Allow once".to_string(), "Deny".to_string()],
            operation: None,
            security: exact.then(|| SecurityDecisionDisplay {
                sink: "ShellExec".to_string(),
                decision: "ask".to_string(),
                rule_ids: vec!["workflow-injection".to_string()],
                source_labels: vec!["web".to_string()],
                source_digests: vec!["sha256:abc".to_string()],
                exact_approval: true,
            }),
        }
    }

    fn operation() -> ToolOperationDisplay {
        ToolOperationDisplay {
            kind: OperationKind::Permission,
            subtype: None,
            status: OperationStatus::InProgress,
            risk: OperationRisk::High,
            confidence: OperationConfidence::High,
            label: "Permission".to_string(),
            target: None,
            command_summary: Some("cargo test".to_string()),
            raw_tool_name: "Bash".to_string(),
        }
    }

    #[tokio::test]
    async fn duplicate_registration_does_not_replace_live_sender() {
        let store = ChatPermissionStore::default();
        let request = request("one", false);
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        store
            .insert(
                "session-1",
                &request,
                &operation(),
                first_tx,
                Duration::from_secs(60),
            )
            .unwrap();
        let (second_tx, _second_rx) = tokio::sync::oneshot::channel();
        assert_eq!(
            store.insert(
                "session-1",
                &request,
                &operation(),
                second_tx,
                Duration::from_secs(60),
            ),
            Err(ChatPermissionRegisterError::Duplicate)
        );

        store
            .resolve(
                "session-1",
                &request.tool_use_id,
                None,
                PermissionResponsePayload::deny(),
            )
            .unwrap();
        assert_eq!(first_rx.await.unwrap().normalized_decision(), "deny");
    }

    #[tokio::test]
    async fn exact_allow_requires_matching_binding_and_is_consumed_once() {
        let store = ChatPermissionStore::default();
        let request = request("exact", true);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let registration = store
            .insert(
                "session-1",
                &request,
                &operation(),
                tx,
                Duration::from_secs(60),
            )
            .unwrap();

        assert_eq!(
            store.resolve(
                "session-1",
                &request.tool_use_id,
                Some("wrong"),
                PermissionResponsePayload::decision("allow"),
            ),
            Err(ChatPermissionResolveError::BindingMismatch)
        );
        store
            .resolve(
                "session-1",
                &request.tool_use_id,
                Some(&registration.response_binding),
                PermissionResponsePayload::decision("allow"),
            )
            .unwrap();
        assert_eq!(rx.await.unwrap().normalized_decision(), "allow");
        assert_eq!(
            store.resolve(
                "session-1",
                &request.tool_use_id,
                Some(&registration.response_binding),
                PermissionResponsePayload::decision("allow"),
            ),
            Err(ChatPermissionResolveError::Stale)
        );
    }

    #[tokio::test]
    async fn exact_always_allow_is_rejected_without_consuming_deny_path() {
        let store = ChatPermissionStore::default();
        let request = request("single-use", true);
        let (tx, rx) = tokio::sync::oneshot::channel();
        store
            .insert(
                "session-1",
                &request,
                &operation(),
                tx,
                Duration::from_secs(60),
            )
            .unwrap();
        assert_eq!(
            store.resolve(
                "session-1",
                &request.tool_use_id,
                None,
                PermissionResponsePayload::decision("always_allow"),
            ),
            Err(ChatPermissionResolveError::ExactApprovalIsSingleUse)
        );
        store
            .resolve(
                "session-1",
                &request.tool_use_id,
                None,
                PermissionResponsePayload::deny(),
            )
            .unwrap();
        assert_eq!(rx.await.unwrap().normalized_decision(), "deny");
    }

    #[test]
    fn session_scope_prevents_cross_session_resolution() {
        let store = ChatPermissionStore::default();
        let request = request("scope", false);
        let (tx, _rx) = tokio::sync::oneshot::channel();
        store
            .insert(
                "session-1",
                &request,
                &operation(),
                tx,
                Duration::from_secs(60),
            )
            .unwrap();
        assert_eq!(
            store.resolve(
                "session-2",
                &request.tool_use_id,
                None,
                PermissionResponsePayload::deny(),
            ),
            Err(ChatPermissionResolveError::Stale)
        );
    }
}

impl ChatPermissionKey {
    fn new(session_id: &str, tool_use_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            tool_use_id: tool_use_id.to_string(),
        }
    }
}
