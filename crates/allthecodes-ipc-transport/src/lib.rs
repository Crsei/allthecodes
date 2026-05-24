//! Transport, framing, sinks, and queue pressure helpers for IPC.

mod event_class;
mod frame;
mod jsonl;
mod sink;

pub use event_class::{
    classify_event, event_type, ClientEventQueue, EventClass, QueuePressureDiagnostic,
};
pub use frame::{IpcFrame, IpcReader, IpcTransport, IpcWriter};
pub use jsonl::{
    parse_frontend_line, JsonlStdioReader, JsonlStdioTransport, JsonlStdioWriter,
    ParsedFrontendLine,
};
pub use sink::{FrontendSink, MemoryTransport};
