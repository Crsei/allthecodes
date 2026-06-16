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

pub struct IpcSessionHub {
    session_id: String,
    runtime: IpcRuntime,
    bridge_rx: Mutex<Option<mpsc::Receiver<BackendMessage>>>,
    event_log: EventLog<BackendMessage>,
    router: OutboundRouter<BackendMessage>,
    active_owner: Mutex<Option<ConnectionId>>,
}

impl IpcSessionHub {
    pub fn new(session_id: impl Into<String>) -> Arc<Self> {
        let session_id = session_id.into();
        let (runtime, bridge_rx) = IpcRuntime::new(session_id.clone(), 256);
        Arc::new(Self {
            session_id,
            runtime,
            bridge_rx: Mutex::new(Some(bridge_rx)),
            event_log: EventLog::new(DEFAULT_EVENT_LOG_CAPACITY),
            router: OutboundRouter::new(DEFAULT_WRITER_CHANNEL_CAPACITY),
            active_owner: Mutex::new(None),
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
    }

    pub fn take_bridge_receiver(&self) -> Option<mpsc::Receiver<BackendMessage>> {
        self.bridge_rx.lock().take()
    }

    pub fn publish(&self, message: BackendMessage) -> SequencedEvent<BackendMessage> {
        let event = self.event_log.append(message);
        let report = self.router.broadcast(event.clone());
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

    fn info(text: &str) -> BackendMessage {
        BackendMessage::SystemInfo {
            text: text.to_string(),
            level: "info".to_string(),
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
}
