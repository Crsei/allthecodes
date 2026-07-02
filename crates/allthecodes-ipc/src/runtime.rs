//! Headless IPC runtime state.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use allthecodes_ipc_protocol::{
    legacy_frontend_to_payload, payload_to_legacy_backend, BackendMessage, ClientHello,
    ClientRequestEnvelope, ClientResponseEnvelope, FrontendMessage, IpcPayload,
    IpcPayloadAdapterError, LaggedEvent, ServerCapabilities, ServerErrorEnvelope, ServerReady,
    ServerRequestEnvelope, ServerRequestMethod, IPC_V2_PROTOCOL_VERSION,
};
use allthecodes_protocol::{ApiError, ClientResponse};
use allthecodes_types::callbacks::{
    AskUserRequestPayload, PermissionRequestPayload, PermissionResponsePayload,
};
use parking_lot::Mutex;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot};

use crate::transport::{classify_event, event_type, EventClass};

const DEFAULT_SERVER_REQUEST_TIMEOUT_MS: u64 = 30_000;

pub type PendingPermissions =
    Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponsePayload>>>>;
pub type PendingQuestions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScopedInteractionKey {
    session_id: String,
    turn_id: String,
    id: String,
}

impl ScopedInteractionKey {
    fn new(session_id: &str, turn_id: &str, id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            id: id.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct PendingInteractions {
    legacy_permissions: PendingPermissions,
    legacy_questions: PendingQuestions,
    scoped_permissions:
        Arc<Mutex<HashMap<ScopedInteractionKey, oneshot::Sender<PermissionResponsePayload>>>>,
    scoped_questions: Arc<Mutex<HashMap<ScopedInteractionKey, oneshot::Sender<String>>>>,
    server_requests: Arc<Mutex<HashMap<String, ServerRequestEnvelope>>>,
}

impl PendingInteractions {
    pub fn new() -> Self {
        Self {
            legacy_permissions: Arc::new(Mutex::new(HashMap::new())),
            legacy_questions: Arc::new(Mutex::new(HashMap::new())),
            scoped_permissions: Arc::new(Mutex::new(HashMap::new())),
            scoped_questions: Arc::new(Mutex::new(HashMap::new())),
            server_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn legacy_permissions(&self) -> PendingPermissions {
        self.legacy_permissions.clone()
    }

    pub fn legacy_questions(&self) -> PendingQuestions {
        self.legacy_questions.clone()
    }

    pub fn pending_server_request(&self, request_id: &str) -> Option<ServerRequestEnvelope> {
        self.server_requests.lock().get(request_id).cloned()
    }

    pub fn insert_server_request(&self, request: ServerRequestEnvelope) {
        self.server_requests
            .lock()
            .insert(request.request_id.clone(), request);
    }

    pub fn insert_scoped_permission(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_use_id: &str,
        sender: oneshot::Sender<PermissionResponsePayload>,
    ) {
        self.scoped_permissions.lock().insert(
            ScopedInteractionKey::new(session_id, turn_id, tool_use_id),
            sender,
        );
    }

    pub fn insert_legacy_permission(
        &self,
        tool_use_id: String,
        sender: oneshot::Sender<PermissionResponsePayload>,
    ) {
        self.legacy_permissions.lock().insert(tool_use_id, sender);
    }

    pub fn insert_scoped_question(
        &self,
        session_id: &str,
        turn_id: &str,
        id: &str,
        sender: oneshot::Sender<String>,
    ) {
        self.scoped_questions
            .lock()
            .insert(ScopedInteractionKey::new(session_id, turn_id, id), sender);
    }

    pub fn insert_legacy_question(&self, id: String, sender: oneshot::Sender<String>) {
        self.legacy_questions.lock().insert(id, sender);
    }

    pub fn complete_permission(
        &self,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        tool_use_id: &str,
        response: PermissionResponsePayload,
    ) -> bool {
        if let (Some(session_id), Some(turn_id)) = (session_id, turn_id) {
            let key = ScopedInteractionKey::new(session_id, turn_id, tool_use_id);
            if let Some(tx) = self.scoped_permissions.lock().remove(&key) {
                self.server_requests.lock().remove(tool_use_id);
                return tx.send(response).is_ok();
            }
        }

        self.legacy_permissions
            .lock()
            .remove(tool_use_id)
            .map(|tx| {
                self.server_requests.lock().remove(tool_use_id);
                tx.send(response).is_ok()
            })
            .unwrap_or(false)
    }

    pub fn complete_question(
        &self,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        id: &str,
        text: String,
    ) -> bool {
        if let (Some(session_id), Some(turn_id)) = (session_id, turn_id) {
            let key = ScopedInteractionKey::new(session_id, turn_id, id);
            if let Some(tx) = self.scoped_questions.lock().remove(&key) {
                self.server_requests.lock().remove(id);
                return tx.send(text).is_ok();
            }
        }

        self.legacy_questions
            .lock()
            .remove(id)
            .map(|tx| {
                self.server_requests.lock().remove(id);
                tx.send(text).is_ok()
            })
            .unwrap_or(false)
    }

    pub fn try_answer_any_question(&self, text: String) -> Option<String> {
        let mut pending = self.legacy_questions.lock();
        let pending_id = pending.keys().next().cloned()?;
        let tx = pending.remove(&pending_id)?;
        drop(pending);
        self.server_requests.lock().remove(&pending_id);
        let _ = tx.send(text);
        Some(pending_id)
    }

    pub fn cleanup(&self) -> PendingCleanup {
        let mut cleanup = PendingCleanup::default();

        for (_, sender) in self.legacy_permissions.lock().drain() {
            let _ = sender.send(PermissionResponsePayload::deny());
            cleanup.permissions += 1;
        }
        for (_, sender) in self.scoped_permissions.lock().drain() {
            let _ = sender.send(PermissionResponsePayload::deny());
            cleanup.permissions += 1;
        }
        for (_, sender) in self.legacy_questions.lock().drain() {
            let _ = sender.send(String::new());
            cleanup.questions += 1;
        }
        for (_, sender) in self.scoped_questions.lock().drain() {
            let _ = sender.send(String::new());
            cleanup.questions += 1;
        }
        cleanup.server_requests = self.server_requests.lock().drain().count();

        cleanup
    }
}

impl Default for PendingInteractions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TurnRuntime {
    pub session_id: String,
    pub turn_id: String,
    pub run_id: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PendingCleanup {
    pub permissions: usize,
    pub questions: usize,
    pub server_requests: usize,
}

#[derive(Clone)]
pub struct SessionRuntime {
    session_id: String,
    run_id: String,
    current_turn: Arc<Mutex<Option<TurnRuntime>>>,
    pending: PendingInteractions,
}

impl SessionRuntime {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: uuid::Uuid::new_v4().to_string(),
            current_turn: Arc::new(Mutex::new(None)),
            pending: PendingInteractions::new(),
        }
    }

    pub fn from_ready_message(message: &BackendMessage) -> Self {
        let session_id = match message {
            BackendMessage::Ready { session_id, .. } => session_id.clone(),
            _ => "legacy-session".to_string(),
        };
        Self::new(session_id)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn pending_interactions(&self) -> &PendingInteractions {
        &self.pending
    }

    pub fn begin_turn(&self, turn_id: impl Into<String>) -> TurnRuntime {
        let turn = TurnRuntime {
            session_id: self.session_id.clone(),
            turn_id: turn_id.into(),
            run_id: self.run_id.clone(),
        };
        *self.current_turn.lock() = Some(turn.clone());
        turn
    }

    pub fn current_turn(&self) -> Option<TurnRuntime> {
        self.current_turn.lock().clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcRuntimeError {
    UnsupportedProtocolVersion { supported_versions: Vec<u16> },
    InboundSequenceNotMonotonic { previous: u64, current: u64 },
    OutboundClosed,
    ResponseSerialization { message: String },
    Adapter(IpcPayloadAdapterError),
}

#[derive(Clone)]
pub struct IpcOutboundQueue {
    sender: mpsc::Sender<BackendMessage>,
    dropped_best_effort: Arc<Mutex<DroppedBestEffort>>,
}

#[derive(Debug, Default)]
struct DroppedBestEffort {
    skipped: u64,
    last_dropped_type: Option<&'static str>,
}

impl DroppedBestEffort {
    fn record(&mut self, message: &BackendMessage) {
        self.skipped += 1;
        self.last_dropped_type = Some(event_type(message));
    }

    fn take_lagged(&mut self) -> Option<LaggedEvent> {
        if self.skipped == 0 {
            return None;
        }

        let lagged = LaggedEvent {
            skipped: self.skipped,
            last_dropped_type: self.last_dropped_type.map(str::to_string),
        };
        self.skipped = 0;
        self.last_dropped_type = None;
        Some(lagged)
    }
}

impl IpcOutboundQueue {
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<BackendMessage>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                dropped_best_effort: Arc::new(Mutex::new(DroppedBestEffort::default())),
            },
            receiver,
        )
    }

    pub async fn send(&self, message: BackendMessage) -> Result<(), IpcRuntimeError> {
        match classify_event(&message) {
            EventClass::Lossless => self.send_lossless(message).await,
            EventClass::BestEffort => self.send_best_effort(message),
        }
    }

    pub async fn send_lossless(&self, message: BackendMessage) -> Result<(), IpcRuntimeError> {
        let lagged = {
            let mut dropped = self.dropped_best_effort.lock();
            dropped.take_lagged()
        };

        if let Some(lagged) = lagged {
            let lagged_message = payload_to_legacy_backend(&IpcPayload::Lagged(lagged))
                .map_err(IpcRuntimeError::Adapter)?;
            self.sender
                .send(lagged_message)
                .await
                .map_err(|_| IpcRuntimeError::OutboundClosed)?;
        }

        self.sender
            .send(message)
            .await
            .map_err(|_| IpcRuntimeError::OutboundClosed)
    }

    pub fn send_best_effort(&self, message: BackendMessage) -> Result<(), IpcRuntimeError> {
        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(message)) => {
                self.dropped_best_effort.lock().record(&message);
                Ok(())
            }
            Err(TrySendError::Closed(_)) => Err(IpcRuntimeError::OutboundClosed),
        }
    }
}

#[derive(Clone)]
pub struct IpcRuntime {
    session: SessionRuntime,
    outbound: IpcOutboundQueue,
    next_outbound_seq: Arc<AtomicU64>,
    last_inbound_seq: Arc<Mutex<u64>>,
    server_request_timeout: Duration,
}

impl IpcRuntime {
    pub fn new(
        session_id: impl Into<String>,
        outbound_capacity: usize,
    ) -> (Self, mpsc::Receiver<BackendMessage>) {
        let (outbound, receiver) = IpcOutboundQueue::channel(outbound_capacity);
        (
            Self {
                session: SessionRuntime::new(session_id),
                outbound,
                next_outbound_seq: Arc::new(AtomicU64::new(1)),
                last_inbound_seq: Arc::new(Mutex::new(0)),
                server_request_timeout: Duration::from_millis(DEFAULT_SERVER_REQUEST_TIMEOUT_MS),
            },
            receiver,
        )
    }

