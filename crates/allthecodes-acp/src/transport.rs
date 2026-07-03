//! Stdio transport for the ACP JSON-RPC runtime.
//!
//! Provides newline-delimited JSON-RPC 2.0 reader and writer over stdin/stdout.
//! Logs and diagnostics go to stderr; stdout contains only valid JSON-RPC frames.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::mpsc;

/// The ACP stdio reader: reads newline-delimited JSON lines from stdin.
pub struct AcpStdioReader {
    inner: BufReader<tokio::io::Stdin>,
}

impl Default for AcpStdioReader {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpStdioReader {
    /// Create a new stdin reader.
    pub fn new() -> Self {
        Self {
            inner: BufReader::new(tokio::io::stdin()),
        }
    }

    /// Read the next newline-delimited JSON line from stdin.
    ///
    /// Returns `None` on EOF.
    pub async fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.inner.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => Some(line),
            Err(e) => {
                tracing::warn!(error = %e, "acp stdin read error");
                None
            }
        }
    }
}

/// The ACP stdio writer: writes newline-delimited JSON-RPC frames to stdout.
pub struct AcpStdioWriter {
    inner: BufWriter<tokio::io::Stdout>,
}

impl Default for AcpStdioWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpStdioWriter {
    /// Create a new stdout writer.
    pub fn new() -> Self {
        Self {
            inner: BufWriter::new(tokio::io::stdout()),
        }
    }

    /// Write a single JSON-RPC message (as a JSON value) to stdout, followed by a newline.
    ///
    /// Flushes after every write so the client receives the frame immediately.
    pub async fn write_message(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
        let json = serde_json::to_string(value)?;
        self.inner.write_all(json.as_bytes()).await?;
        self.inner.write_all(b"\n").await?;
        self.inner.flush().await?;
        Ok(())
    }

    /// Write a batch JSON-RPC response as a single JSON array line.
    pub async fn write_batch(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
        // For batches, the value is already a JSON array.
        self.write_message(value).await
    }
}

/// An async channel-based sink for writing ACP notifications and responses.
/// Used by the runtime to send outgoing messages from concurrent tasks
/// without holding a mutable reference to the writer.
#[derive(Debug, Clone)]
pub struct AcpSink {
    tx: mpsc::UnboundedSender<serde_json::Value>,
}

impl AcpSink {
    /// Create a new sink connected to the given channel sender.
    pub fn new(tx: mpsc::UnboundedSender<serde_json::Value>) -> Self {
        Self { tx }
    }

    /// Enqueue a JSON-RPC message to be written to stdout.
    pub fn send(&self, value: serde_json::Value) -> bool {
        self.tx.send(value).is_ok()
    }
}

/// Spawn a background task that drains the sink channel and writes to stdout.
pub fn spawn_sink_writer(
    mut rx: mpsc::UnboundedReceiver<serde_json::Value>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut writer = AcpStdioWriter::new();
        while let Some(value) = rx.recv().await {
            if let Err(e) = writer.write_message(&value).await {
                tracing::error!(error = %e, "acp stdout write failed");
                break;
            }
        }
    })
}
