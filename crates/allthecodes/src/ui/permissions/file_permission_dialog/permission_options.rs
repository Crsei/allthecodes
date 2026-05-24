//! File permission option builders.

use crate::ui::permissions::utils::{PermissionDecision, PermissionOption, PermissionScope};

pub fn file_permission_options(path: &str, persistent: bool) -> Vec<PermissionOption> {
    let scope = if persistent {
        PermissionScope::Project
    } else {
        PermissionScope::Request
    };
    vec![
        PermissionOption::new(
            "Allow edit",
            format!("permit edit to {path}"),
            PermissionDecision::Allow,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Deny edit",
            "leave file unchanged",
            PermissionDecision::Deny,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Always allow path",
            format!("save rule for {path}"),
            PermissionDecision::AlwaysAllow,
            scope,
        ),
    ]
}
