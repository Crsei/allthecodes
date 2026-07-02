//! Dynamic workflow — JSON DAG-based multi-agent orchestration tool.
//!
//! Phase 1: model-defined JSON DAG with fan-out map/reduce support.
//! The model provides a static DAG of stages; the executor resolves dependencies
//! layer-by-layer, runs ready stages concurrently (bounded by semaphore), and
//! collects results for downstream reduce stages.
//!
//! Phase 2 (future): pipeline (no-barrier) stages + dynamic map items resolved
//! from previous stage outputs.

mod context;
mod definition;
mod executor;

pub use context::{WorkflowContext, WorkflowExecutionReport};
pub use definition::{DynamicWorkflowAction, DynamicWorkflowObservation, DynamicWorkflowTool};
pub use executor::{validate_workflow_plan, WorkflowPlan};

use crate::tool::Tools;
use std::sync::Arc;

pub fn tools() -> Tools {
    vec![Arc::new(DynamicWorkflowTool)]
}
