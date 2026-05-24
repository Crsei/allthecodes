//! Teammate context isolation using `tokio::task_local!`.
//!
//! Corresponds to TypeScript: `utils/teammateContext.ts` (AsyncLocalStorage)
//!
//! Each in-process teammate runs inside a `TEAMMATE_CONTEXT.scope(...)` block,
//! making identity information available to all code within that scope without
//! threading it through every function signature.

use super::types::TeammateIdentity;

// ---------------------------------------------------------------------------
// task_local storage
// ---------------------------------------------------------------------------

tokio::task_local! {
    /// The identity of the currently-executing in-process teammate.
    static TEAMMATE_CONTEXT: TeammateIdentity;
}

// ---------------------------------------------------------------------------
// Public API — accessors (return None when outside a teammate scope)
// ---------------------------------------------------------------------------

/// Try to get the agent ID from the current task-local context.
pub fn try_get_agent_id() -> Option<String> {
    TEAMMATE_CONTEXT.try_with(|ctx| ctx.agent_id.clone()).ok()
}

/// Try to get the agent name from the current task-local context.
pub fn try_get_agent_name() -> Option<String> {
    TEAMMATE_CONTEXT.try_with(|ctx| ctx.agent_name.clone()).ok()
}

/// Try to get the team name from the current task-local context.
pub fn try_get_team_name() -> Option<String> {
    TEAMMATE_CONTEXT.try_with(|ctx| ctx.team_name.clone()).ok()
}

/// Try to get the teammate's assigned color.
pub fn try_get_color() -> Option<String> {
    TEAMMATE_CONTEXT
        .try_with(|ctx| ctx.color.clone())
        .ok()
        .flatten()
}

/// Try to get whether plan mode is required.
pub fn try_get_plan_mode_required() -> Option<bool> {
    TEAMMATE_CONTEXT.try_with(|ctx| ctx.plan_mode_required).ok()
}

/// Try to get the parent session ID.
pub fn try_get_parent_session_id() -> Option<String> {
    TEAMMATE_CONTEXT
        .try_with(|ctx| ctx.parent_session_id.clone())
        .ok()
}

/// Try to get the full identity struct.
pub fn try_get_identity() -> Option<TeammateIdentity> {
    TEAMMATE_CONTEXT.try_with(|ctx| ctx.clone()).ok()
}

// ---------------------------------------------------------------------------
// Scope runner
// ---------------------------------------------------------------------------

/// Run a future within a teammate context scope.
///
/// All `try_get_*` calls within `fut` will resolve from `identity`.
///
/// ```ignore
/// use crate::context;
/// context::run_in_scope(identity, async {
///     let id = context::try_get_agent_id(); // Some("researcher@team")
///     do_work().await;
/// }).await;
/// ```
pub async fn run_in_scope<F, R>(identity: TeammateIdentity, fut: F) -> R
where
    F: std::future::Future<Output = R>,
{
    TEAMMATE_CONTEXT.scope(identity, fut).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity() -> TeammateIdentity {
        TeammateIdentity {
            agent_id: "researcher@test-team".into(),
            agent_name: "researcher".into(),
            team_name: "test-team".into(),
            color: Some("blue".into()),
            plan_mode_required: false,
            parent_session_id: "sess-123".into(),
        }
    }

    #[test]
    fn test_outside_scope_returns_none() {
        assert!(try_get_agent_id().is_none());
        assert!(try_get_team_name().is_none());
        assert!(try_get_identity().is_none());
    }

    #[tokio::test]
    async fn test_inside_scope() {
        let id = test_identity();
        run_in_scope(id, async {
            assert_eq!(try_get_agent_id().unwrap(), "researcher@test-team");
            assert_eq!(try_get_agent_name().unwrap(), "researcher");
            assert_eq!(try_get_team_name().unwrap(), "test-team");
            assert_eq!(try_get_color().unwrap(), "blue");
            assert_eq!(try_get_plan_mode_required(), Some(false));
            assert_eq!(try_get_parent_session_id().unwrap(), "sess-123");

            let full = try_get_identity().unwrap();
            assert_eq!(full.agent_id, "researcher@test-team");
        })
        .await;
    }

    #[tokio::test]
    async fn test_scope_isolation() {
        let id = test_identity();
        run_in_scope(id, async {
            assert!(try_get_agent_id().is_some());
        })
        .await;
        // Outside scope again
        assert!(try_get_agent_id().is_none());
    }
}
