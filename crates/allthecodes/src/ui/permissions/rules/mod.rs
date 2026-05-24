//! Permission rule management surfaces.
pub mod add_permission_rules;
pub mod add_workspace_directory;
pub mod permission_rule_description;
pub mod permission_rule_input;
pub mod permission_rule_list;
pub mod recent_denials_tab;
pub mod remove_workspace_directory;
pub mod workspace_tab;

use crate::ui::permissions::utils::{PermissionDecision, PermissionScope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    pub pattern: String,
    pub decision: PermissionDecision,
    pub scope: PermissionScope,
    pub source: String,
}

impl PermissionRule {
    pub fn new(
        pattern: impl Into<String>,
        decision: PermissionDecision,
        scope: PermissionScope,
        source: impl Into<String>,
    ) -> Self {
        Self {
            pattern: pattern.into(),
            decision,
            scope,
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDirectory {
    pub path: String,
    pub trusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDenial {
    pub tool_name: String,
    pub pattern: String,
    pub reason: String,
}
