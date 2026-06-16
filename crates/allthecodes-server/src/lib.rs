//! `allthecodes-server` — transport abstraction and server lifecycle.
//!
//! This crate provides the types that decouple *what* servers to start
//! ([`ServerMode`]) from *how* to start them ([`ServerManager`]), and the
//! `--listen` argument parser ([`ListenUrl`]).
//!
//! It depends only on `axum`, `tokio`, `tokio-util`, and `tracing` — not on
//! `allthecodes-web` or `allthecodes-daemon` — so neither server crate needs
//! the other at compile time.

mod event_log;
mod outbound_router;
mod server_manager;
mod server_mode;
mod transport;

pub use event_log::{
    EventLog, EventSeq, ReplayBatch, ReplayStatus, SequencedEvent, DEFAULT_EVENT_LOG_CAPACITY,
};
pub use outbound_router::{
    OutboundRouter, RouterBroadcastReport, RouterSendError, DEFAULT_WRITER_CHANNEL_CAPACITY,
};
pub use server_manager::{ServerHandle, ServerManager};
pub use server_mode::ServerMode;
pub use transport::{
    ConnectionClosedReason, ConnectionId, ConnectionOrigin, ListenUrl, OriginRejection,
    OutboundEnvelope, TransportEvent, TransportKind,
};
