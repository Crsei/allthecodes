//! Agent Teams constants.
//!
//! Corresponds to TypeScript: `utils/swarm/constants.ts`

// ---------------------------------------------------------------------------
// Identity constants
// ---------------------------------------------------------------------------

/// The name used for the team leader agent.
pub const TEAM_LEAD_NAME: &str = "team-lead";

/// Tmux session name for swarm windows.
pub const SWARM_SESSION_NAME: &str = "allthecodes-swarm";

/// Tmux window name for the swarm view.
pub const SWARM_VIEW_WINDOW_NAME: &str = "swarm-view";

/// Tmux command name.
pub const TMUX_COMMAND: &str = "tmux";

/// Hidden tmux session name (for background panes).
pub const HIDDEN_SESSION_NAME: &str = "allthecodes-hidden";

// ---------------------------------------------------------------------------
// Environment variables
// ---------------------------------------------------------------------------

/// Env var overriding the teammate launch command.
pub const TEAMMATE_COMMAND_ENV_VAR: &str = "ALLTHECODES_TEAMMATE_COMMAND";

/// Env var setting the teammate's UI color.
pub const TEAMMATE_COLOR_ENV_VAR: &str = "ALLTHECODES_AGENT_COLOR";

/// Env var requiring plan mode for a teammate.
pub const PLAN_MODE_REQUIRED_ENV_VAR: &str = "ALLTHECODES_PLAN_MODE_REQUIRED";

/// Env var enabling experimental agent teams.
pub const AGENT_TEAMS_ENV_VAR: &str = "ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS";

// ---------------------------------------------------------------------------
// Mailbox configuration
// ---------------------------------------------------------------------------

/// Max lock acquisition retries for mailbox file lock.
pub const MAILBOX_LOCK_RETRIES: usize = 10;

/// Minimum backoff delay (ms) between lock retries.
pub const MAILBOX_LOCK_MIN_TIMEOUT_MS: u64 = 5;

/// Maximum backoff delay (ms) between lock retries.
pub const MAILBOX_LOCK_MAX_TIMEOUT_MS: u64 = 100;

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// Delay (ms) waiting for shell to initialize in a new pane.
pub const PANE_SHELL_INIT_DELAY_MS: u64 = 200;

/// Polling interval (ms) for checking mailbox messages.
pub const MAILBOX_POLL_INTERVAL_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// Maximum number of distinct teammate colors.
pub const MAX_TEAMMATE_COLORS: usize = 8;

/// Available teammate UI colors (round-robin assigned).
pub const AGENT_COLORS: &[&str] = &[
    "red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan",
];

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Subdirectory under the resolved data root for team data.
pub const TEAMS_DIR_NAME: &str = "teams";

/// Subdirectory under the resolved data root for team task lists.
pub const TASKS_DIR_NAME: &str = "tasks";

/// Filename for the team configuration file.
pub const TEAM_CONFIG_FILENAME: &str = "config.json";

/// Subdirectory within a team dir for agent inboxes.
pub const INBOXES_DIR_NAME: &str = "inboxes";

/// Inbox file extension.
pub const INBOX_EXTENSION: &str = "json";

/// Lock file suffix appended to inbox files.
pub const LOCK_FILE_SUFFIX: &str = ".lock";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_lead_name() {
        assert_eq!(TEAM_LEAD_NAME, "team-lead");
    }

    #[test]
    fn test_agent_colors_count() {
        assert_eq!(AGENT_COLORS.len(), MAX_TEAMMATE_COLORS);
    }

    #[test]
    fn test_env_var_names() {
        assert!(AGENT_TEAMS_ENV_VAR.starts_with("ALLTHECODES_"));
        assert!(TEAMMATE_COLOR_ENV_VAR.starts_with("ALLTHECODES_"));
        assert!(PLAN_MODE_REQUIRED_ENV_VAR.starts_with("ALLTHECODES_"));
        assert!(TEAMMATE_COMMAND_ENV_VAR.starts_with("ALLTHECODES_"));
    }
}
