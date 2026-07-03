//! Test support for ACP runtime integration tests.
//!
//! Provides a `RuntimeHarness` that drives the ACP runtime dispatch path
//! via in-memory channels instead of real stdin/stdout.

use std::sync::Arc;

use agent_client_protocol_schema::rpc::RequestId;
use allthecodes_engine::types::app_state::AppState;
use tokio::sync::mpsc;

use allthecodes_acp::engine_factory::AcpEngineFactory;
use allthecodes_acp::permissions::AcpPermissionManager;
use allthecodes_acp::runtime::RuntimeContext;
use allthecodes_acp::runtime::{dispatch_request, send_dispatch_outcome};
use allthecodes_acp::session::AcpSessionManager;
use allthecodes_acp::transport::AcpSink;
use allthecodes_acp::AcpCliOverrides;

/// In-memory test harness for the ACP runtime dispatch.
///
/// Wraps the dispatch infrastructure so tests can send JSON-RPC requests
/// and observe responses/notifications without real stdio or engines.
pub struct RuntimeHarness {
    pub sink: AcpSink,
    /// Channel that receives all outgoing messages (both responses and
    /// notifications) in the order they were sent.
    pub outgoing_rx: mpsc::UnboundedReceiver<serde_json::Value>,
    pub ctx: RuntimeContext,
    pub session_manager: Arc<AcpSessionManager>,
    pub permission_manager: Arc<AcpPermissionManager>,
    pub capabilities: allthecodes_acp::runtime::AcpCapabilities,
}

impl RuntimeHarness {
    /// Create a new test harness with a given engine factory.
    pub fn new_with_factory(
        cwd: std::path::PathBuf,
        engine_factory: Arc<dyn AcpEngineFactory>,
    ) -> Self {
        let (sink_tx, sink_rx) = mpsc::unbounded_channel();
        let sink = AcpSink::new(sink_tx);

        let merged_config = allthecodes_config::settings::EffectiveSettings::default();

        // Create a minimal RuntimeContext
        let ctx = RuntimeContext {
            model: "test-model".to_string(),
            cwd,
            tools: vec![],
            app_state_template: AppState::default(),
            merged_config,
            cli_overrides: AcpCliOverrides::default(),
            engine_factory: engine_factory.clone(),
        };

        let session_manager = Arc::new(AcpSessionManager::new(engine_factory));
        let permission_manager = Arc::new(AcpPermissionManager::new());
        let capabilities = allthecodes_acp::runtime::AcpCapabilities::baseline();

        Self {
            sink,
            outgoing_rx: sink_rx,
            ctx,
            session_manager,
            permission_manager,
            capabilities,
        }
    }

    /// Send a JSON-RPC request via the in-memory dispatch and return the
    /// JSON-RPC response.  Also captures any pre-response messages (notifications)
    /// that were sent before the dispatch returned.
    #[allow(dead_code)]
    pub async fn send_request_and_capture(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
        let id = RequestId::Number(1);
        let (_cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let raw_params = params.as_ref().and_then(|p| {
            serde_json::value::RawValue::from_string(serde_json::to_string(p).ok()?).ok()
        });

        // Dispatch the request.  Any notifications the handler sends via
        // sink.send() go into the channel during this call.
        let result = dispatch_request(
            method,
            raw_params.as_deref(),
            &id,
            &self.capabilities,
            &self.session_manager,
            &self.permission_manager,
            &self.sink,
            &self.ctx,
        )
        .await;

        // Drain the channel for any messages that were sent BEFORE the
        // dispatch returned.  These are pre-response messages.
        let pre_response = self.drain_all();

        if let Some(result) = result {
            send_dispatch_outcome(&id, result, &self.sink, &mut cancel_rx).await;
        }
        let response = self.drain_all().into_iter().find(is_response);
        (response, pre_response)
    }

    /// Dispatch a request, intentionally yield before writing its response, and
    /// return the actual outgoing order that a runtime response writer would see.
    #[allow(dead_code)]
    pub async fn send_request_with_response_gap(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Vec<serde_json::Value> {
        let id = RequestId::Number(2);
        let (_cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let raw_params = params.as_ref().and_then(|p| {
            serde_json::value::RawValue::from_string(serde_json::to_string(p).ok()?).ok()
        });

        let result = dispatch_request(
            method,
            raw_params.as_deref(),
            &id,
            &self.capabilities,
            &self.session_manager,
            &self.permission_manager,
            &self.sink,
            &self.ctx,
        )
        .await;

        tokio::task::yield_now().await;
        let mut messages = self.drain_all();
        if let Some(result) = result {
            send_dispatch_outcome(&id, result, &self.sink, &mut cancel_rx).await;
        }
        tokio::task::yield_now().await;
        messages.extend(self.drain_all());
        messages
    }

    /// Dispatch a request, cancel it before its response is written, and return
    /// the actual outgoing order.
    #[allow(dead_code)]
    pub async fn send_request_cancelled_before_response(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Vec<serde_json::Value> {
        let id = RequestId::Number(3);
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        let raw_params = params.as_ref().and_then(|p| {
            serde_json::value::RawValue::from_string(serde_json::to_string(p).ok()?).ok()
        });

        let result = dispatch_request(
            method,
            raw_params.as_deref(),
            &id,
            &self.capabilities,
            &self.session_manager,
            &self.permission_manager,
            &self.sink,
            &self.ctx,
        )
        .await;

        let _ = cancel_tx.send(());
        tokio::task::yield_now().await;
        let mut messages = self.drain_all();
        if let Some(result) = result {
            send_dispatch_outcome(&id, result, &self.sink, &mut cancel_rx).await;
        }
        tokio::task::yield_now().await;
        messages.extend(self.drain_all());
        messages
    }

    /// Drain all messages currently in the outgoing channel.
    pub fn drain_all(&mut self) -> Vec<serde_json::Value> {
        let mut msgs = Vec::new();
        while let Ok(value) = self.outgoing_rx.try_recv() {
            msgs.push(value);
        }
        msgs
    }
}

/// Check if a JSON value is a session/update notification.
#[allow(dead_code)]
pub fn is_session_update(value: &serde_json::Value) -> bool {
    value
        .get("method")
        .and_then(|v| v.as_str())
        .is_some_and(|m| m == "session/update" || m == "notifications/notification")
}

/// Check if a JSON value is a JSON-RPC response (has "id" and either "result" or "error").
#[allow(dead_code)]
pub fn is_response(value: &serde_json::Value) -> bool {
    value.get("id").is_some() && (value.get("result").is_some() || value.get("error").is_some())
}