    #[cfg(test)]
    fn with_server_request_timeout(
        session_id: impl Into<String>,
        outbound_capacity: usize,
        timeout: Duration,
    ) -> (Self, mpsc::Receiver<BackendMessage>) {
        let (mut runtime, receiver) = Self::new(session_id, outbound_capacity);
        runtime.server_request_timeout = timeout;
        (runtime, receiver)
    }

    pub fn session(&self) -> &SessionRuntime {
        &self.session
    }

    pub fn pending_interactions(&self) -> &PendingInteractions {
        self.session.pending_interactions()
    }

    pub fn next_outbound_seq(&self) -> u64 {
        self.next_outbound_seq.fetch_add(1, Ordering::SeqCst)
    }

    pub fn verify_inbound_seq(&self, seq: u64) -> Result<(), IpcRuntimeError> {
        let mut last = self.last_inbound_seq.lock();
        if seq <= *last {
            return Err(IpcRuntimeError::InboundSequenceNotMonotonic {
                previous: *last,
                current: seq,
            });
        }
        *last = seq;
        Ok(())
    }

    pub fn accept_hello(&self, hello: &ClientHello) -> Result<(), IpcRuntimeError> {
        if hello.supports_current_version() {
            Ok(())
        } else {
            Err(IpcRuntimeError::UnsupportedProtocolVersion {
                supported_versions: hello.supported_versions.clone(),
            })
        }
    }

