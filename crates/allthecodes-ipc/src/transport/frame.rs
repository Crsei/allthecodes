//! Transport-neutral IPC frames and traits.

use std::io;

use async_trait::async_trait;

/// A single newline-delimited IPC frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcFrame {
    pub line: String,
}

impl IpcFrame {
    pub fn new(line: impl Into<String>) -> Self {
        Self { line: line.into() }
    }

    pub fn json<T: serde::Serialize>(value: &T) -> io::Result<Self> {
        serde_json::to_string(value)
            .map(Self::new)
            .map_err(io::Error::other)
    }
}

#[async_trait]
pub trait IpcReader: Send {
    async fn read_frame(&mut self) -> io::Result<Option<IpcFrame>>;
}

#[async_trait]
pub trait IpcWriter: Send {
    async fn write_frame(&mut self, frame: IpcFrame) -> io::Result<()>;
}

pub trait IpcTransport {
    type Reader: IpcReader;
    type Writer: IpcWriter;

    fn split(self) -> (Self::Reader, Self::Writer);
}
