//! Shell permission classification and preview helpers.

use super::utils::{
    command_preview, shell_risk_hint, PermissionDecision, PermissionOption, PermissionScope,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    PowerShell,
}

impl ShellKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }
}

pub fn shell_permission_options(command: &str) -> Vec<PermissionOption> {
    let risk = shell_risk_hint(command);
    vec![
        PermissionOption::new(
            "Allow",
            format!("run {risk} once"),
            PermissionDecision::Allow,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Deny",
            "do not run command",
            PermissionDecision::Deny,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Always allow exact command",
            "save exact command rule",
            PermissionDecision::AlwaysAllow,
            PermissionScope::Project,
        ),
    ]
}

pub fn render_shell_permission_details(kind: ShellKind, command: &str) -> String {
    format!(
        "shell: {}\ncommand: {}\nrisk: {}",
        kind.label(),
        command_preview(command),
        shell_risk_hint(command)
    )
}
