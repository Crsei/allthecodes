//! Generic headless JSONL runtime.
//!
//! This crate owns stdin/stdout multiplexing and protocol event routing. The
//! binary installs a [`HeadlessRuntimeHost`] for engine, command, plugin, and
//! team behavior so `cc-ipc` does not depend on those owner crates.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use allthecodes_ipc_protocol::{BackendMessage, FrontendMessage};
use allthecodes_server::{
    ConnectionClosedReason, ConnectionId, ConnectionOrigin, TransportEvent, TransportKind,
};
use tracing::{debug, error, warn};

pub use crate::runtime::{
    PendingInteractions, PendingPermissions, PendingQuestions, SessionRuntime,
};
use crate::transport::{
    parse_frontend_line, FrontendSink, IpcReader, IpcTransport, JsonlStdioTransport,
    ParsedFrontendLine,
};
pub type BoxHeadlessFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct BackgroundAgentCompletion {
    pub agent_id: String,
    pub description: String,
    pub result_preview: String,
    pub had_error: bool,
    pub duration_ms: u64,
}

pub trait HeadlessRuntimeHost: Send + Sync + 'static {
    fn install(
        &self,
        pending_permissions: PendingPermissions,
        pending_questions: PendingQuestions,
        sink: FrontendSink,
        agent_tx: allthecodes_types::agent_channel::AgentSender,
        subsystem_tx: tokio::sync::broadcast::Sender<
            allthecodes_ipc_protocol::subsystem_events::SubsystemEvent,
        >,
    );

    fn ready_message(&self, model: String) -> BackendMessage;

    fn dispatch_frontend<'a>(
        &'a self,
        msg: FrontendMessage,
        runtime: &'a SessionRuntime,
        sink: &'a FrontendSink,
    ) -> BoxHeadlessFuture<'a, bool>;

    fn background_agent_completed(
        &self,
        agent_id: &str,
        result_preview: &str,
        had_error: bool,
        duration_ms: u64,
    ) -> Option<BackgroundAgentCompletion>;

    fn shutdown_background_agents<'a>(&'a self, reason: &'a str) -> BoxHeadlessFuture<'a, usize>;
}

pub struct HeadlessRuntimeConfig {
    pub host: Arc<dyn HeadlessRuntimeHost>,
    pub model: String,
    pub sink: FrontendSink,
}

impl HeadlessRuntimeConfig {
    pub fn new(host: Arc<dyn HeadlessRuntimeHost>, model: String) -> Self {
        Self {
            host,
            model,
            sink: FrontendSink::stdout(),
        }
    }
}

