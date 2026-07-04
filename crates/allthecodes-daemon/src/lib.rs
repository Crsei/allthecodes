//! cc-daemon — KAIROS daemon contracts and runtime owner.

pub mod account_auth;
pub mod automation_state;
pub mod bridge_worker;
pub mod channels;
pub mod gateway_bridge;
pub mod gateway_client;
pub mod gateway_routes;
mod gateway_run_events;
pub mod memory_log;
pub mod notification;
pub mod operation_lock;
pub mod process_state;
pub mod protocol;
pub mod readiness;
pub mod routes;
pub mod runtime;
pub mod scheduler_loop;
pub mod server;
pub mod sse;
pub mod state;
pub mod supervisor;
pub mod team_memory_proxy;
pub mod tick;
pub mod web;
pub mod webhook;

pub(crate) fn protocol_store() -> protocol::DaemonProtocolStore {
    protocol::DaemonProtocolStore::new(process_state::daemon_dir())
}

// Re-export build_router for use by allthecodes-server integration.
pub use server::build_router;
