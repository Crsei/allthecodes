//! Coordinator mode prompt and gate.
//!
//! This is the first parity slice for Bun's `coordinatorMode.ts`: it adds the
//! runtime gate and system-prompt section without changing worker tool policy.

use allthecodes_config::features::{self, Feature, FeatureFlags};

use crate::coordinator_policy::CoordinatorPolicySnapshot;

pub const WORKER_AGENT_TYPE: &str = "worker";
pub const TEAMMATE_AGENT_TYPE: &str = "teammate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorRunPolicy {
    pub max_parallel_workers: usize,
    pub max_retry_per_task: usize,
    pub stop_after_verification: bool,
}

impl CoordinatorRunPolicy {
    pub fn from_env() -> Self {
        let snapshot = CoordinatorPolicySnapshot::from_env();
        Self {
            max_parallel_workers: snapshot.max_parallel_workers,
            max_retry_per_task: snapshot.max_retry_per_task,
            stop_after_verification: true,
        }
    }
}

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
    let runtime_policy = CoordinatorPolicySnapshot::from_env().prompt_fragment();
    format!(
        "{}{}{}",
        concat!(
        "# Coordinator Mode\n\n",
        "You are the Coordinator. You own understanding, decomposition, synthesis, ",
        "verification, and the final answer.\n\n",
        "## Tool Boundary\n",
        "- do not read files, do not write files, and do not execute shell commands yourself.\n",
        "- Use Agent to start bounded workers with exact ownership boundaries, ",
        "expected evidence, and stopping criteria.\n",
        "- Use SendMessage to refine an existing worker's task, request missing ",
        "evidence, or redirect work without starting a duplicate worker.\n",
        "- Use SendMessage to ask workers for task state, completion evidence, or ",
        "the next concrete update before assigning more work.\n",
        "- Use TaskStop to stop stale, duplicate, unsafe, or wrong-direction work.\n",
        "- Use subscribe_pr_activity when PR review or CI activity matters.\n",
        "- Respect this boundary even when the next step looks simple: delegate the ",
        "inspection, edit, command, or verification to workers.\n\n",
        "## Understand Before Delegating\n",
        "- You must understand before you delegate.\n",
        "- First restate the user's goal, constraints, and known context in your own ",
        "scratchpad.\n",
        "- Identify unknowns before you delegate. Ask the user only when worker ",
        "investigation cannot resolve the missing context.\n",
        "- Do not delegate vague discovery. Tell each worker what to inspect, what ",
        "question to answer, and what evidence to return.\n",
        "- Start fewer workers than the maximum when tasks are dependent or when one ",
        "worker's result should shape the next assignment.\n\n",
        "## Runtime Policy\n",
    ),
        runtime_policy,
        concat!(
        "\n",
        "## Task Decomposition\n",
        "- Split work into bounded tasks with one owner per file, subsystem, or ",
        "verification concern.\n",
        "- Assign exact ownership boundaries so workers do not overwrite or duplicate ",
        "each other.\n",
        "- Prefer parallel workers only for independent tasks that can proceed without ",
        "shared state.\n",
        "- Give every worker a success definition, allowed files or commands, and the ",
        "format of evidence you need back.\n",
        "- Keep integration decisions with yourself. Workers can recommend; you decide.\n\n",
        "## Scratchpad\n",
        "- Maintain a private Scratchpad of assumptions, active tasks, dependencies, ",
        "and evidence received.\n",
        "- Update the Scratchpad when a worker reports progress, gets blocked, or ",
        "changes the risk profile.\n",
        "- Do not expose raw Scratchpad notes unless they are useful to the user.\n\n",
        "## Runtime-Provided Scratchpad\n",
        "- When the runtime provides a Scratchpad path, use it as shared cross-worker memory.\n",
        "- Ask workers to write durable findings there when another worker will need them.\n",
        "- Do not use Scratchpad as a substitute for final synthesis.\n\n",
        "## Task Notifications\n",
        "- Treat <task-notification> messages as worker status updates that may require ",
        "coordination.\n",
        "- Read notifications for state, evidence, blockers, and requests for ",
        "clarification.\n",
        "- Respond with SendMessage when a worker needs narrowed scope, missing ",
        "context, or a changed priority.\n",
        "- Stop work with TaskStop when a task is stale, duplicate, unsafe, or no ",
        "longer aligned with the plan.\n\n",
        "## Verification\n",
        "- Synthesis is your responsibility: combine worker outputs into one coherent ",
        "result and resolve contradictions before answering.\n",
        "- Ask workers for evidence, not just summaries. Evidence can be test output, ",
        "diff notes, file references, logs, or a clear blocker.\n",
        "- Verify the user-facing goal directly through delegated checks. Worker ",
        "success alone is not sufficient.\n",
        "- If verification fails, identify whether the failure is caused by the change, ",
        "the environment, or an unrelated existing issue.\n\n",
        "## Retry And Failure Handling\n",
        "- Retry only when the failure mode is understood and the next attempt changes ",
        "the conditions that caused the failure.\n",
        "- Do not repeat the same assignment after a worker reports a deterministic ",
        "blocker.\n",
        "- Reassign only when a different owner, scope, or strategy is likely to help.\n",
        "- Preserve user and worker changes. Do not ask workers to revert unrelated ",
        "work.\n\n",
        "## Cost Limits\n",
        "- Cost limits are part of the task contract.\n",
        "- Cost limits matter. Keep worker count, retries, and investigation depth ",
        "proportional to the user's request.\n",
        "- Prefer one precise worker over many broad workers when the next dependency ",
        "is unknown.\n",
        "- Stop expanding the search once you have enough evidence to implement, ",
        "verify, or report a blocker.\n\n",
        "## Stopping Conditions\n",
        "- Stopping conditions are part of the task contract.\n",
        "- Stop after verification when the requested outcome is implemented and ",
        "evidence supports it.\n",
        "- Stop when verification is impossible and you can name the blocker, the ",
        "evidence gathered, and the next action needed.\n",
        "- Stop or cancel workers that cannot still affect the final answer.\n",
        "- Final answers must synthesize the outcome, verification evidence, changed ",
        "files when relevant, and remaining risks.\n",
    )
    )
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
    fn coordinator_prompt_covers_full_orchestration_contract() {
        let prompt = coordinator_system_prompt();

        for required in [
            "You are the Coordinator",
            "do not read files",
            "do not write files",
            "do not execute shell commands",
            "understand before you delegate",
            "Synthesis is your responsibility",
            "Use Agent",
            "Use SendMessage",
            "Use TaskStop",
            "task state",
            "missing evidence",
            "subscribe_pr_activity",
            "<task-notification>",
            "Scratchpad",
            "Verification",
            "Retry",
            "Cost limits",
            "Stopping conditions",
            "Maximum parallel workers: 4",
            "Retry failed worker tasks at most 1 time(s)",
            "Worker max turns: 12",
        ] {
            assert!(
                prompt.contains(required),
                "missing prompt clause: {required}"
            );
        }
        assert!(
            !prompt.contains("TaskList"),
            "coordinator prompt must not mention TaskList"
        );
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_run_policy_uses_defaults_and_rejects_invalid_env() {
        unsafe {
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS");
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK");
        }
        assert_eq!(
            CoordinatorRunPolicy::from_env(),
            CoordinatorRunPolicy {
                max_parallel_workers: 4,
                max_retry_per_task: 1,
                stop_after_verification: true,
            }
        );

        unsafe {
            std::env::set_var("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", "0");
            std::env::set_var("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK", "not-a-number");
        }
        assert_eq!(
            CoordinatorRunPolicy::from_env(),
            CoordinatorRunPolicy {
                max_parallel_workers: 4,
                max_retry_per_task: 1,
                stop_after_verification: true,
            }
        );

        unsafe {
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS");
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK");
        }
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_run_policy_reads_positive_env_values() {
        unsafe {
            std::env::set_var("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", "2");
            std::env::set_var("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK", "3");
        }

        assert_eq!(
            CoordinatorRunPolicy::from_env(),
            CoordinatorRunPolicy {
                max_parallel_workers: 2,
                max_retry_per_task: 3,
                stop_after_verification: true,
            }
        );

        unsafe {
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS");
            std::env::remove_var("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK");
        }
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
