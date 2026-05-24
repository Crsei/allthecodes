//! TeammateExecutor trait — execution backend abstraction.
//!
//! Corresponds to TypeScript: `utils/swarm/backends/types.ts`
//!
//! allthecodes intentionally supports only the in-process backend. The pane
//! backend traits remain as a typed boundary for parity review, but tmux and
//! iTerm2 are not runtime-selectable backends in this port.

use anyhow::{bail, Result};
use async_trait::async_trait;

use super::types::*;

// ---------------------------------------------------------------------------
// Backend strategy
// ---------------------------------------------------------------------------

/// Runtime-supported Agent Teams backends in allthecodes.
///
/// tmux/iTerm2 pane backends are intentionally not included for MVP-005; keep
/// the enum variants for persisted config compatibility and upstream parity
/// review, but reject them before execution.
pub const SUPPORTED_BACKENDS: &[BackendType] = &[BackendType::InProcess];

/// The default and only executable backend.
pub fn default_backend_type() -> BackendType {
    BackendType::InProcess
}

/// Return true when the backend can be executed by this binary.
pub fn is_backend_supported(backend_type: BackendType) -> bool {
    SUPPORTED_BACKENDS.contains(&backend_type)
}

/// Human-readable backend strategy for commands, tools, and diagnostics.
pub fn strategy_summary() -> &'static str {
    "Agent Teams backend strategy: in-process is the only supported backend; tmux/iTerm2 pane backends are intentionally not included in allthecodes."
}

/// Consistent message for unsupported pane backends.
pub fn unsupported_backend_message(backend_type: BackendType) -> String {
    format!(
        "Agent Teams backend '{}' is not supported by allthecodes. Supported backend: in-process. tmux/iTerm2 pane backends are an intentional scope choice documented in docs/IMPLEMENTATION_GAPS.md §7.",
        backend_type
    )
}

/// Fail fast when persisted config or tool input asks for an unsupported
/// backend.
pub fn ensure_backend_supported(backend_type: BackendType) -> Result<()> {
    if is_backend_supported(backend_type) {
        Ok(())
    } else {
        bail!("{}", unsupported_backend_message(backend_type))
    }
}

// ---------------------------------------------------------------------------
// TeammateExecutor trait
// ---------------------------------------------------------------------------

/// Trait for teammate execution backends.
///
/// Implementations handle spawning, messaging, and lifecycle management
/// of teammate agents.
#[async_trait]
pub trait TeammateExecutor: Send + Sync {
    /// The backend type identifier.
    fn backend_type(&self) -> BackendType;

    /// Whether this backend is available in the current environment.
    async fn is_available(&self) -> bool;

    /// Spawn a new teammate.
    async fn spawn(&self, config: TeammateSpawnConfig) -> Result<TeammateSpawnResult>;

    /// Send a message to a teammate's mailbox.
    async fn send_message(
        &self,
        agent_id: &str,
        team_name: &str,
        message: TeammateMessage,
    ) -> Result<()>;

    /// Request graceful termination (teammate can reject).
    ///
    /// Returns `true` if the shutdown request was sent successfully.
    async fn terminate(&self, agent_id: &str, team_name: &str, reason: Option<&str>) -> bool;

    /// Force-kill a teammate (immediate, no negotiation).
    async fn kill(&self, agent_id: &str) -> bool;

    /// Check if a teammate is still active.
    async fn is_active(&self, agent_id: &str) -> bool;
}

// ---------------------------------------------------------------------------
// PaneBackend trait (tmux / iTerm2 specialization)
// ---------------------------------------------------------------------------

/// Result of creating a new terminal pane.
#[derive(Debug)]
pub struct CreatePaneResult {
    pub pane_id: String,
    pub session_name: String,
}

/// Extended trait for pane-based backends (tmux, iTerm2).
#[async_trait]
pub trait PaneBackend: TeammateExecutor {
    /// Display name for the backend (e.g., "tmux", "iTerm2").
    fn display_name(&self) -> &str;

    /// Whether this backend supports hiding/showing panes.
    fn supports_hide_show(&self) -> bool;

    /// Check if we're running inside this backend's environment.
    async fn is_running_inside(&self) -> bool;

    /// Create a new pane for a teammate.
    async fn create_teammate_pane(&self, name: &str, color: &str) -> Result<CreatePaneResult>;

    /// Send a shell command to a pane.
    async fn send_command_to_pane(&self, pane_id: &str, command: &str) -> Result<()>;

    /// Set the border color of a pane.
    async fn set_pane_border_color(&self, pane_id: &str, color: &str) -> Result<()>;

    /// Set the title of a pane.
    async fn set_pane_title(&self, pane_id: &str, name: &str, color: &str) -> Result<()>;

    /// Rebalance pane layout after adding/removing panes.
    async fn rebalance_panes(&self, window_target: &str, has_leader: bool) -> Result<()>;

    /// Kill a specific pane.
    async fn kill_pane(&self, pane_id: &str) -> bool;

    /// Hide a pane (move to hidden session).
    async fn hide_pane(&self, pane_id: &str) -> bool;

    /// Show a previously hidden pane.
    async fn show_pane(&self, pane_id: &str, target: &str) -> bool;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_pane_result() {
        let result = CreatePaneResult {
            pane_id: "%3".into(),
            session_name: "allthecodes-swarm".into(),
        };
        assert_eq!(result.pane_id, "%3");
    }

    #[test]
    fn in_process_is_the_only_supported_backend() {
        assert_eq!(default_backend_type(), BackendType::InProcess);
        assert!(is_backend_supported(BackendType::InProcess));
        assert!(!is_backend_supported(BackendType::Tmux));
        assert!(!is_backend_supported(BackendType::ITerm2));
        assert!(ensure_backend_supported(BackendType::Tmux).is_err());
    }
}