    pub fn ready_payload(
        &self,
        model: impl Into<String>,
        cwd: impl Into<String>,
        permission_mode: impl Into<String>,
        available_models: Vec<String>,
        legacy_compatibility_mode: bool,
    ) -> IpcPayload {
        IpcPayload::Ready(ServerReady {
            accepted_version: IPC_V2_PROTOCOL_VERSION,
            session_id: self.session.session_id().to_string(),
            model: model.into(),
            cwd: cwd.into(),
            permission_mode: permission_mode.into(),
            available_models,
            capabilities: ServerCapabilities::default(),
            legacy_compatibility_mode,
        })
    }

    pub async fn send_backend(&self, message: BackendMessage) -> Result<(), IpcRuntimeError> {
        self.outbound.send(message).await
    }

    pub async fn request_permission(
        &self,
        request: PermissionRequestPayload,
    ) -> Result<PermissionResponsePayload, IpcRuntimeError> {
        let command = request.legacy_command();
        let request_id = request.tool_use_id.clone();
        let envelope = ServerRequestEnvelope {
            request_id: request_id.clone(),
            method: ServerRequestMethod::PermissionDecision,
            params: serde_json::json!({
                "tool_use_id": request.tool_use_id,
                "tool": request.tool_name,
                "command": command,
                "input": request.tool_input,
                "options": request.options,
                "operation": request.operation,
            }),
            timeout_ms: Some(Self::timeout_ms(self.server_request_timeout)),
        };
        let (sender, receiver) = oneshot::channel();
        self.pending_interactions()
            .insert_server_request(envelope.clone());
        self.pending_interactions()
            .insert_legacy_permission(envelope.request_id.clone(), sender);

        let legacy = payload_to_legacy_backend(&IpcPayload::ServerRequest(envelope))
            .map_err(IpcRuntimeError::Adapter)?;
        if let Err(error) = self.send_backend(legacy).await {
            self.pending_interactions().complete_permission(
                None,
                None,
                &request_id,
                PermissionResponsePayload::deny(),
            );
            return Err(error);
        }

        Ok(self
            .wait_for_permission_response(request_id, receiver)
            .await)
    }

