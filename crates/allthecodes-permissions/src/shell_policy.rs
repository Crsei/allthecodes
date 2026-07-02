use crate::command_risk::{classify_command_risk, CommandRisk, CommandRiskLevel, ShellKind};
use allthecodes_shell_command::ReadOnlyResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPolicyShell {
    Bash,
    PowerShell,
}

impl ShellPolicyShell {
    pub fn label(self) -> &'static str {
        match self {
            ShellPolicyShell::Bash => "bash",
            ShellPolicyShell::PowerShell => "powershell",
        }
    }

    fn risk_shell(self) -> ShellKind {
        match self {
            ShellPolicyShell::Bash => ShellKind::Bash,
            ShellPolicyShell::PowerShell => ShellKind::PowerShell,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPolicyKind {
    ReadOnly,
    Build,
    Mutate,
    Destructive,
    Deploy,
    Secret,
    ParserFailure,
}

impl ShellPolicyKind {
    pub fn label(self) -> &'static str {
        match self {
            ShellPolicyKind::ReadOnly => "read_only",
            ShellPolicyKind::Build => "build",
            ShellPolicyKind::Mutate => "mutate",
            ShellPolicyKind::Destructive => "destructive",
            ShellPolicyKind::Deploy => "deploy",
            ShellPolicyKind::Secret => "secret",
            ShellPolicyKind::ParserFailure => "parser_failure",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellPolicyDecision {
    pub decision_id: String,
    pub command: String,
    pub shell: ShellPolicyShell,
    pub kind: ShellPolicyKind,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub expected_side_effects: Vec<String>,
    pub audit_labels: Vec<String>,
    pub risk: CommandRisk,
}

pub fn decide_shell_policy(command: &str, shell: ShellPolicyShell) -> ShellPolicyDecision {
    let risk = classify_command_risk(command, shell.risk_shell());
    let kind = kind_from_risk(command, shell, &risk);
    ShellPolicyDecision {
        decision_id: decision_id(command, shell),
        command: command.to_string(),
        shell,
        kind,
        reason: risk.reason.clone(),
        matched_rule: risk.matched_rule.clone(),
        expected_side_effects: expected_side_effects(kind),
        audit_labels: audit_labels(kind, &risk),
        risk,
    }
}

fn kind_from_risk(command: &str, shell: ShellPolicyShell, risk: &CommandRisk) -> ShellPolicyKind {
    if risk.reason.contains("failed to parse") {
        return ShellPolicyKind::ParserFailure;
    }

    match risk.level {
        CommandRiskLevel::Read if strict_read_only(command, shell) => ShellPolicyKind::ReadOnly,
        CommandRiskLevel::Read => ShellPolicyKind::Mutate,
        CommandRiskLevel::Build => ShellPolicyKind::Build,
        CommandRiskLevel::Mutate => ShellPolicyKind::Mutate,
        CommandRiskLevel::Destructive => ShellPolicyKind::Destructive,
        CommandRiskLevel::Deploy => ShellPolicyKind::Deploy,
        CommandRiskLevel::Secret => ShellPolicyKind::Secret,
    }
}

fn strict_read_only(command: &str, shell: ShellPolicyShell) -> bool {
    match shell {
        ShellPolicyShell::Bash => matches!(
            crate::read_only_shell::is_read_only_bash_command(command),
            ReadOnlyResult::ReadOnly
        ),
        ShellPolicyShell::PowerShell => matches!(
            crate::read_only_shell::is_read_only_powershell_command(command),
            ReadOnlyResult::ReadOnly
        ),
    }
}

fn expected_side_effects(kind: ShellPolicyKind) -> Vec<String> {
    match kind {
        ShellPolicyKind::ReadOnly => vec!["none".to_string()],
        ShellPolicyKind::Build => vec!["build_artifacts".to_string()],
        ShellPolicyKind::Mutate => vec!["workspace_mutation".to_string()],
        ShellPolicyKind::Destructive => vec!["destructive_mutation".to_string()],
        ShellPolicyKind::Deploy => vec!["external_state_change".to_string()],
        ShellPolicyKind::Secret => vec!["secret_exposure".to_string()],
        ShellPolicyKind::ParserFailure => vec!["unknown".to_string()],
    }
}

fn audit_labels(kind: ShellPolicyKind, risk: &CommandRisk) -> Vec<String> {
    let mut labels = vec![kind.label().to_string()];
    if let Some(rule) = &risk.matched_rule {
        labels.push(rule.clone());
    }
    labels
}

fn decision_id(command: &str, shell: ShellPolicyShell) -> String {
    format!("shell-policy:{}:{:016x}", shell.label(), fnv1a(command))
}

fn fnv1a(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bash_policy_golden_cases() {
        let cases = [
            ("git status --short", ShellPolicyKind::ReadOnly),
            ("cargo test", ShellPolicyKind::Build),
            ("mkdir -p tmp/cache", ShellPolicyKind::Mutate),
            ("rm -rf target", ShellPolicyKind::Destructive),
            ("git push origin main", ShellPolicyKind::Deploy),
            ("cat ~/.ssh/id_rsa", ShellPolicyKind::Secret),
        ];

        for (command, expected) in cases {
            let decision = decide_shell_policy(command, ShellPolicyShell::Bash);

            assert_eq!(decision.kind, expected, "command: {command}");
            assert_eq!(decision.command, command);
            assert_eq!(decision.shell, ShellPolicyShell::Bash);
            assert!(decision.decision_id.starts_with("shell-policy:bash:"));
            assert_eq!(decision.audit_labels[0], expected.label());
        }
    }

    #[test]
    fn compound_shell_syntax_is_not_read_only_policy() {
        let decision = decide_shell_policy("git status && git diff", ShellPolicyShell::Bash);

        assert_eq!(decision.kind, ShellPolicyKind::Mutate);
        assert_eq!(decision.audit_labels[0], "mutate");
    }

    #[test]
    fn classifies_powershell_policy_golden_cases() {
        let decision = decide_shell_policy(
            "Remove-Item -Recurse -Force C:\\tmp",
            ShellPolicyShell::PowerShell,
        );

        assert_eq!(decision.kind, ShellPolicyKind::Destructive);
        assert!(decision.decision_id.starts_with("shell-policy:powershell:"));
        assert_eq!(decision.audit_labels[0], "destructive");
    }

    #[test]
    fn policy_decision_id_is_stable_for_same_input() {
        let first = decide_shell_policy("git status --short", ShellPolicyShell::Bash);
        let second = decide_shell_policy("git status --short", ShellPolicyShell::Bash);

        assert_eq!(first.decision_id, second.decision_id);
    }
}
