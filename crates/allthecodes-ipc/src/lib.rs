//! cc-ipc - IPC spine between backend and frontend.
//!
//! Issue #76 (`[workspace-split] Phase 7`): target destination for
//! `crates/allthecodes/src/ipc/` plus the `tools/system_status.rs` wrapper
//! (which reads `ipc::subsystem_handlers` and therefore moves with the IPC
//! crate to avoid a `cc-tools -> cc-ipc` edge).
//!
//! Agent event / command / channel types live in
//! `cc-types::{agent_events, agent_types, agent_channel}` so the IPC runtime
//! can share them without depending on the root binary crate.

pub mod agent_handlers;
pub mod adapters;
pub mod client;
pub mod headless;
pub mod runtime;
pub mod subsystem_events;
pub mod subsystem_handlers;
pub mod transport;

pub use subsystem_events as event_bus;

pub use allthecodes_types::{
    agent_channel::{agent_channel, AgentIpcEvent, AgentReceiver, AgentSender},
    agent_events::{AgentCommand, AgentEvent, TeamCommand, TeamEvent},
    agent_types::{AgentInfo, AgentNode, TeamMemberInfo},
};
