//! Remote-control gateway foundation.
//!
//! This crate owns remote source identity, deterministic session-key
//! derivation, and gateway-local configuration models. Runtime daemon,
//! TUI, IPC, and `QueryEngine` integration belong in `allthecodes`
//! adapter modules so dependency direction stays one-way.

pub mod adapters;
pub mod api;
mod api_support;
pub mod auth;
pub mod config;
pub mod delivery;
pub mod events;
pub mod policy;
pub mod run;
pub mod runner;
pub mod session_key;
pub mod source;
pub mod store;
pub mod webhook;
mod webhook_hmac;
mod webhook_render;

pub use adapters::{
    AdapterProvider, AdapterRegistry, AdapterState, AdapterStatus, AdapterTestMessage,
    RemoteAdapter,
};
pub use allthecodes_types::output::{
    EventSeq, OutputEvent, OutputLifecycleState, OutputReadBatch, OutputStream,
};
pub use api::{GatewayApiState, GatewayBusySnapshotProvider, StaticBusySnapshotProvider};
pub use auth::{GatewayAuthMode, GatewayAuthVerifier, RemoteGatewayAuth};
pub use config::{GatewayConfig, GatewayLimits, GatewayPersistence, GatewaySecurityConfig};
pub use delivery::{
    CallbackDeliverySink, ChannelDeliverySink, DeliveryPayload, DeliveryRecord, DeliveryRouter,
    DeliveryStatus, DeliveryTarget,
};
pub use events::{RunEvent, RunEventKind};
pub use policy::{BusyDecision, BusySnapshot, GatewayPolicy, GatewayRateLimiter};
pub use run::{
    BusyPolicy, CreateRunOutcome, GatewayDiagnostic, GatewayError, RunId, RunMeta, RunPolicy,
    RunRequest, RunStatus,
};
pub use runner::{
    GatewayCommand, GatewayCommandKind, GatewayCommandReceipt, GatewayCommandSink,
    GatewayRunAction, GatewayRunSubmission, GatewayRunner,
};
pub use session_key::{SessionKey, SessionKeyPolicy};
pub use source::{RemoteSource, RemoteSourceMetadata, RemoteTransport};
pub use store::{GatewayRecoveryReport, GatewayStore, SessionLock, SessionLockOutcome};
pub use webhook::{WebhookRouteConfig, WebhookVerifier};