pub async fn run_headless(config: HeadlessRuntimeConfig) -> anyhow::Result<()> {
    let connection_id = ConnectionId::next();
    log_transport_event(TransportEvent::<FrontendMessage>::ConnectionOpened {
        connection_id: connection_id.clone(),
        kind: TransportKind::HeadlessStdio,
        origin: ConnectionOrigin::local_native(),
    });

    let sink = config.sink.clone();
    let ready_message = config.host.ready_message(config.model);
    let runtime = SessionRuntime::from_ready_message(&ready_message);
    let pending_permissions = runtime.pending_interactions().legacy_permissions();
    let pending_questions = runtime.pending_interactions().legacy_questions();

    let (agent_tx, mut agent_rx) = allthecodes_types::agent_channel::agent_channel();
    let event_bus = crate::subsystem_events::SubsystemEventBus::new();
    let mut event_rx = event_bus.subscribe();

    config.host.install(
        pending_permissions.clone(),
        pending_questions.clone(),
        sink.clone(),
        agent_tx,
        event_bus.sender(),
    );

    sink.send(&ready_message)?;

    let (mut reader, _writer) = JsonlStdioTransport::new().split();

    let close_reason = loop {
        tokio::select! {
            frame = reader.read_frame() => {
                let line = match frame {
                    Ok(Some(frame)) => frame.line,
                    Ok(None) => {
                        debug!("headless: stdin closed, exiting");
                        break ConnectionClosedReason::StdinEof;
                    }
                    Err(e) => {
                        error!("headless: error reading stdin: {}", e);
                        break ConnectionClosedReason::TransportError(e.to_string());
                    }
                };

                let event = match parse_frontend_line(&line) {
                    ParsedFrontendLine::Message(msg) => TransportEvent::IncomingMessage {
                        connection_id: connection_id.clone(),
                        kind: TransportKind::HeadlessStdio,
                        message: msg,
                    },
                    ParsedFrontendLine::Diagnostic(diagnostic) => {
                        warn!(
                            "headless: failed to parse FrontendMessage - line: {}",
                            line
                        );
                        let _ = sink.send(&diagnostic);
                        continue;
                    }
                };

                let TransportEvent::IncomingMessage { message: msg, .. } = event else {
                    continue;
                };

                if !config
                    .host
                    .dispatch_frontend(msg, &runtime, &sink)
                    .await
                {
                    break ConnectionClosedReason::ProtocolQuit;
                }
            }

            Some(event) = agent_rx.recv() => {
                match event {
                    allthecodes_types::agent_channel::AgentIpcEvent::Agent(ref agent_event) => {
                        if let allthecodes_types::agent_events::AgentEvent::Completed {
                            ref agent_id,
                            ref result_preview,
                            had_error,
                            duration_ms,
                            ..
                        } = agent_event {
                            if let Some(done) = config.host.background_agent_completed(
                                agent_id,
                                result_preview,
                                *had_error,
                                *duration_ms,
                            ) {
                                let _ = sink.send(&BackendMessage::BackgroundAgentComplete {
                                    agent_id: done.agent_id,
                                    description: done.description,
                                    result_preview: done.result_preview,
                                    had_error: done.had_error,
                                    duration_ms: done.duration_ms,
                                });
                            }
                        }
                        let _ = sink.send(&BackendMessage::AgentEvent {
                            event: agent_event.clone(),
                        });
                    }
                    allthecodes_types::agent_channel::AgentIpcEvent::Team(team_event) => {
                        let _ = sink.send(&BackendMessage::TeamEvent {
                            event: team_event,
                        });
                    }
                }
            }

            Ok(event) = event_rx.recv() => {
                let msg = match event {
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Lsp(e) => {
                        BackendMessage::LspEvent { event: e }
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Mcp(e) => {
                        BackendMessage::McpEvent { event: e }
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Plugin(e) => {
                        BackendMessage::PluginEvent { event: e }
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Skill(e) => {
                        BackendMessage::SkillEvent { event: e }
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Ide(e) => {
                        BackendMessage::IdeEvent { event: e }
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::AgentSettings(e) => {
                        BackendMessage::AgentSettingsEvent { event: *e }
                    }
                    // Phase 2 integration: internal audit events, not forwarded
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::CompletionProvided { .. } => {
                        continue;
                    }
                    allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::TelemetryFlush => {
                        continue;
                    }
                };
                let _ = sink.send(&msg);
            }
        }
    };

    log_transport_event(TransportEvent::<FrontendMessage>::ConnectionClosed {
        connection_id,
        kind: TransportKind::HeadlessStdio,
        reason: close_reason,
    });

    let cancelled = config
        .host
        .shutdown_background_agents("headless runtime exit")
        .await;
    if cancelled > 0 {
        let _ = sink.send(&BackendMessage::SystemInfo {
            text: format!(
                "Cancelled {} background agent(s) during shutdown cleanup.",
                cancelled
            ),
            level: "info".to_string(),
        });
    }

    Ok(())
}

fn log_transport_event<T: std::fmt::Debug>(event: TransportEvent<T>) {
    match event {
        TransportEvent::ConnectionOpened {
            connection_id,
            kind,
            origin,
        } => debug!(
            connection_id = %connection_id,
            kind = ?kind,
            origin = ?origin,
            "transport connection opened"
        ),
        TransportEvent::IncomingMessage {
            connection_id,
            kind,
            message,
        } => debug!(
            connection_id = %connection_id,
            kind = ?kind,
            message = ?message,
            "transport incoming message"
        ),
        TransportEvent::ConnectionClosed {
            connection_id,
            kind,
            reason,
        } => debug!(
            connection_id = %connection_id,
            kind = ?kind,
            reason = ?reason,
            "transport connection closed"
        ),
    }
}
