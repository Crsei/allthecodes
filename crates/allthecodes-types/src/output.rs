use std::time::Duration;

use serde::{Deserialize, Serialize};

pub type EventSeq = u64;

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
