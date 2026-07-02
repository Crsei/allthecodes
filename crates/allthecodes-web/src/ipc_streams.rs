use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::warn;

use allthecodes_ipc::runtime::IpcRuntime;
use allthecodes_ipc_protocol::{
    payload_to_legacy_backend, BackendMessage, IpcPayload, LaggedEvent,
};
use allthecodes_server::{
    ConnectionId, EventLog, EventSeq, OutboundRouter, ReplayBatch, ReplayStatus, RouterSendError,
    SequencedEvent, DEFAULT_EVENT_LOG_CAPACITY, DEFAULT_WRITER_CHANNEL_CAPACITY,
};
use allthecodes_types::agent_channel::{agent_channel, AgentIpcEvent, AgentReceiver, AgentSender};

pub struct IpcSessionHub {
    session_id: String,
    runtime: IpcRuntime,
    bridge_rx: Mutex<Option<mpsc::Receiver<BackendMessage>>>,
    agent_tx: AgentSender,
    agent_rx: Mutex<Option<AgentReceiver>>,
    event_log: EventLog<BackendMessage>,
    router: OutboundRouter<BackendMessage>,
    active_owner: Mutex<Option<ConnectionId>>,
    lagged_disconnects: Mutex<HashMap<ConnectionId, u64>>,
}

