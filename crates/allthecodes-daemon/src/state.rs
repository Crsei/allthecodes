//! Shared daemon state passed to all axum HTTP handlers.
//!
//! Wraps the [`QueryEngine`] and provides SSE client management, event
//! buffering for re-attach, and notification dispatch.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tokio::sync::mpsc;

use allthecodes_config::features::FeatureFlags;
use allthecodes_engine::lifecycle::QueryEngine;

// ---------------------------------------------------------------------------
// Monotonic event ID counter
// ---------------------------------------------------------------------------

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns a monotonically increasing event ID string.
pub(super) fn next_event_id() -> String {
    EVENT_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

// ---------------------------------------------------------------------------
// SseEvent
// ---------------------------------------------------------------------------

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
    pub tx: mpsc::UnboundedSender<SseEvent>,
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

/// Maximum number of events retained in the ring buffer for re-attach.
const MAX_EVENT_BUFFER: usize = 1000;

/// Shared state for the KAIROS daemon, passed to all axum handlers.
///
/// All fields are `Arc`-wrapped or `Copy`, so the struct is cheaply cloneable.
#[derive(Clone)]
pub struct DaemonState {
    pub engine: Arc<QueryEngine>,
    pub features: Arc<FeatureFlags>,
    pub clients: Arc<RwLock<HashMap<String, SseClient>>>,
    pub is_query_running: Arc<AtomicBool>,
    pub notification_tx: mpsc::UnboundedSender<Notification>,
    pub notification_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<Notification>>>>,
    pub event_buffer: Arc<Mutex<VecDeque<SseEvent>>>,
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
            clients: Arc::new(RwLock::new(HashMap::new())),
            is_query_running: Arc::new(AtomicBool::new(false)),
            notification_tx,
            notification_rx: Arc::new(Mutex::new(Some(notification_rx))),
            event_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_EVENT_BUFFER))),
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
    pub fn broadcast(&self, mut event: SseEvent) {
        event.id = next_event_id();

        // Buffer for re-attach (ring buffer, capped at MAX_EVENT_BUFFER).
        {
            let mut buf = self.event_buffer.lock();
            if buf.len() >= MAX_EVENT_BUFFER {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }

        // Fan out to all connected clients.
        let clients = self.clients.read();
        for client in clients.values() {
            // Ignore send errors -- the client may have disconnected.
            let _ = client.tx.send(event.clone());
        }
    }

    /// Return all buffered events whose numeric ID is strictly greater than
    /// `last_id`. Used by frontends re-attaching after a disconnect.
    pub fn events_since(&self, last_id: &str) -> Vec<SseEvent> {
        let last: u64 = last_id.parse().unwrap_or(0);
        let buf = self.event_buffer.lock();
        buf.iter()
            .filter(|e| e.id.parse::<u64>().unwrap_or(0) > last)
            .cloned()
            .collect()
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify monotonic ID generation.
    #[test]
    fn next_event_id_is_monotonic() {
        let a: u64 = next_event_id().parse().unwrap();
        let b: u64 = next_event_id().parse().unwrap();
        let c: u64 = next_event_id().parse().unwrap();
        assert!(a < b);
        assert!(b < c);
    }

    /// Broadcast should buffer events and respect ring buffer cap.
    #[test]
    fn broadcast_buffers_and_caps() {
        // We can't easily construct a real QueryEngine in a unit test, so we
        // test the buffering logic directly via the event_buffer field.
        let buf: Arc<Mutex<VecDeque<SseEvent>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(MAX_EVENT_BUFFER)));

        // Fill beyond capacity.
        for i in 0..MAX_EVENT_BUFFER + 50 {
            let event = SseEvent {
                id: i.to_string(),
                event_type: "test".into(),
                data: serde_json::json!({}),
            };
            let mut b = buf.lock();
            if b.len() >= MAX_EVENT_BUFFER {
                b.pop_front();
            }
            b.push_back(event);
        }

        let b = buf.lock();
        assert_eq!(b.len(), MAX_EVENT_BUFFER);
        // The oldest remaining should be event #50 (0..49 were evicted).
        assert_eq!(b.front().unwrap().id, "50");
    }

    /// events_since should filter by numeric ID.
    #[test]
    fn events_since_filters_correctly() {
        let buf: Arc<Mutex<VecDeque<SseEvent>>> = Arc::new(Mutex::new(VecDeque::new()));
        {
            let mut b = buf.lock();
            for i in 10..15u64 {
                b.push_back(SseEvent {
                    id: i.to_string(),
                    event_type: "test".into(),
                    data: serde_json::json!({"n": i}),
                });
            }
        }

        // Simulate events_since logic.
        let last: u64 = "12".parse().unwrap();
        let result: Vec<SseEvent> = buf
            .lock()
            .iter()
            .filter(|e| e.id.parse::<u64>().unwrap_or(0) > last)
            .cloned()
            .collect();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "13");
        assert_eq!(result[1].id, "14");
    }
}
