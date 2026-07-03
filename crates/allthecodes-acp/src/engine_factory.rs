//! Engine factory — trait and params for creating per-session QueryEngines.
//!
//! The trait is implemented by the root binary bridge
//! (`full_init::acp_runtime_bridge`) to inject root-owned dependencies.
//!
//! NOTE: The trait is defined in `lib.rs`. This module re-exports it and
//! also exposes `AcpEngineParams` so engine_factory-dependent code has a
//! single import.

pub use crate::{AcpEngineFactory, AcpEngineParams};
