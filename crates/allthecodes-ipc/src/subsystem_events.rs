//! Broadcast bus for in-process subsystem events.

use tokio::sync::broadcast;

use allthecodes_ipc_protocol::subsystem_events::SubsystemEvent;

/// Broadcast-based event bus for subsystem events.
pub struct SubsystemEventBus {
    tx: broadcast::Sender<SubsystemEvent>,
}

impl SubsystemEventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { tx }
    }

    pub fn sender(&self) -> broadcast::Sender<SubsystemEvent> {
        self.tx.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SubsystemEvent> {
        self.tx.subscribe()
    }
}

impl Default for SubsystemEventBus {
    fn default() -> Self {
        Self::new()
    }
}
