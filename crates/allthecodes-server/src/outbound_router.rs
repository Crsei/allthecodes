use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::{ConnectionId, SequencedEvent};

pub const DEFAULT_WRITER_CHANNEL_CAPACITY: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterSendError<T> {
    UnknownConnection { event: SequencedEvent<T> },
    Full { event: SequencedEvent<T> },
    Closed { event: SequencedEvent<T> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RouterBroadcastReport {
    pub delivered: usize,
    pub full: usize,
    pub closed: usize,
}

#[derive(Debug)]
struct OutboundRouterInner<T> {
    writers: Mutex<HashMap<ConnectionId, mpsc::Sender<SequencedEvent<T>>>>,
    writer_capacity: usize,
}

#[derive(Debug, Clone)]
pub struct OutboundRouter<T> {
    inner: Arc<OutboundRouterInner<T>>,
}

impl<T> OutboundRouter<T>
where
    T: Clone,
{
    pub fn new(writer_capacity: usize) -> Self {
        Self {
            inner: Arc::new(OutboundRouterInner {
                writers: Mutex::new(HashMap::new()),
                writer_capacity,
            }),
        }
    }

    pub fn register(&self, connection_id: ConnectionId) -> mpsc::Receiver<SequencedEvent<T>> {
        let (sender, receiver) = mpsc::channel(self.inner.writer_capacity);
        self.inner
            .writers
            .lock()
            .expect("outbound router mutex poisoned")
            .insert(connection_id, sender);
        receiver
    }

    pub fn unregister(&self, connection_id: &ConnectionId) -> bool {
        self.inner
            .writers
            .lock()
            .expect("outbound router mutex poisoned")
            .remove(connection_id)
            .is_some()
    }

    pub fn send_to(
        &self,
        connection_id: &ConnectionId,
        event: SequencedEvent<T>,
    ) -> Result<(), RouterSendError<T>> {
        let sender = {
            self.inner
                .writers
                .lock()
                .expect("outbound router mutex poisoned")
                .get(connection_id)
                .cloned()
        };

        let Some(sender) = sender else {
            return Err(RouterSendError::UnknownConnection { event });
        };

        match sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(event)) => Err(RouterSendError::Full { event }),
            Err(mpsc::error::TrySendError::Closed(event)) => Err(RouterSendError::Closed { event }),
        }
    }

    pub fn broadcast(&self, event: SequencedEvent<T>) -> RouterBroadcastReport {
        let senders: Vec<_> = self
            .inner
            .writers
            .lock()
            .expect("outbound router mutex poisoned")
            .iter()
            .map(|(id, sender)| (id.clone(), sender.clone()))
            .collect();

        let mut report = RouterBroadcastReport::default();
        let mut stale = Vec::new();
        for (connection_id, sender) in senders {
            match sender.try_send(event.clone()) {
                Ok(()) => report.delivered += 1,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    report.full += 1;
                    stale.push(connection_id);
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    report.closed += 1;
                    stale.push(connection_id);
                }
            }
        }

        if !stale.is_empty() {
            let mut writers = self
                .inner
                .writers
                .lock()
                .expect("outbound router mutex poisoned");
            for connection_id in stale {
                writers.remove(&connection_id);
            }
        }

        report
    }

    pub fn disconnect_all(&self) -> usize {
        self.inner
            .writers
            .lock()
            .expect("outbound router mutex poisoned")
            .drain()
            .count()
    }

    pub fn len(&self) -> usize {
        self.inner
            .writers
            .lock()
            .expect("outbound router mutex poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Default for OutboundRouter<T>
where
    T: Clone,
{
    fn default() -> Self {
        Self::new(DEFAULT_WRITER_CHANNEL_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, message: &str) -> SequencedEvent<String> {
        SequencedEvent::new(seq, message.to_string())
    }

    #[tokio::test]
    async fn sends_to_registered_connection() {
        let router = OutboundRouter::new(1);
        let connection_id = ConnectionId::from_static("test-1");
        let mut rx = router.register(connection_id.clone());

        router.send_to(&connection_id, event(1, "a")).unwrap();

        assert_eq!(rx.recv().await.unwrap(), event(1, "a"));
    }

    #[test]
    fn reports_full_channel_without_blocking() {
        let router = OutboundRouter::new(1);
        let connection_id = ConnectionId::from_static("test-1");
        let _rx = router.register(connection_id.clone());

        router.send_to(&connection_id, event(1, "a")).unwrap();
        let error = router.send_to(&connection_id, event(2, "b")).unwrap_err();

        assert!(matches!(error, RouterSendError::Full { .. }));
        assert_eq!(router.len(), 1);
    }

    #[tokio::test]
    async fn broadcasts_to_all_registered_connections() {
        let router = OutboundRouter::new(1);
        let mut one = router.register(ConnectionId::from_static("one"));
        let mut two = router.register(ConnectionId::from_static("two"));

        let report = router.broadcast(event(1, "a"));

        assert_eq!(
            report,
            RouterBroadcastReport {
                delivered: 2,
                full: 0,
                closed: 0
            }
        );
        assert_eq!(one.recv().await.unwrap(), event(1, "a"));
        assert_eq!(two.recv().await.unwrap(), event(1, "a"));
    }

    #[test]
    fn unregister_and_disconnect_all_remove_writers() {
        let router = OutboundRouter::<String>::new(1);
        let one = ConnectionId::from_static("one");
        router.register(one.clone());
        router.register(ConnectionId::from_static("two"));

        assert!(router.unregister(&one));
        assert_eq!(router.len(), 1);
        assert_eq!(router.disconnect_all(), 1);
        assert!(router.is_empty());
    }
}
