//! Agent IPC channel — the dedicated mpsc channel for agent + team events.
//!
//! Moved from `src/ipc/agent_channel.rs` to cc-types in Phase 6 so the
//! `ToolUseContext::bg_agent_tx` field in cc-types::tool can be typed without
//! depending on the future cc-ipc crate.

use super::agent_events::{AgentEvent, TeamEvent};

/// All events that flow through the agent channel.
#[derive(Debug)]
pub enum AgentIpcEvent {
    Agent(AgentEvent),
    Team(TeamEvent),
}

/// Sender half — injected into agent tool and team modules.
pub type AgentSender = tokio::sync::mpsc::UnboundedSender<AgentIpcEvent>;

/// Receiver half — consumed by the headless event loop.
pub type AgentReceiver = tokio::sync::mpsc::UnboundedReceiver<AgentIpcEvent>;

/// Create a new agent channel pair.
pub fn agent_channel() -> (AgentSender, AgentReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}
