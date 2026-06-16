//! Shared daemon state passed to all axum HTTP handlers.
//!
//! Wraps the [`QueryEngine`] and provides SSE client management, event
//! buffering for re-attach, and notification dispatch.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tokio::sync::mpsc;

use allthecodes_config::features::FeatureFlags;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_server::{
    ConnectionId, EventLog, EventSeq, OutboundRouter, ReplayBatch, ReplayStatus, SequencedEvent,
    DEFAULT_EVENT_LOG_CAPACITY, DEFAULT_WRITER_CHANNEL_CAPACITY,
};

/// A single Server-Sent Event destined for connected frontends.
#[derive(Debug, Clone, Serialize)]
pub struct SseEvent {
    pub id: String,
    pub event_type: String,
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// SseClient
// ---------------------------------------------------------------------------

/// A connected SSE frontend client.
pub struct SseClient {
    pub client_id: String,
    pub connection_id: ConnectionId,
    pub connected_at: std::time::Instant,
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// A notification to be delivered to the user (push, toast, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub level: String,
    pub source: serde_json::Value,
}

// ---------------------------------------------------------------------------
// DaemonState
// ---------------------------------------------------------------------------

/// Shared state for the KAIROS daemon, passed to all axum handlers.
///
/// All fields are `Arc`-wrapped or `Copy`, so the struct is cheaply cloneable.
#[derive(Clone)]
pub struct DaemonState {
    pub engine: Arc<QueryEngine>,
    pub features: Arc<FeatureFlags>,
    pub account_auth: Arc<Mutex<crate::account_auth::AccountAuthMemory>>,
    pub clients: Arc<RwLock<HashMap<String, SseClient>>>,
    pub is_query_running: Arc<AtomicBool>,
    pub notification_tx: mpsc::UnboundedSender<Notification>,
    pub notification_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<Notification>>>>,
    pub event_log: EventLog<SseEvent>,
    pub event_router: OutboundRouter<SseEvent>,
    lagged_disconnects: Arc<Mutex<HashMap<ConnectionId, u64>>>,
    pub port: u16,
    // Team memory proxy (populated when Feature::TeamMemory is enabled)
    pub team_memory_port: Option<u16>,
    pub team_memory_secret: Option<String>,
}

impl DaemonState {
    /// Create a new `DaemonState` with channels and empty buffers.
    pub fn new(engine: Arc<QueryEngine>, features: Arc<FeatureFlags>, port: u16) -> Self {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        Self {
            engine,
            features,
            account_auth: Arc::new(Mutex::new(crate::account_auth::AccountAuthMemory::default())),
            clients: Arc::new(RwLock::new(HashMap::new())),
            is_query_running: Arc::new(AtomicBool::new(false)),
            notification_tx,
            notification_rx: Arc::new(Mutex::new(Some(notification_rx))),
            event_log: EventLog::new(DEFAULT_EVENT_LOG_CAPACITY),
            event_router: OutboundRouter::new(DEFAULT_WRITER_CHANNEL_CAPACITY),
            lagged_disconnects: Arc::new(Mutex::new(HashMap::new())),
            port,
            team_memory_port: None,
            team_memory_secret: None,
        }
    }

    /// Broadcast an event to all connected SSE clients and buffer it for
    /// re-attach.
    ///
    /// The event is assigned a monotonic ID before dispatch. Disconnected
    /// clients (whose channel has been dropped) are silently skipped.
    pub fn broadcast(&self, event: SseEvent) {
        let event = self.event_log.append_with(|seq| SseEvent {
            id: seq.to_string(),
            event_type: event.event_type,
            data: event.data,
        });
        let report = self.event_router.broadcast(event);
        for connection_id in &report.full_connections {
            *self
                .lagged_disconnects
                .lock()
                .entry(connection_id.clone())
                .or_insert(0) += 1;
        }
        if report.full > 0 || report.closed > 0 {
            tracing::warn!(
                delivered = report.delivered,
                full = report.full,
                closed = report.closed,
                "daemon SSE fanout skipped lagging connections"
            );
        }
    }

    /// Return all buffered events whose numeric ID is strictly greater than
    /// `last_id`. Used by frontends re-attaching after a disconnect.
    pub fn events_since(&self, last_id: &str) -> Vec<SseEvent> {
        self.replay_after(last_id.parse().ok())
            .events
            .into_iter()
            .map(|event| event.message)
            .collect()
    }

