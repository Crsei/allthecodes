//! Agent Teams / Multi-Agent Swarm system.
//!
//! Corresponds to TypeScript: `utils/swarm/`, `coordinator/`, and related tools.
//!
//! Provides multi-agent coordination where a Team Lead creates and manages
//! multiple Teammate agents running in parallel. Communication happens via
//! file-based mailbox IPC (`{data_root}/teams/{name}/inboxes/`).

pub mod backend;
pub mod command;
pub mod constants;
pub mod context;
pub mod coordinator;
pub mod helpers;
pub mod identity;
pub mod in_process;
pub mod layout_manager;
pub mod loaded_threads;
pub mod mailbox;
pub mod multi_agent_v2;
pub mod pr_activity;
pub mod protocol;
pub mod reconnection;
pub mod runner;
pub mod send_message;
pub(crate) mod storage_paths;
pub mod team_spawn;
pub mod tool_specs;
pub mod types;

/// Check if Agent Teams is enabled via the upstream env-var switch or a
/// session-local experimental override.
pub fn is_agent_teams_enabled() -> bool {
    allthecodes_config::features::enabled(allthecodes_config::features::Feature::AgentTeams)
}

/// Check if Agent Teams is active in the given app state.
///
/// Returns true when **either** the env-var opt-in is set **or** an active
/// team context already exists. The second condition lets conversation-triggered
/// flows (`/team create`, `TeamSpawn`) unlock team tools without requiring a
/// pre-exported env var.
pub fn is_agent_teams_active(app_state: &allthecodes_engine::types::app_state::AppState) -> bool {
    if is_agent_teams_enabled() {
        return true;
    }
    app_state
        .team_context
        .as_ref()
        .map(|tc| !tc.team_name.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_gate_default_off() {
        let _ = is_agent_teams_enabled();
    }
}