    pub async fn request_question(
        &self,
        request: AskUserRequestPayload,
    ) -> Result<String, IpcRuntimeError> {
        let question_id = request.question.clone();
        let question_text = request.question;
        let choices = request.choices;
        let allow_free_text = request.allow_free_text;
        let envelope = ServerRequestEnvelope {
            request_id: question_id.clone(),
            method: ServerRequestMethod::AskUserQuestion,
            params: serde_json::json!({
                "id": question_id,
                "text": question_text,
                "choices": choices,
                "allow_free_text": allow_free_text,
            }),
            timeout_ms: Some(Self::timeout_ms(self.server_request_timeout)),
        };
        let (sender, receiver) = oneshot::channel();
        self.pending_interactions()
            .insert_server_request(envelope.clone());
        self.pending_interactions()
            .insert_legacy_question(envelope.request_id.clone(), sender);

        let legacy = payload_to_legacy_backend(&IpcPayload::ServerRequest(envelope))
            .map_err(IpcRuntimeError::Adapter)?;
        if let Err(error) = self.send_backend(legacy).await {
            self.pending_interactions()
                .complete_question(None, None, &question_id, String::new());
            return Err(error);
        }

        Ok(self.wait_for_question_response(question_id, receiver).await)
    }

    fn timeout_ms(timeout: Duration) -> u64 {
        timeout.as_millis().try_into().unwrap_or(u64::MAX)
    }

    async fn wait_for_permission_response(
        &self,
        request_id: String,
        receiver: oneshot::Receiver<PermissionResponsePayload>,
    ) -> PermissionResponsePayload {
        match tokio::time::timeout(self.server_request_timeout, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) | Err(_) => {
                self.pending_interactions().complete_permission(
                    None,
                    None,
                    &request_id,
                    PermissionResponsePayload::deny(),
                );
                PermissionResponsePayload::deny()
            }
        }
    }

