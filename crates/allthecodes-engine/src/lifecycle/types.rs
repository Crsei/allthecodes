//! Core types used across the QueryEngine lifecycle.

use std::time::Instant;

use crate::types::message::Usage;
use allthecodes_types::sdk::UsageTracking;

pub(crate) trait UsageTrackingExt {
    /// Accumulate a single API call's usage.
    ///
    /// Also syncs the cost to the global ProcessState for cross-module access.
    fn add_usage(&mut self, usage: &Usage, cost_usd: f64);
}

impl UsageTrackingExt for allthecodes_types::sdk::UsageTracking {
    fn add_usage(&mut self, usage: &Usage, cost_usd: f64) {
        *self = std::mem::take(self).with_added_usage(usage, cost_usd);
        // Sync to global ProcessState
        crate::bootstrap::PROCESS_STATE.write().total_cost_usd += cost_usd;
    }
}

fn total_usage_tokens(usage: &UsageTracking) -> u64 {
    usage
        .total_input_tokens
        .saturating_add(usage.total_output_tokens)
        .saturating_add(usage.total_cache_read_tokens)
        .saturating_add(usage.total_cache_creation_tokens)
}

/// Runtime-only state for durable goal accounting.
///
/// Goal records persist cumulative usage. This state keeps the live engine
/// baseline so a newly created goal does not inherit tokens used before it
/// existed, and each write can be guarded by the current goal id.
#[derive(Debug, Clone)]
pub(crate) struct GoalRuntimeState {
    pub(crate) active_goal_id: Option<String>,
    last_accounted_usage: UsageTracking,
    last_accounted_at: Option<Instant>,
    pub(crate) budget_warning_sent_for_goal_id: Option<String>,
    pub(crate) continuation_queued_for_goal_id: Option<String>,
}

impl GoalRuntimeState {
    pub(crate) fn prime_active_goal(&mut self, goal_id: &str, usage: &UsageTracking, now: Instant) {
        if self.active_goal_id.as_deref() != Some(goal_id) {
            self.active_goal_id = Some(goal_id.to_string());
            self.last_accounted_usage = usage.clone();
            self.last_accounted_at = Some(now);
            self.continuation_queued_for_goal_id = None;
        } else if self.last_accounted_at.is_none() {
            self.last_accounted_usage = usage.clone();
            self.last_accounted_at = Some(now);
        }
    }

    pub(crate) fn account_delta(
        &mut self,
        goal_id: &str,
        usage: &UsageTracking,
        now: Instant,
    ) -> Option<(u64, u64)> {
        if self.active_goal_id.as_deref() != Some(goal_id) {
            self.prime_active_goal(goal_id, usage, now);
            return None;
        }

        let previous_tokens = total_usage_tokens(&self.last_accounted_usage);
        let current_tokens = total_usage_tokens(usage);
        let token_delta = current_tokens.saturating_sub(previous_tokens);
        let seconds_delta = self
            .last_accounted_at
            .map(|last| now.saturating_duration_since(last).as_secs())
            .unwrap_or_default();

        self.last_accounted_usage = usage.clone();
        self.last_accounted_at = Some(now);

        Some((token_delta, seconds_delta))
    }

    pub(crate) fn account_explicit_token_delta(
        &mut self,
        goal_id: &str,
        usage: &UsageTracking,
        token_delta: u64,
        now: Instant,
    ) -> (u64, u64) {
        if self.active_goal_id.as_deref() != Some(goal_id) {
            self.prime_active_goal(goal_id, usage, now);
            self.last_accounted_usage = usage.clone();
            self.last_accounted_at = Some(now);
            return (token_delta, 0);
        }

        let seconds_delta = self
            .last_accounted_at
            .map(|last| now.saturating_duration_since(last).as_secs())
            .unwrap_or_default();
        self.last_accounted_usage = usage.clone();
        self.last_accounted_at = Some(now);
        (token_delta, seconds_delta)
    }

    pub(crate) fn clear_for_goal(&mut self, goal_id: &str) {
        if self.active_goal_id.as_deref() == Some(goal_id) {
            self.clear_active();
        }
        if self.continuation_queued_for_goal_id.as_deref() == Some(goal_id) {
            self.continuation_queued_for_goal_id = None;
        }
    }

    pub(crate) fn clear_active(&mut self) {
        self.active_goal_id = None;
        self.last_accounted_usage = UsageTracking::default();
        self.last_accounted_at = None;
        self.continuation_queued_for_goal_id = None;
    }

    pub(crate) fn budget_warning_already_sent(&self, goal_id: &str) -> bool {
        self.budget_warning_sent_for_goal_id.as_deref() == Some(goal_id)
    }

    pub(crate) fn mark_budget_warning_sent(&mut self, goal_id: &str) {
        self.budget_warning_sent_for_goal_id = Some(goal_id.to_string());
    }
}

impl Default for GoalRuntimeState {
    fn default() -> Self {
        Self {
            active_goal_id: None,
            last_accounted_usage: UsageTracking::default(),
            last_accounted_at: None,
            budget_warning_sent_for_goal_id: None,
            continuation_queued_for_goal_id: None,
        }
    }
}

/// Reason for aborting a query.
#[derive(Debug, Clone)]
pub enum AbortReason {
    /// User pressed Ctrl-C or called abort().
    UserAbort,
    /// Max budget exceeded.
    MaxBudget { spent_usd: f64, limit_usd: f64 },
    /// Max turns exceeded.
    MaxTurns { turns: usize, limit: usize },
    /// Unrecoverable API error.
    ApiError { message: String },
}
