use std::sync::Arc;

use tracing::warn;

use crate::types::message::{AssistantMessage, ContentBlock};
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
    in_flight_for: Option<String>,
    empty_response_count: usize,
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
        self.in_flight_for = Some(goal_id.to_string());
    }

    pub(crate) fn observe_assistant_response(
        &mut self,
        assistant_message: &AssistantMessage,
    ) -> Option<String> {
        let goal_id = self.in_flight_for.take()?;
        if assistant_has_progress(assistant_message) {
            self.empty_response_count = 0;
            return None;
        }

        self.empty_response_count += 1;
        if self.empty_response_count >= 2 {
            self.clear();
            Some(goal_id)
        } else {
            None
        }
    }

    pub(crate) fn clear(&mut self) {
        self.queued_for = None;
        self.in_flight_for = None;
        self.empty_response_count = 0;
    }
}

fn assistant_has_progress(assistant_message: &AssistantMessage) -> bool {
    assistant_message.content.iter().any(|block| match block {
        ContentBlock::Text { text } => !text.trim().is_empty(),
        ContentBlock::Thinking { thinking, .. } => !thinking.trim().is_empty(),
        ContentBlock::ConnectorText { connector_text, .. } => !connector_text.trim().is_empty(),
        ContentBlock::ToolUse { .. } | ContentBlock::ServerToolUse { .. } => true,
        ContentBlock::ToolResult { .. } | ContentBlock::RedactedThinking { .. } => true,
        ContentBlock::Image { .. } => true,
    })
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

pub(crate) fn mark_active_goal_paused(deps: &Arc<dyn QueryDeps>, reason: impl Into<String>) {
    let session_id = deps.audit_context().session_id;
    let reason = reason.into();
    let Some(goal) = active_goal(deps) else {
        return;
    };
    if let Err(error) = allthecodes_tools::goals::mark_goal_paused_for_session(
        &session_id,
        Some(&goal.goal_id),
        reason,
    ) {
        warn!(%error, "failed to pause active goal");
    }
}

pub(crate) fn mark_active_goal_usage_limited(deps: &Arc<dyn QueryDeps>, reason: impl Into<String>) {
    let session_id = deps.audit_context().session_id;
    let reason = reason.into();
    let Some(goal) = active_goal(deps) else {
        return;
    };
    if let Err(error) = allthecodes_tools::goals::mark_goal_usage_limited_for_session(
        &session_id,
        Some(&goal.goal_id),
        reason,
    ) {
        warn!(%error, "failed to mark active goal usage-limited");
    }
}
