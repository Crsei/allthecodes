use std::sync::Arc;

use tracing::warn;

use crate::types::state::QueryLoopState;
use crate::types::tool::PermissionMode;

use super::deps::QueryDeps;
use super::turn_context::QueryRunContext;

pub(crate) struct ActiveGoalContinuation {
    pub(crate) goal_id: String,
    pub(crate) message: String,
}

#[derive(Debug, Default)]
pub(crate) struct GoalContinuationScheduler {
    queued_for: Option<String>,
}

impl GoalContinuationScheduler {
    pub(crate) fn next_idle_continuation(
        &mut self,
        deps: &Arc<dyn QueryDeps>,
        context: &QueryRunContext,
        state: &QueryLoopState,
    ) -> Option<ActiveGoalContinuation> {
        if suppress_goal_continuation(deps, context, state) {
            self.queued_for = None;
            return None;
        }

        let goal = active_goal(deps)?;
        if self.queued_for.as_deref() == Some(goal.goal_id.as_str()) {
            return None;
        }

        self.queued_for = Some(goal.goal_id.clone());
        Some(ActiveGoalContinuation {
            goal_id: goal.goal_id.clone(),
            message: format!(
                "<goal_continuation goal_id=\"{}\">\n\
Continue working toward the active session goal:\n\n{}\n\n\
If the goal is complete, call UpdateGoal with status=complete. \
If progress is blocked by missing external input, call UpdateGoal with status=blocked.\n\
</goal_continuation>",
                goal.goal_id, goal.objective
            ),
        })
    }

    pub(crate) fn confirm_ready(&mut self, deps: &Arc<dyn QueryDeps>, goal_id: &str) -> bool {
        let ready =
            goal_is_still_active(deps, goal_id) && self.queued_for.as_deref() == Some(goal_id);
        if !ready {
            self.queued_for = None;
        }
        ready
    }

    pub(crate) fn mark_dispatched(&mut self, goal_id: &str) {
        if self.queued_for.as_deref() == Some(goal_id) {
            self.queued_for = None;
        }
    }
}

fn suppress_goal_continuation(
    deps: &Arc<dyn QueryDeps>,
    context: &QueryRunContext,
    state: &QueryLoopState,
) -> bool {
    if context.query_source.is_autonomous() || context.query_source.starts_with_agent() {
        return true;
    }
    if deps.get_app_state().tool_permission_context.mode == PermissionMode::Plan {
        return true;
    }
    state.pending_tool_use_summary.is_some() || state.stop_hook_active.unwrap_or(false)
}

fn active_goal(deps: &Arc<dyn QueryDeps>) -> Option<allthecodes_tools::goals::GoalRecord> {
    let session_id = deps.audit_context().session_id;
    let goal = match allthecodes_tools::goals::load_goal_for_session(&session_id) {
        Ok(Some(goal)) => goal,
        Ok(None) => return None,
        Err(error) => {
            warn!(%error, "failed to load active goal for continuation");
            return None;
        }
    };

    allthecodes_tools::goals::goal_is_active(&goal).then_some(goal)
}

fn goal_is_still_active(deps: &Arc<dyn QueryDeps>, goal_id: &str) -> bool {
    let session_id = deps.audit_context().session_id;
    match allthecodes_tools::goals::load_goal_for_session(&session_id) {
        Ok(Some(goal)) => {
            goal.goal_id == goal_id && allthecodes_tools::goals::goal_is_active(&goal)
        }
        Ok(None) => false,
        Err(error) => {
            warn!(%error, "failed to reload active goal before continuation");
            false
        }
    }
}
