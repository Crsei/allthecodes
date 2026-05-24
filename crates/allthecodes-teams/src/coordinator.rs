//! Coordinator mode prompt and gate.
//!
//! This is the first parity slice for Bun's `coordinatorMode.ts`: it adds the
//! runtime gate and system-prompt section without changing worker tool policy.

use allthecodes_config::features::{self, Feature, FeatureFlags};

pub const WORKER_AGENT_TYPE: &str = "worker";
pub const TEAMMATE_AGENT_TYPE: &str = "teammate";

/// True when coordinator mode is explicitly enabled for the current session.
pub fn is_coordinator_mode_enabled() -> bool {
    features::enabled(Feature::Coordinator)
}

/// Enable or disable coordinator mode for the current process/session.
///
/// Runtime toggling preserves other effective feature gates and automatically
/// enables Agent Teams when coordinator mode is turned on because the
/// coordinator runtime delegates through the team backend.
pub fn set_coordinator_mode_enabled(enabled: bool) {
    let mut flags: FeatureFlags = features::current();
    flags.coordinator = enabled;
    if enabled {
        flags.agent_teams = true;
    }
    features::set_runtime_override(flags);
}

/// Default teammate agent type for newly spawned in-process teammates.
pub fn default_teammate_agent_type() -> &'static str {
    if is_coordinator_mode_enabled() {
        WORKER_AGENT_TYPE
    } else {
        TEAMMATE_AGENT_TYPE
    }
}

/// Optional system prompt section injected when coordinator mode is enabled.
pub fn coordinator_prompt_section() -> Option<String> {
    is_coordinator_mode_enabled().then(coordinator_system_prompt)
}

/// Build the coordinator prompt section.
pub fn coordinator_system_prompt() -> String {
    concat!(
        "# Coordinator Mode\n\n",
        "You are coordinating an Agent Team. Treat yourself as the team lead: ",
        "break work into small, verifiable tasks, delegate only when parallel ",
        "work materially helps, and keep ownership of final integration and ",
        "verification.\n\n",
        "## Worker Coordination\n",
        "- Spawn or address workers only for bounded tasks with clear ownership.\n",
        "- Use SendMessage for direct worker updates and concise handoffs.\n",
        "- Use TaskList to inspect active work before assigning more work.\n",
        "- Use TaskStop to cancel stale, duplicate, or unsafe worker tasks.\n",
        "- Do not assume a worker has finished until task state or mailbox output ",
        "proves it.\n\n",
        "## Completion Rules\n",
        "- Integrate worker results before reporting completion.\n",
        "- Verify the user-facing goal directly; worker success alone is not enough.\n",
        "- If a worker is blocked, decide whether to unblock, reassign, or stop the task.\n",
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    #[test]
    fn coordinator_prompt_names_worker_tools_and_lead_role() {
        let prompt = coordinator_system_prompt();
        assert!(prompt.contains("Coordinator Mode"));
        assert!(prompt.contains("team lead"));
        assert!(prompt.contains("SendMessage"));
        assert!(prompt.contains("TaskList"));
        assert!(prompt.contains("TaskStop"));
        assert!(prompt.contains("worker"));
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_prompt_section_respects_feature_gate() {
        let _guard = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());
        assert!(coordinator_prompt_section().is_none());

        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        features::set_runtime_override(flags);
        assert!(coordinator_prompt_section().is_some());
    }

    #[test]
    #[serial_test::serial]
    fn runtime_toggle_preserves_flags_and_enables_agent_teams() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.team_memory = true;
        features::set_runtime_override(flags);

        set_coordinator_mode_enabled(true);
        let current = features::current();
        assert!(current.coordinator);
        assert!(current.agent_teams);
        assert!(current.team_memory);
        assert_eq!(default_teammate_agent_type(), WORKER_AGENT_TYPE);

        set_coordinator_mode_enabled(false);
        let current = features::current();
        assert!(!current.coordinator);
        assert!(
            current.agent_teams,
            "stop should not silently disable Agent Teams"
        );
        assert!(current.team_memory);
        assert_eq!(default_teammate_agent_type(), TEAMMATE_AGENT_TYPE);
    }
}
