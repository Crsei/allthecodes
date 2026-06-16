use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::EventSeq;

pub const DEFAULT_OUTPUT_RETENTION_BYTES: usize = 1024 * 1024;
pub const DEFAULT_DETACH_RESUME_TTL: Duration = Duration::from_secs(30);
pub const DEFAULT_EXITED_OUTPUT_RETENTION_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    Pty,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputLifecycleState {
    Starting,
    Running,
    Exited,
    Expired,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputEvent {
    pub seq: EventSeq,
    pub stream: OutputStream,
    pub chunk: String,
    pub timestamp_ms: u64,
    pub process_or_run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputReadBatch {
    pub events: Vec<OutputEvent>,
    pub next_seq: EventSeq,
    pub truncated: bool,
    pub first_available_seq: EventSeq,
    pub state: OutputLifecycleState,
}

#[derive(Debug)]
struct OutputRetentionInner {
    events: Mutex<VecDeque<OutputEvent>>,
    next_seq: AtomicU64,
    first_available_seq: AtomicU64,
    total_bytes: AtomicU64,
    max_bytes: usize,
    state: Mutex<OutputLifecycleState>,
}

#[derive(Debug, Clone)]
pub struct OutputRetention {
    inner: Arc<OutputRetentionInner>,
}

impl OutputRetention {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(OutputRetentionInner {
                events: Mutex::new(VecDeque::new()),
                next_seq: AtomicU64::new(1),
                first_available_seq: AtomicU64::new(1),
                total_bytes: AtomicU64::new(0),
                max_bytes,
                state: Mutex::new(OutputLifecycleState::Starting),
            }),
        }
    }

    pub fn append(
        &self,
        stream: OutputStream,
        chunk: impl Into<String>,
        process_or_run_id: impl Into<String>,
    ) -> OutputEvent {
        self.append_at(stream, chunk, process_or_run_id, now_millis())
    }

    pub fn append_at(
        &self,
        stream: OutputStream,
        chunk: impl Into<String>,
        process_or_run_id: impl Into<String>,
        timestamp_ms: u64,
    ) -> OutputEvent {
        let seq = self.inner.next_seq.fetch_add(1, Ordering::SeqCst);
        let event = OutputEvent {
            seq,
            stream,
            chunk: chunk.into(),
            timestamp_ms,
            process_or_run_id: process_or_run_id.into(),
        };
        self.store(event.clone());
        event
    }

    pub fn read(&self, after_seq: Option<EventSeq>, limit_bytes: usize) -> OutputReadBatch {
        let first_available_seq = self.first_available_seq();
        let requested_after_seq = after_seq.unwrap_or(first_available_seq.saturating_sub(1));
        let mut truncated = requested_after_seq + 1 < first_available_seq;
        let mut bytes = 0usize;
        let mut events = Vec::new();
        let limit_bytes = limit_bytes.max(1);

        {
            let retained = self
                .inner
                .events
                .lock()
                .expect("output retention mutex poisoned");
            for event in retained
                .iter()
                .filter(|event| event.seq > requested_after_seq)
            {
                let event_len = event.chunk.len();
                if !events.is_empty() && bytes.saturating_add(event_len) > limit_bytes {
                    truncated = true;
                    break;
                }
                bytes = bytes.saturating_add(event_len);
                events.push(event.clone());
                if bytes >= limit_bytes {
                    truncated = retained.iter().any(|candidate| candidate.seq > event.seq);
                    break;
                }
            }
        }

        let next_seq = events
            .last()
            .map(|event| event.seq.saturating_add(1))
            .unwrap_or_else(|| self.latest_seq().saturating_add(1))
            .max(requested_after_seq.saturating_add(1));

        OutputReadBatch {
            events,
            next_seq,
            truncated,
            first_available_seq,
            state: self.state(),
        }
    }

    pub fn set_state(&self, state: OutputLifecycleState) {
        *self
            .inner
            .state
            .lock()
            .expect("output retention state mutex poisoned") = state;
    }

    pub fn state(&self) -> OutputLifecycleState {
        *self
            .inner
            .state
            .lock()
            .expect("output retention state mutex poisoned")
    }

    pub fn latest_seq(&self) -> EventSeq {
        self.inner.next_seq.load(Ordering::SeqCst).saturating_sub(1)
    }

    pub fn first_available_seq(&self) -> EventSeq {
        self.inner.first_available_seq.load(Ordering::SeqCst)
    }

    pub fn retained_bytes(&self) -> usize {
        self.inner.total_bytes.load(Ordering::SeqCst) as usize
    }

    pub fn max_bytes(&self) -> usize {
        self.inner.max_bytes
    }

    fn store(&self, event: OutputEvent) {
        if self.inner.max_bytes == 0 {
            self.inner
                .first_available_seq
                .store(event.seq.saturating_add(1), Ordering::SeqCst);
            self.inner.total_bytes.store(0, Ordering::SeqCst);
            return;
        }

        let mut retained = self
            .inner
            .events
            .lock()
            .expect("output retention mutex poisoned");
        let mut total = self.inner.total_bytes.load(Ordering::SeqCst) as usize;
        total = total.saturating_add(event.chunk.len());
        retained.push_back(event);

        while total > self.inner.max_bytes {
            let Some(removed) = retained.pop_front() else {
                break;
            };
            total = total.saturating_sub(removed.chunk.len());
            self.inner
                .first_available_seq
                .store(removed.seq.saturating_add(1), Ordering::SeqCst);
        }
        self.inner.total_bytes.store(total as u64, Ordering::SeqCst);
    }
}