impl IpcSessionHub {
    pub fn new(session_id: impl Into<String>) -> Arc<Self> {
        let session_id = session_id.into();
        let (runtime, bridge_rx) = IpcRuntime::new(session_id.clone(), 256);
        let (agent_tx, agent_rx) = agent_channel();
        Arc::new(Self {
            session_id,
            runtime,
            bridge_rx: Mutex::new(Some(bridge_rx)),
            agent_tx,
            agent_rx: Mutex::new(Some(agent_rx)),
            event_log: EventLog::new(DEFAULT_EVENT_LOG_CAPACITY),
            router: OutboundRouter::new(DEFAULT_WRITER_CHANNEL_CAPACITY),
            active_owner: Mutex::new(None),
            lagged_disconnects: Mutex::new(HashMap::new()),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn runtime(&self) -> &IpcRuntime {
        &self.runtime
    }

    pub fn register_connection(
        &self,
        connection_id: ConnectionId,
    ) -> mpsc::Receiver<SequencedEvent<BackendMessage>> {
        self.router.register(connection_id)
    }

    pub fn unregister_connection(&self, connection_id: &ConnectionId) {
        self.router.unregister(connection_id);
        self.release_turn_if_owner(connection_id);
        self.lagged_disconnects.lock().remove(connection_id);
    }

    pub fn take_bridge_receiver(&self) -> Option<mpsc::Receiver<BackendMessage>> {
        self.bridge_rx.lock().take()
    }

    pub fn agent_sender(&self) -> AgentSender {
        self.agent_tx.clone()
    }

    pub fn take_agent_receiver(&self) -> Option<AgentReceiver> {
        self.agent_rx.lock().take()
    }

    pub fn publish(&self, message: BackendMessage) -> SequencedEvent<BackendMessage> {
        let event = self.event_log.append(message);
        let report = self.router.broadcast(event.clone());
        for connection_id in &report.full_connections {
            *self
                .lagged_disconnects
                .lock()
                .entry(connection_id.clone())
                .or_insert(0) += 1;
        }
        if report.full > 0 || report.closed > 0 {
            warn!(
                session_id = %self.session_id,
                delivered = report.delivered,
                full = report.full,
                closed = report.closed,
                "IPC outbound fanout skipped lagging connections"
            );
        }
        event
    }

    pub fn replay_after(&self, after_seq: Option<EventSeq>) -> ReplayBatch<BackendMessage> {
        self.event_log.replay_after(after_seq)
    }

    pub fn latest_seq(&self) -> EventSeq {
        self.event_log.latest_seq()
    }

    pub fn claim_turn(&self, connection_id: &ConnectionId) -> bool {
        let mut owner = self.active_owner.lock();
        if owner.is_some() {
            return false;
        }
        *owner = Some(connection_id.clone());
        true
    }

    pub fn is_turn_owner(&self, connection_id: &ConnectionId) -> bool {
        self.active_owner
            .lock()
            .as_ref()
            .map(|owner| owner == connection_id)
            .unwrap_or(false)
    }

    pub fn release_turn_if_owner(&self, connection_id: &ConnectionId) -> bool {
        let mut owner = self.active_owner.lock();
        if owner
            .as_ref()
            .map(|owner| owner == connection_id)
            .unwrap_or(false)
        {
            *owner = None;
            return true;
        }
        false
    }

    pub fn send_control_to(&self, connection_id: &ConnectionId, message: BackendMessage) {
        let event = SequencedEvent::new(self.latest_seq(), message);
        match self.router.send_to(connection_id, event) {
            Ok(()) => {}
            Err(RouterSendError::Full { .. }) | Err(RouterSendError::Closed { .. }) => {
                self.router.unregister(connection_id);
            }
            Err(RouterSendError::UnknownConnection { .. }) => {}
        }
    }

    pub fn take_disconnect_lag(&self, connection_id: &ConnectionId) -> Option<u64> {
        self.lagged_disconnects.lock().remove(connection_id)
    }

    pub fn lagged_message(skipped: u64, last_dropped_type: Option<String>) -> BackendMessage {
        payload_to_legacy_backend(&IpcPayload::Lagged(LaggedEvent {
            skipped,
            last_dropped_type,
        }))
        .unwrap_or_else(|error| BackendMessage::Error {
            message: format!("ipc client lagged: {error:?}"),
            recoverable: true,
        })
    }

    pub fn replay_lagged_message(
        status: &ReplayStatus,
        latest_seq: EventSeq,
    ) -> Option<BackendMessage> {
        let ReplayStatus::Compacted {
            requested_after_seq,
            oldest_seq,
        } = status
        else {
            return None;
        };
        let skipped = oldest_seq.saturating_sub(*requested_after_seq + 1);
        Some(BackendMessage::Error {
            message: format!(
                "ipc client lagged: requested_after_seq={} oldest_seq={} latest_seq={} skipped={}",
                requested_after_seq, oldest_seq, latest_seq, skipped
            ),
            recoverable: true,
        })
    }
}

pub(crate) async fn forward_agent_ipc_event(runtime: &IpcRuntime, event: AgentIpcEvent) -> bool {
    let message = match event {
        AgentIpcEvent::Agent(event) => BackendMessage::AgentEvent { event },
        AgentIpcEvent::Team(event) => BackendMessage::TeamEvent { event },
    };

    runtime.send_backend(message).await.is_ok()
}

pub fn ipc_seq_marker(session_id: &str, seq: EventSeq) -> BackendMessage {
    BackendMessage::StatusLineUpdate {
        payload: serde_json::json!({
            "transport": {
                "kind": "ipc_websocket",
                "session_id": session_id,
                "event_log_seq": seq,
            }
        }),
        lines: Vec::new(),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::agent_events::AgentEvent;
    use allthecodes_types::agent_runtime_record::{
        AgentRuntimeExecutionRecord, AgentRuntimePermissionDecision,
    };

    fn info(text: &str) -> BackendMessage {
        BackendMessage::SystemInfo {
            text: text.to_string(),
            level: "info".to_string(),
        }
    }

    fn execution_record_message() -> BackendMessage {
        BackendMessage::AgentEvent {
            event: AgentEvent::ExecutionRecord {
                agent_id: "agent-1".to_string(),
                record: Box::new(AgentRuntimeExecutionRecord {
                    session_id: "session-1".to_string(),
                    agent_id: "agent-1".to_string(),
                    tool: "shell".to_string(),
                    tool_use_id: Some("toolu-1".to_string()),
                    permission_decision: Some(AgentRuntimePermissionDecision::DeniedByPolicy),
                    exit_code: Some(2),
                    had_error: true,
                    ..Default::default()
                }),
            },
        }
    }

    #[tokio::test]
    async fn publish_logs_and_fans_out_to_registered_connections() {
        let hub = IpcSessionHub::new("session-1");
        let connection_id = ConnectionId::from_static("conn-1");
        let mut rx = hub.register_connection(connection_id);

        let event = hub.publish(info("hello"));

        assert_eq!(event.seq, 1);
        let received = rx.recv().await.unwrap().message;
        assert!(matches!(
            received,
            BackendMessage::SystemInfo { text, .. } if text == "hello"
        ));
        assert_eq!(hub.replay_after(Some(0)).events.len(), 1);
    }

    #[tokio::test]
    async fn agent_sender_forwards_execution_record_to_runtime_queue() {
        let hub = IpcSessionHub::new("session-1");
        let mut bridge_rx = hub.take_bridge_receiver().expect("bridge receiver");
        let mut agent_rx = hub.take_agent_receiver().expect("agent receiver");
        let runtime = hub.runtime().clone();
        let bridge_task = tokio::spawn(async move {
            if let Some(event) = agent_rx.recv().await {
                assert!(forward_agent_ipc_event(&runtime, event).await);
            }
        });

        hub.agent_sender()
            .send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                match execution_record_message() {
                    BackendMessage::AgentEvent { event } => event,
                    _ => unreachable!("execution_record_message returns agent event"),
                },
            ))
            .expect("send agent event");

        let outbound = bridge_rx.recv().await.expect("bridged backend message");
        let encoded = serde_json::to_value(&outbound).expect("serialize bridged message");
        assert_eq!(encoded["type"], "agent_event");
        assert_eq!(encoded["event"]["kind"], "execution_record");
        assert_eq!(encoded["event"]["record"]["session_id"], "session-1");
        assert_eq!(
            encoded["event"]["record"]["permission_decision"],
            "denied_by_policy"
        );

        bridge_task.await.expect("agent bridge task");
    }

    #[test]
    fn full_writer_channel_records_lagged_disconnect() {
        let hub = IpcSessionHub::new("session-1");
        let connection_id = ConnectionId::from_static("lagged");
        let _rx = hub.register_connection(connection_id.clone());

        for index in 0..=DEFAULT_WRITER_CHANNEL_CAPACITY {
            hub.publish(info(&format!("event-{index}")));
        }

        assert_eq!(hub.take_disconnect_lag(&connection_id), Some(1));
    }

    #[test]
    fn claim_turn_allows_single_owner() {
        let hub = IpcSessionHub::new("session-1");
        let first = ConnectionId::from_static("first");
        let second = ConnectionId::from_static("second");

        assert!(hub.claim_turn(&first));
        assert!(!hub.claim_turn(&second));
        assert!(hub.release_turn_if_owner(&first));
        assert!(hub.claim_turn(&second));
    }

    #[test]
    fn seq_marker_uses_existing_backend_message_shape() {
        let marker = ipc_seq_marker("session-1", 42);

        let BackendMessage::StatusLineUpdate {
            payload,
            lines,
            error,
        } = marker
        else {
            panic!("expected status line update marker");
        };
        assert!(lines.is_empty());
        assert!(error.is_none());
        assert_eq!(payload["transport"]["event_log_seq"], 42);
        assert_eq!(payload["transport"]["session_id"], "session-1");
    }

    #[test]
    fn execution_record_replays_as_legacy_agent_event() {
        let hub = IpcSessionHub::new("session-1");

        let event = hub.publish(execution_record_message());

        assert_eq!(event.seq, 1);
        let replay = hub.replay_after(Some(0));
        assert_eq!(replay.events.len(), 1);
        let encoded = serde_json::to_value(&replay.events[0].message)
            .expect("serialize replayed backend message");
        assert_eq!(encoded["type"], "agent_event");
        assert_eq!(encoded["event"]["kind"], "execution_record");
        assert_eq!(encoded["event"]["record"]["session_id"], "session-1");
        assert_eq!(encoded["event"]["record"]["tool"], "shell");
        assert_eq!(
            encoded["event"]["record"]["permission_decision"],
            "denied_by_policy"
        );
        assert_eq!(encoded["event"]["record"]["exit_code"], 2);
        assert_eq!(encoded["event"]["record"]["had_error"], true);
    }
}
