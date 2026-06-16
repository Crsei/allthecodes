use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type EventSeq = u64;

pub const DEFAULT_EVENT_LOG_CAPACITY: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencedEvent<T> {
    pub seq: EventSeq,
    pub message: T,
}

impl<T> SequencedEvent<T> {
    pub fn new(seq: EventSeq, message: T) -> Self {
        Self { seq, message }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStatus {
    Complete,
    Compacted {
        requested_after_seq: EventSeq,
        oldest_seq: EventSeq,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayBatch<T> {
    pub status: ReplayStatus,
    pub requested_after_seq: EventSeq,
    pub high_watermark: EventSeq,
    pub events: Vec<SequencedEvent<T>>,
}

#[derive(Debug)]
struct EventLogInner<T> {
    events: Mutex<VecDeque<SequencedEvent<T>>>,
    next_seq: AtomicU64,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub struct EventLog<T> {
    inner: Arc<EventLogInner<T>>,
}

impl<T> EventLog<T>
where
    T: Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(EventLogInner {
                events: Mutex::new(VecDeque::with_capacity(capacity)),
                next_seq: AtomicU64::new(1),
                capacity,
            }),
        }
    }

    pub fn append(&self, message: T) -> SequencedEvent<T> {
        self.append_with(|_| message)
    }

    pub fn append_with<F>(&self, build: F) -> SequencedEvent<T>
    where
        F: FnOnce(EventSeq) -> T,
    {
        if self.inner.capacity == 0 {
            let seq = self.inner.next_seq.fetch_add(1, Ordering::SeqCst);
            return SequencedEvent::new(seq, build(seq));
        }

        let mut events = self.inner.events.lock().expect("event log mutex poisoned");
        let seq = self.inner.next_seq.fetch_add(1, Ordering::SeqCst);
        let event = SequencedEvent::new(seq, build(seq));
        if events.len() >= self.inner.capacity {
            events.pop_front();
        }
        events.push_back(event.clone());
        event
    }

    pub fn replay_after(&self, after_seq: Option<EventSeq>) -> ReplayBatch<T> {
        let events = self.inner.events.lock().expect("event log mutex poisoned");
        let high_watermark = events
            .back()
            .map(|event| event.seq)
            .unwrap_or_else(|| self.latest_seq());
        let requested_after_seq = after_seq.unwrap_or(high_watermark);
        let oldest_seq = events.front().map(|event| event.seq);
        let status = match oldest_seq {
            Some(oldest) if requested_after_seq + 1 < oldest => ReplayStatus::Compacted {
                requested_after_seq,
                oldest_seq: oldest,
            },
            _ => ReplayStatus::Complete,
        };
        let events = events
            .iter()
            .filter(|event| event.seq > requested_after_seq)
            .cloned()
            .collect();
        ReplayBatch {
            status,
            requested_after_seq,
            high_watermark,
            events,
        }
    }

    pub fn oldest_seq(&self) -> Option<EventSeq> {
        self.inner
            .events
            .lock()
            .expect("event log mutex poisoned")
            .front()
            .map(|event| event.seq)
    }

    pub fn latest_seq(&self) -> EventSeq {
        self.inner.next_seq.load(Ordering::SeqCst).saturating_sub(1)
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

impl<T> Default for EventLog<T>
where
    T: Clone,
{
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_LOG_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_monotonic_sequences() {
        let log = EventLog::new(10);

        let first = log.append("a");
        let second = log.append("b");

        assert_eq!(first.seq + 1, second.seq);
        assert_eq!(log.latest_seq(), second.seq);
        assert_eq!(log.oldest_seq(), Some(first.seq));
    }

    #[test]
    fn replay_after_returns_newer_events() {
        let log = EventLog::new(10);
        let first = log.append("a");
        let second = log.append("b");
        let third = log.append("c");

        let replay = log.replay_after(Some(first.seq));

        assert_eq!(replay.status, ReplayStatus::Complete);
        assert_eq!(replay.requested_after_seq, first.seq);
        assert_eq!(replay.high_watermark, third.seq);
        assert_eq!(replay.events, vec![second, third]);
    }

    #[test]
    fn replay_without_after_starts_at_live_tail() {
        let log = EventLog::new(10);
        log.append("a");

        let replay = log.replay_after(None);

        assert_eq!(replay.status, ReplayStatus::Complete);
        assert_eq!(replay.requested_after_seq, log.latest_seq());
        assert_eq!(replay.high_watermark, log.latest_seq());
        assert!(replay.events.is_empty());
    }

    #[test]
    fn bounded_log_reports_compacted_replay() {
        let log = EventLog::new(2);
        let first = log.append("a");
        log.append("b");
        let third = log.append("c");

        let replay = log.replay_after(Some(first.seq.saturating_sub(1)));

        assert_eq!(
            replay.status,
            ReplayStatus::Compacted {
                requested_after_seq: first.seq.saturating_sub(1),
                oldest_seq: third.seq - 1,
            }
        );
        assert_eq!(replay.events.len(), 2);
        assert_eq!(log.oldest_seq(), Some(third.seq - 1));
    }

    #[test]
    fn zero_capacity_keeps_only_sequence_counter() {
        let log = EventLog::new(0);
        let event = log.append("a");

        assert_eq!(event.seq, 1);
        assert_eq!(log.latest_seq(), 1);
        assert_eq!(log.oldest_seq(), None);
        assert!(log.replay_after(Some(0)).events.is_empty());
    }
}