impl Default for OutputRetention {
    fn default() -> Self {
        Self::new(DEFAULT_OUTPUT_RETENTION_BYTES)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_incremental_events_and_next_seq() {
        let output = OutputRetention::new(1024);
        output.set_state(OutputLifecycleState::Running);
        output.append_at(OutputStream::Stdout, "one", "run-1", 10);
        output.append_at(OutputStream::Stderr, "two", "run-1", 11);

        let batch = output.read(Some(1), 1024);

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].seq, 2);
        assert_eq!(batch.events[0].stream, OutputStream::Stderr);
        assert_eq!(batch.next_seq, 3);
        assert!(!batch.truncated);
        assert_eq!(batch.first_available_seq, 1);
        assert_eq!(batch.state, OutputLifecycleState::Running);
    }

    #[test]
    fn bounded_retention_drops_old_chunks_and_reports_truncation() {
        let output = OutputRetention::new(6);
        output.append_at(OutputStream::Pty, "abc", "pty-1", 10);
        output.append_at(OutputStream::Pty, "def", "pty-1", 11);
        output.append_at(OutputStream::Pty, "ghi", "pty-1", 12);

        let batch = output.read(Some(0), 1024);

        assert!(batch.truncated);
        assert_eq!(batch.first_available_seq, 2);
        assert_eq!(
            batch
                .events
                .iter()
                .map(|event| event.chunk.as_str())
                .collect::<Vec<_>>(),
            vec!["def", "ghi"]
        );
        assert!(output.retained_bytes() <= 6);
    }

    #[test]
    fn byte_limited_read_keeps_progress() {
        let output = OutputRetention::new(1024);
        output.append_at(OutputStream::Stdout, "abcd", "run-1", 10);
        output.append_at(OutputStream::Stdout, "efgh", "run-1", 11);

        let batch = output.read(Some(0), 4);

        assert!(batch.truncated);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.next_seq, 2);
    }

    #[test]
    fn late_output_after_exit_is_retained() {
        let output = OutputRetention::new(1024);
        output.set_state(OutputLifecycleState::Exited);
        output.append_at(OutputStream::System, "exit 0", "run-1", 10);
        output.append_at(OutputStream::Stdout, "late", "run-1", 11);

        let batch = output.read(Some(0), 1024);

        assert_eq!(batch.state, OutputLifecycleState::Exited);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[1].chunk, "late");
    }

    #[test]
    fn empty_read_after_future_cursor_does_not_move_next_seq_backwards() {
        let output = OutputRetention::new(1024);
        output.append_at(OutputStream::Stdout, "one", "run-1", 10);

        let batch = output.read(Some(99), 1024);

        assert!(batch.events.is_empty());
        assert!(!batch.truncated);
        assert_eq!(batch.next_seq, 100);
        assert_eq!(batch.first_available_seq, 1);
    }

    #[test]
    fn zero_retention_keeps_sequence_watermark() {
        let output = OutputRetention::new(0);
        let event = output.append_at(OutputStream::Stdout, "dropped", "run-1", 10);

        assert_eq!(event.seq, 1);
        assert_eq!(output.first_available_seq(), 2);
        let batch = output.read(Some(0), 1024);
        assert!(batch.truncated);
        assert!(batch.events.is_empty());
    }
}