    async fn wait_for_question_response(
        &self,
        request_id: String,
        receiver: oneshot::Receiver<String>,
    ) -> String {
        match tokio::time::timeout(self.server_request_timeout, receiver).await {
            Ok(Ok(text)) => text,
            Ok(Err(_)) | Err(_) => {
                self.pending_interactions().complete_question(
                    None,
                    None,
                    &request_id,
                    String::new(),
                );
                String::new()
            }
        }
    }

    pub fn resolve_legacy_client_response(&self, message: &FrontendMessage) -> bool {
        let Ok(IpcPayload::ClientResponse(response)) = legacy_frontend_to_payload(message) else {
            return false;
        };

        match message {
            FrontendMessage::PermissionResponse {
                tool_use_id,
                decision,
                feedback,
                session_id,
                turn_id,
            } => self.pending_interactions().complete_permission(
                session_id.as_deref(),
                turn_id.as_deref(),
                tool_use_id,
                PermissionResponsePayload::new(decision.clone(), feedback.clone()),
            ),
            FrontendMessage::QuestionResponse {
                id,
                text,
                session_id,
                turn_id,
            } => self.pending_interactions().complete_question(
                session_id.as_deref(),
                turn_id.as_deref(),
                id,
                text.clone(),
            ),
            _ => self.resolve_client_response(response),
        }
    }

    pub fn resolve_client_response(&self, response: ClientResponseEnvelope) -> bool {
        let request = self
            .pending_interactions()
            .pending_server_request(&response.request_id);
        match request.map(|request| request.method) {
            Some(ServerRequestMethod::PermissionDecision) => {
                let decision = response
                    .result
                    .get("decision")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("deny")
                    .to_string();
                let feedback = response
                    .result
                    .get("feedback")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                self.pending_interactions().complete_permission(
                    response
                        .result
                        .get("session_id")
                        .and_then(serde_json::Value::as_str),
                    response
                        .result
                        .get("turn_id")
                        .and_then(serde_json::Value::as_str),
                    &response.request_id,
                    PermissionResponsePayload::new(decision, feedback),
                )
            }
            Some(ServerRequestMethod::AskUserQuestion) => {
                let text = response
                    .result
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.pending_interactions().complete_question(
                    response
                        .result
                        .get("session_id")
                        .and_then(serde_json::Value::as_str),
                    response
                        .result
                        .get("turn_id")
                        .and_then(serde_json::Value::as_str),
                    &response.request_id,
                    text,
                )
            }
            None => false,
        }
    }

    pub fn cleanup_pending(&self) -> PendingCleanup {
        self.pending_interactions().cleanup()
    }

    pub async fn dispatch_client_request<F, Fut>(
        &self,
        request: ClientRequestEnvelope,
        dispatch: F,
    ) -> Result<ClientResponseEnvelope, ServerErrorEnvelope>
    where
        F: FnOnce(allthecodes_protocol::ClientRequest) -> Fut,
        Fut: Future<Output = Result<ClientResponse, ApiError>>,
    {
        let response = dispatch(request.request).await.map_err(|error| {
            let body = error.into_body();
            ServerErrorEnvelope {
                request_id: Some(request.request_id.clone()),
                ..ServerErrorEnvelope::from(body)
            }
        })?;
        let result = serde_json::to_value(response).map_err(|error| ServerErrorEnvelope {
            request_id: Some(request.request_id.clone()),
            code: -32603,
            message: "failed to serialize client response".to_string(),
            data: Some(serde_json::json!({
                "error": error.to_string(),
            })),
        })?;

        Ok(ClientResponseEnvelope {
            request_id: request.request_id,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_ipc_protocol::{ClientCapabilities, ClientRequestEnvelope};
    use allthecodes_protocol::{ClientRequest, NoParams};

    fn info(text: &str) -> BackendMessage {
        BackendMessage::SystemInfo {
            text: text.to_string(),
            level: "info".to_string(),
        }
    }

    fn progress(id: &str) -> BackendMessage {
        BackendMessage::ToolProgress {
            tool_use_id: id.to_string(),
            tool: "bash".to_string(),
            output: String::new(),
            elapsed_seconds: 1,
            total_lines: None,
            total_bytes: None,
            timeout_ms: None,
            operation: None,
        }
    }

    #[test]
    fn scoped_permission_matches_session_and_turn() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending.insert_scoped_permission("session-1", "turn-1", "tool-1", tx);

        assert!(!pending.complete_permission(
            Some("session-1"),
            Some("wrong-turn"),
            "tool-1",
            PermissionResponsePayload::decision("deny"),
        ));
        assert!(pending.complete_permission(
            Some("session-1"),
            Some("turn-1"),
            "tool-1",
            PermissionResponsePayload::decision("allow"),
        ));
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PermissionResponsePayload::decision("allow")
        );
    }

    #[test]
    fn legacy_permission_fallback_still_resolves() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending
            .legacy_permissions()
            .lock()
            .insert("tool-1".to_string(), tx);

        assert!(pending.complete_permission(
            None,
            None,
            "tool-1",
            PermissionResponsePayload::decision("allow")
        ));
        assert_eq!(
            rx.blocking_recv().unwrap(),
            PermissionResponsePayload::decision("allow")
        );
    }

