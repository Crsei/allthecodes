//! Runtime verification evidence and bounded verification policies.
//!
//! This module only classifies evidence already produced by the normal tool
//! pipeline. It never executes a verification command itself.

pub mod evidence;
pub mod policy;

pub use evidence::{
    classify_shell_evidence, evidence_from_tool_result, evidence_from_tool_result_with_duration,
};
pub use policy::{evaluate, policy_by_name, VerificationDecision, VerificationPolicy};
