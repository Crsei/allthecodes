//! Core types used across the QueryEngine lifecycle.

use crate::types::message::Usage;

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