    #[test]
    fn scoped_question_matches_session_and_turn() {
        let pending = PendingInteractions::new();
        let (tx, rx) = oneshot::channel();
        pending.insert_scoped_question("session-1", "turn-1", "question-1", tx);

        assert!(pending.complete_question(
            Some("session-1"),
            Some("turn-1"),
            "question-1",
            "yes".to_string(),
        ));
        assert_eq!(rx.blocking_recv().unwrap(), "yes");
    }

    #[test]
    fn session_runtime_tracks_current_turn() {
        let runtime = SessionRuntime::new("session-1");
        let turn = runtime.begin_turn("turn-1");

        assert_eq!(turn.session_id, "session-1");
        assert_eq!(runtime.current_turn().unwrap().turn_id, "turn-1");
        assert_eq!(runtime.session_id(), "session-1");
        assert!(!runtime.run_id().is_empty());
    }

    #[test]
    fn cleanup_resolves_pending_interactions_with_defaults() {
        let pending = PendingInteractions::new();
        let (permission_tx, permission_rx) = oneshot::channel();
        let (question_tx, question_rx) = oneshot::channel();
        pending.insert_server_request(ServerRequestEnvelope {
            request_id: "tool-1".to_string(),
            method: ServerRequestMethod::PermissionDecision,
            params: serde_json::json!({ "tool_use_id": "tool-1" }),
            timeout_ms: None,
        });
        pending.insert_legacy_permission("tool-1".to_string(), permission_tx);
        pending.insert_legacy_question("question-1".to_string(), question_tx);

        let cleanup = pending.cleanup();

        assert_eq!(
            cleanup,
            PendingCleanup {
                permissions: 1,
                questions: 1,
                server_requests: 1,
            }
        );
        assert_eq!(
            permission_rx.blocking_recv().unwrap(),
            PermissionResponsePayload::deny()
        );
        assert_eq!(question_rx.blocking_recv().unwrap(), "");
    }

    #[tokio::test]
    async fn runtime_sends_backend_messages_through_outbound_queue() {
        let (runtime, mut receiver) = IpcRuntime::new("session-1", 8);

        runtime.send_backend(info("ready")).await.unwrap();

        assert!(matches!(
            receiver.recv().await,
            Some(BackendMessage::SystemInfo { text, .. }) if text == "ready"
        ));
    }

