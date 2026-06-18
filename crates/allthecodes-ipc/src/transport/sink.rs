//! Frontend output sinks and in-memory transport for tests.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::Arc;

use allthecodes_ipc_protocol::BackendMessage;
use async_trait::async_trait;
use parking_lot::Mutex;

use super::{IpcFrame, IpcReader, IpcTransport, IpcWriter};

trait SinkWriter: Send + Sync {
    fn write_one(&self, msg: &BackendMessage) -> io::Result<()>;
    fn write_many(&self, msgs: Vec<BackendMessage>) -> io::Result<()>;
}

struct StdoutWriter;

impl SinkWriter for StdoutWriter {
    fn write_one(&self, msg: &BackendMessage) -> io::Result<()> {
        let json = serde_json::to_string(msg).map_err(io::Error::other)?;
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{}", json)?;
        stdout.flush()
    }

    fn write_many(&self, msgs: Vec<BackendMessage>) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        for msg in msgs {
            let json = serde_json::to_string(&msg).map_err(io::Error::other)?;
            writeln!(stdout, "{}", json)?;
        }
        stdout.flush()
    }
}

#[derive(Default)]
struct MemoryWriter {
    messages: Mutex<Vec<BackendMessage>>,
}

impl SinkWriter for MemoryWriter {
    fn write_one(&self, msg: &BackendMessage) -> io::Result<()> {
        self.messages.lock().push(msg.clone());
        Ok(())
    }

    fn write_many(&self, msgs: Vec<BackendMessage>) -> io::Result<()> {
        self.messages.lock().extend(msgs);
        Ok(())
    }
}

/// Single point of egress for [`BackendMessage`]s to a frontend process.
#[derive(Clone)]
pub struct FrontendSink {
    writer: Arc<dyn SinkWriter>,
    memory: Option<Arc<MemoryWriter>>,
}

impl FrontendSink {
    /// Create a sink that writes JSON lines to stdout.
    pub fn stdout() -> Self {
        Self {
            writer: Arc::new(StdoutWriter),
            memory: None,
        }
    }

    /// Create an in-memory sink for tests and runtime adapters.
    pub fn memory() -> Self {
        let memory = Arc::new(MemoryWriter::default());
        Self {
            writer: memory.clone(),
            memory: Some(memory),
        }
    }

    /// Serialize and write a single message.
    pub fn send(&self, msg: &BackendMessage) -> io::Result<()> {
        self.writer.write_one(msg)
    }

    /// Write multiple messages as one batch.
    pub fn send_many(&self, msgs: impl IntoIterator<Item = BackendMessage>) -> io::Result<()> {
        self.writer.write_many(msgs.into_iter().collect())
    }

    /// Return messages captured by an in-memory sink.
    pub fn captured(&self) -> Vec<BackendMessage> {
        self.memory
            .as_ref()
            .map(|memory| memory.messages.lock().clone())
            .unwrap_or_default()
    }
}

/// In-memory framed transport for adapter and runtime tests.
pub struct MemoryTransport {
    inbound: Arc<Mutex<VecDeque<IpcFrame>>>,
    outbound: Arc<Mutex<Vec<IpcFrame>>>,
}

impl MemoryTransport {
    pub fn new(inbound: impl IntoIterator<Item = IpcFrame>) -> Self {
        Self {
            inbound: Arc::new(Mutex::new(inbound.into_iter().collect())),
            outbound: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn empty() -> Self {
        Self::new([])
    }

    pub fn outbound_frames(&self) -> Vec<IpcFrame> {
        self.outbound.lock().clone()
    }
}

pub struct MemoryReader {
    inbound: Arc<Mutex<VecDeque<IpcFrame>>>,
}

pub struct MemoryWriterFrames {
    outbound: Arc<Mutex<Vec<IpcFrame>>>,
}

impl IpcTransport for MemoryTransport {
    type Reader = MemoryReader;
    type Writer = MemoryWriterFrames;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (
            MemoryReader {
                inbound: self.inbound,
            },
            MemoryWriterFrames {
                outbound: self.outbound,
            },
        )
    }
}

#[async_trait]
impl IpcReader for MemoryReader {
    async fn read_frame(&mut self) -> io::Result<Option<IpcFrame>> {
        Ok(self.inbound.lock().pop_front())
    }
}

#[async_trait]
impl IpcWriter for MemoryWriterFrames {
    async fn write_frame(&mut self, frame: IpcFrame) -> io::Result<()> {
        self.outbound.lock().push(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_ipc_protocol::BackendMessage;

    #[test]
    fn memory_sink_captures_single_and_batch_messages() {
        let sink = FrontendSink::memory();
        sink.send(&BackendMessage::SystemInfo {
            text: "one".to_string(),
            level: "info".to_string(),
        })
        .unwrap();
        sink.send_many([BackendMessage::Error {
            message: "two".to_string(),
            recoverable: true,
        }])
        .unwrap();

        assert_eq!(sink.captured().len(), 2);
    }

    #[tokio::test]
    async fn memory_transport_reads_and_captures_batch_send() {
        let transport = MemoryTransport::new([
            IpcFrame::new(r#"{"type":"quit"}"#),
            IpcFrame::new(r#"{"type":"abort_query"}"#),
        ]);
        let (mut reader, mut writer) = transport.split();

        assert_eq!(
            reader.read_frame().await.unwrap().unwrap().line,
            r#"{"type":"quit"}"#
        );
        assert_eq!(
            reader.read_frame().await.unwrap().unwrap().line,
            r#"{"type":"abort_query"}"#
        );
        assert!(reader.read_frame().await.unwrap().is_none());

        writer.write_frame(IpcFrame::new("one")).await.unwrap();
        writer.write_frame(IpcFrame::new("two")).await.unwrap();

        assert_eq!(writer.outbound.lock().len(), 2);
    }
}
