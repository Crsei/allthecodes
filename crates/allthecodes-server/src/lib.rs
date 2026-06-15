//! `allthecodes-server` — transport abstraction and server lifecycle.
//!
//! This crate provides the types that decouple *what* servers to start
//! ([`ServerMode`]) from *how* to start them ([`ServerManager`]), and the
//! `--listen` argument parser ([`ListenUrl`]).
//!
//! It depends only on `axum`, `tokio`, `tokio-util`, and `tracing` — not on
//! `allthecodes-web` or `allthecodes-daemon` — so neither server crate needs
//! the other at compile time.

mod server_manager;
mod server_mode;
mod transport;

pub use server_manager::{ServerHandle, ServerManager};
pub use server_mode::ServerMode;
pub use transport::ListenUrl;