    #[tokio::test]
    async fn outbound_queue_drops_best_effort_and_marks_next_lossless_lagged() {
        let (runtime, mut receiver) = IpcRuntime::new("session-1", 1);

        runtime.send_backend(progress("old")).await.unwrap();
        runtime.send_backend(progress("dropped")).await.unwrap();

        let runtime_for_lossless = runtime.clone();
        let send_lossless = tokio::spawn(async move {
            runtime_for_lossless
                .send_backend(info("recovered"))
                .await
                .unwrap();
        });

        assert!(matches!(
            receiver.recv().await,
            Some(BackendMessage::ToolProgress { tool_use_id, .. }) if tool_use_id == "old"
        ));

        let lagged = receiver.recv().await.unwrap();
        assert!(matches!(
            &lagged,
            BackendMessage::Error { message, recoverable: true }
                if message.contains("ipc client lagged: skipped=1")
                    && message.contains("last_dropped_type=tool_progress")
        ));

        assert!(matches!(
            receiver.recv().await,
            Some(BackendMessage::SystemInfo { text, .. }) if text == "recovered"
        ));
        send_lossless.await.unwrap();
    }

    #[tokio::test]
    async fn runtime_permission_request_uses_server_request_and_legacy_output() {
        let (runtime, mut receiver) = IpcRuntime::new("session-1", 8);
        let request = PermissionRequestPayload {
            tool_use_id: "tool-1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": "ls" }),
            message: "ls".to_string(),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
        };
        let runtime_for_response = runtime.clone();

        let task = tokio::spawn(async move { runtime.request_permission(request).await.unwrap() });
        let outbound = receiver.recv().await.unwrap();

        assert!(matches!(
            outbound,
            BackendMessage::PermissionRequest {
                tool_use_id,
                tool,
                command,
                options,
                ..
            } if tool_use_id == "tool-1"
                && tool == "Bash"
                && command == "Bash: ls"
                && options == vec!["allow", "deny"]
        ));
        assert!(matches!(
            runtime_for_response
                .pending_interactions()
                .pending_server_request("tool-1"),
            Some(ServerRequestEnvelope {
                method: ServerRequestMethod::PermissionDecision,
                ..
            })
        ));

        assert!(runtime_for_response.resolve_legacy_client_response(
            &FrontendMessage::PermissionResponse {
                tool_use_id: "tool-1".to_string(),
                decision: "allow".to_string(),
                feedback: Some("ok".to_string()),
                session_id: None,
                turn_id: None,
            },
        ));

        assert_eq!(
            task.await.unwrap(),
            PermissionResponsePayload::new("allow", Some("ok".to_string()))
        );
        assert!(runtime_for_response
            .pending_interactions()
            .pending_server_request("tool-1")
            .is_none());
    }

    #[tokio::test]
    async fn permission_request_times_out_with_deny_and_cleans_pending() {
        let (runtime, mut receiver) =
            IpcRuntime::with_server_request_timeout("session-1", 8, Duration::from_millis(1));
        let request = PermissionRequestPayload {
            tool_use_id: "tool-1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": "ls" }),
            message: "ls".to_string(),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
        };
        let runtime_for_task = runtime.clone();

        let task =
            tokio::spawn(
                async move { runtime_for_task.request_permission(request).await.unwrap() },
            );
        let outbound = receiver.recv().await.unwrap();
        assert!(matches!(
            outbound,
            BackendMessage::PermissionRequest { tool_use_id, .. } if tool_use_id == "tool-1"
        ));

        assert_eq!(task.await.unwrap(), PermissionResponsePayload::deny());
        assert!(runtime
            .pending_interactions()
            .pending_server_request("tool-1")
            .is_none());
    }

    #[tokio::test]
    async fn runtime_question_request_uses_server_request_and_legacy_output() {
        let (runtime, mut receiver) = IpcRuntime::new("session-1", 8);
        let request = AskUserRequestPayload {
            question: "Continue?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            allow_free_text: false,
        };
        let runtime_for_response = runtime.clone();

        let task = tokio::spawn(async move { runtime.request_question(request).await.unwrap() });
        let outbound = receiver.recv().await.unwrap();

        assert!(matches!(
            outbound,
            BackendMessage::QuestionRequest {
                id,
                text,
                choices,
                allow_free_text: false,
            } if id == "Continue?" && text == "Continue?" && choices == vec!["yes", "no"]
        ));
        assert!(matches!(
            runtime_for_response
                .pending_interactions()
                .pending_server_request("Continue?"),
            Some(ServerRequestEnvelope {
                method: ServerRequestMethod::AskUserQuestion,
                ..
            })
        ));

        assert!(runtime_for_response.resolve_legacy_client_response(
            &FrontendMessage::QuestionResponse {
                id: "Continue?".to_string(),
                text: "yes".to_string(),
                session_id: None,
                turn_id: None,
            },
        ));

        assert_eq!(task.await.unwrap(), "yes");
        assert!(runtime_for_response
            .pending_interactions()
            .pending_server_request("Continue?")
            .is_none());
    }

    #[tokio::test]
    async fn question_request_times_out_with_empty_answer_and_cleans_pending() {
        let (runtime, mut receiver) =
            IpcRuntime::with_server_request_timeout("session-1", 8, Duration::from_millis(1));
        let request = AskUserRequestPayload {
            question: "Continue?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            allow_free_text: false,
        };
        let runtime_for_task = runtime.clone();

        let task =
            tokio::spawn(async move { runtime_for_task.request_question(request).await.unwrap() });
        let outbound = receiver.recv().await.unwrap();
        assert!(matches!(
            outbound,
            BackendMessage::QuestionRequest { id, .. } if id == "Continue?"
        ));

        assert_eq!(task.await.unwrap(), "");
        assert!(runtime
            .pending_interactions()
            .pending_server_request("Continue?")
            .is_none());
    }

    #[test]
    fn runtime_allocates_monotonic_outbound_seq_and_validates_inbound_seq() {
        let (runtime, _receiver) = IpcRuntime::new("session-1", 8);

        assert_eq!(runtime.next_outbound_seq(), 1);
        assert_eq!(runtime.next_outbound_seq(), 2);
        assert!(runtime.verify_inbound_seq(1).is_ok());
        assert_eq!(
            runtime.verify_inbound_seq(1),
            Err(IpcRuntimeError::InboundSequenceNotMonotonic {
                previous: 1,
                current: 1,
            })
        );
        assert!(runtime.verify_inbound_seq(2).is_ok());
    }

    #[test]
    fn runtime_accepts_only_supported_ipc_v2_hello() {
        let (runtime, _receiver) = IpcRuntime::new("session-1", 8);
        let hello = ClientHello {
            client_name: "web".to_string(),
            client_version: "0.1.0".to_string(),
            supported_versions: vec![IPC_V2_PROTOCOL_VERSION],
            capabilities: ClientCapabilities {
                server_requests: true,
                lagged_events: true,
                typed_client_requests: true,
            },
            notification_subscriptions: Vec::new(),
        };

        assert!(runtime.accept_hello(&hello).is_ok());

        let unsupported = ClientHello {
            supported_versions: vec![1],
            ..hello
        };
        assert_eq!(
            runtime.accept_hello(&unsupported),
            Err(IpcRuntimeError::UnsupportedProtocolVersion {
                supported_versions: vec![1],
            })
        );
    }

    #[test]
    fn runtime_builds_ready_payload_from_session_context() {
        let (runtime, _receiver) = IpcRuntime::new("session-1", 8);

        let payload = runtime.ready_payload(
            "test-model",
            "/repo",
            "default",
            vec!["test-model".to_string()],
            true,
        );

        assert!(matches!(
            payload,
            IpcPayload::Ready(ServerReady {
                accepted_version: IPC_V2_PROTOCOL_VERSION,
                session_id,
                model,
                legacy_compatibility_mode: true,
                ..
            }) if session_id == "session-1" && model == "test-model"
        ));
    }

    #[tokio::test]
    async fn runtime_dispatches_client_request_with_supplied_dispatcher() {
        let (runtime, _receiver) = IpcRuntime::new("session-1", 8);
        let request = ClientRequestEnvelope {
            request_id: "req-1".to_string(),
            request: ClientRequest::Capabilities(NoParams {}),
        };

        let response = runtime
            .dispatch_client_request(request, |request| async move {
                assert!(matches!(request, ClientRequest::Capabilities(NoParams {})));
                Ok(ClientResponse::Capabilities(
                    allthecodes_protocol::v1::capabilities::CapabilityDiscoveryResponse {
                        capabilities: std::collections::HashMap::new(),
                    },
                ))
            })
            .await
            .unwrap();

        assert_eq!(response.request_id, "req-1");
        assert_eq!(response.result["method"], "Capabilities");
    }
}