    pub fn replay_after(&self, after_seq: Option<EventSeq>) -> ReplayBatch<SseEvent> {
        self.event_log.replay_after(after_seq)
    }

    pub fn register_sse_client(
        &self,
        client_id: String,
        connection_id: ConnectionId,
    ) -> tokio::sync::mpsc::Receiver<SequencedEvent<SseEvent>> {
        let receiver = self.event_router.register(connection_id.clone());
        self.clients.write().insert(
            client_id.clone(),
            SseClient {
                client_id,
                connection_id,
                connected_at: std::time::Instant::now(),
            },
        );
        receiver
    }

    pub fn unregister_sse_client(&self, client_id: &str, connection_id: &ConnectionId) {
        self.event_router.unregister(connection_id);
        self.clients.write().remove(client_id);
        self.lagged_disconnects.lock().remove(connection_id);
    }

    pub fn latest_seq(&self) -> EventSeq {
        self.event_log.latest_seq()
    }

    pub fn lagged_event(&self, status: &ReplayStatus) -> Option<SseEvent> {
        lagged_sse_event(status, self.latest_seq())
    }

    pub fn live_lagged_event(&self, skipped: u64) -> SseEvent {
        SseEvent {
            id: String::new(),
            event_type: "lagged".to_string(),
            data: serde_json::json!({
                "skipped": skipped,
                "latest_seq": self.latest_seq(),
            }),
        }
    }

    pub fn take_disconnect_lag(&self, connection_id: &ConnectionId) -> Option<u64> {
        self.lagged_disconnects.lock().remove(connection_id)
    }

    /// Whether any frontend SSE client is currently connected.
    pub fn has_clients(&self) -> bool {
        !self.clients.read().is_empty()
    }

    /// Returns `true` when the user is actively looking at the terminal.
    ///
    /// Currently equivalent to [`has_clients`](Self::has_clients) -- if any
    /// frontend is connected, we assume the user is focused.
    pub fn terminal_focus(&self) -> bool {
        self.has_clients()
    }
}

fn lagged_sse_event(status: &ReplayStatus, latest_seq: EventSeq) -> Option<SseEvent> {
    let ReplayStatus::Compacted {
        requested_after_seq,
        oldest_seq,
    } = status
    else {
        return None;
    };
    Some(SseEvent {
        id: String::new(),
        event_type: "lagged".to_string(),
        data: serde_json::json!({
            "requested_after_seq": requested_after_seq,
            "oldest_seq": oldest_seq,
            "latest_seq": latest_seq,
            "skipped": oldest_seq.saturating_sub(*requested_after_seq + 1),
        }),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_assigns_sse_ids_and_caps() {
        let log = EventLog::new(2);

        let first = log.append_with(|seq| SseEvent {
            id: seq.to_string(),
            event_type: "test".into(),
            data: serde_json::json!({}),
        });
        log.append_with(|seq| SseEvent {
            id: seq.to_string(),
            event_type: "test".into(),
            data: serde_json::json!({}),
        });
        let third = log.append_with(|seq| SseEvent {
            id: seq.to_string(),
            event_type: "test".into(),
            data: serde_json::json!({}),
        });

        assert_eq!(first.seq, 1);
        assert_eq!(third.seq, 3);
        assert_eq!(log.oldest_seq(), Some(2));
        let replay = log.replay_after(Some(0));
        assert!(matches!(replay.status, ReplayStatus::Compacted { .. }));
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].message.id, "2");
    }

    #[test]
    fn event_log_replay_filters_by_numeric_seq() {
        let log = EventLog::new(10);
        for i in 0..5u64 {
            log.append_with(|seq| SseEvent {
                id: seq.to_string(),
                event_type: "test".into(),
                data: serde_json::json!({"n": i}),
            });
        }

        let result: Vec<SseEvent> = log
            .replay_after(Some(3))
            .events
            .into_iter()
            .map(|event| event.message)
            .collect();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "4");
        assert_eq!(result[1].id, "5");
    }

    #[test]
    fn lagged_sse_event_does_not_advance_last_event_id() {
        let event = lagged_sse_event(
            &ReplayStatus::Compacted {
                requested_after_seq: 1,
                oldest_seq: 5,
            },
            10,
        )
        .expect("compacted replay should produce lag event");

        assert!(event.id.is_empty());
        assert_eq!(event.event_type, "lagged");
        assert_eq!(event.data["latest_seq"], serde_json::json!(10));
        assert_eq!(event.data["skipped"], serde_json::json!(3));
    }
}
