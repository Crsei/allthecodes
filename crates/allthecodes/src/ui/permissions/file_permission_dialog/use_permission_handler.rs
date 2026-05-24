//! File permission decision helpers.

use crate::ui::permissions::utils::PermissionDecision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePermissionDecision {
    pub decision: PermissionDecision,
    pub path: String,
    pub save_rule: bool,
}

pub fn describe_file_permission_decision(decision: &FilePermissionDecision) -> String {
    format!(
        "{} {} (save rule: {})",
        decision.decision.label(),
        decision.path,
        decision.save_rule
    )
}
