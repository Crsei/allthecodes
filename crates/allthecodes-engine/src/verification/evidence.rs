use allthecodes_permissions::command_risk::{
    classify_command_risk, CommandRiskConfidence, CommandRiskLevel, ShellKind,
};
use allthecodes_session::record_replay::types::{EvidenceKind, VerificationEvidenceRecord};
use allthecodes_types::agent_runtime_record::compute_digest;
use serde_json::Value;

use crate::types::tool::ToolResult;

/// Classify a successful shell command as positive verification evidence.
///
/// The command itself is never returned or persisted by this module. Unknown,
/// mutating, destructive, deploy, secret, low-confidence, interrupted, and
/// non-zero executions do not count as evidence.
pub fn classify_shell_evidence(command: &str, exit_code: i32) -> Option<EvidenceKind> {
    if exit_code != 0 {
        return None;
    }

    let normalized = normalize_command(command);
    if normalized.is_empty() {
        return None;
    }

    let risk = classify_command_risk(&normalized, ShellKind::Bash);
    if risk.confidence == CommandRiskConfidence::Low
        || risk.segments.len() != 1
        || matches!(
            risk.level,
            CommandRiskLevel::Mutate
                | CommandRiskLevel::Deploy
                | CommandRiskLevel::Destructive
                | CommandRiskLevel::Secret
        )
    {
        return None;
    }

    if is_test_command(&normalized) {
        return Some(EvidenceKind::Test);
    }
    if is_lint_command(&normalized) {
        return Some(EvidenceKind::Lint);
    }
    if is_security_command(&normalized) {
        return Some(EvidenceKind::SecurityGate);
    }
    if is_api_smoke_command(&normalized) {
        return Some(EvidenceKind::ApiSmoke);
    }
    if is_browser_smoke_command(&normalized) {
        return Some(EvidenceKind::BrowserSmoke);
    }

    (risk.level == CommandRiskLevel::Build && risk.confidence != CommandRiskConfidence::Low)
        .then_some(EvidenceKind::Build)
}

/// Convert a structured shell result into a canonical, metadata-only record.
pub fn evidence_from_tool_result(
    tool_use_id: &str,
    tool_name: &str,
    input: &Value,
    result: &ToolResult,
) -> Option<VerificationEvidenceRecord> {
    evidence_from_tool_result_with_duration(tool_use_id, tool_name, input, result, 0)
}

/// Variant used by the execution boundary when the duration is available
/// outside of `ToolResult`.
pub fn evidence_from_tool_result_with_duration(
    tool_use_id: &str,
    tool_name: &str,
    input: &Value,
    result: &ToolResult,
    duration_ms: u64,
) -> Option<VerificationEvidenceRecord> {
    let shell = result.shell.as_ref()?;
    if shell.interrupted || shell.error.is_some() {
        return None;
    }

    let command = shell
        .command
        .as_deref()
        .or_else(|| input_string_field(input, &["command", "cmd", "script"]))?;
    let exit_code = shell.exit_code?;
    let kind = classify_shell_evidence_for_tool(command, tool_name, exit_code)?;

    Some(VerificationEvidenceRecord {
        evidence_id: format!("evidence-{tool_use_id}"),
        kind,
        tool_use_id: tool_use_id.to_string(),
        command_digest: compute_digest(normalize_command(command).as_bytes()),
        exit_code,
        duration_ms,
        artifact_ids: vec![],
    })
}

fn classify_shell_evidence_for_tool(
    command: &str,
    tool_name: &str,
    exit_code: i32,
) -> Option<EvidenceKind> {
    if matches!(tool_name, "PowerShell" | "powershell" | "pwsh" | "Pwsh") {
        // The command vocabulary is shared for cargo/npm checks. For native
        // PowerShell commands, retain the same fail-closed classification and
        // only allow explicit known verification forms.
        if exit_code == 0 && is_test_command(&normalize_command(command)) {
            return Some(EvidenceKind::Test);
        }
    }
    classify_shell_evidence(command, exit_code)
}

fn normalize_command(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn input_string_field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| input.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn is_test_command(command: &str) -> bool {
    command == "cargo test"
        || command.starts_with("cargo test ")
        || command == "cargo nextest run"
        || command.starts_with("cargo nextest run ")
        || command.starts_with("npm test")
        || command.starts_with("npm run test")
        || command.starts_with("yarn test")
        || command.starts_with("pnpm test")
}

fn is_lint_command(command: &str) -> bool {
    command == "cargo fmt"
        || command.starts_with("cargo fmt ")
        || command == "cargo clippy"
        || command.starts_with("cargo clippy ")
        || command.starts_with("npm run lint")
        || command.starts_with("yarn lint")
        || command.starts_with("pnpm lint")
}

fn is_security_command(command: &str) -> bool {
    command.starts_with("cargo audit")
        || command.starts_with("cargo deny")
        || command.starts_with("npm audit")
        || command.starts_with("gitleaks ")
        || command.starts_with("trivy ")
        || command.starts_with("bandit ")
}

fn is_api_smoke_command(command: &str) -> bool {
    (command.starts_with("curl ") || command.starts_with("wget "))
        && ["/health", "/ready", "/status", "/api/"]
            .iter()
            .any(|path| command.contains(path))
}

fn is_browser_smoke_command(command: &str) -> bool {
    command.starts_with("playwright ")
        || command.starts_with("npx playwright ")
        || command.starts_with("npm run test:e2e")
        || command.starts_with("yarn test:e2e")
        || command.starts_with("pnpm test:e2e")
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::ShellExecutionOutput;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn classifies_known_successful_commands_and_rejects_failures() {
        for (command, exit, expected) in [
            ("cargo build --workspace", 0, Some(EvidenceKind::Build)),
            (
                "cargo test -p allthecodes-tools",
                0,
                Some(EvidenceKind::Test),
            ),
            ("cargo fmt --all --check", 0, Some(EvidenceKind::Lint)),
            ("rm -rf /tmp/example", 0, None),
            ("cargo test", 101, None),
            ("cargo test || true", 0, None),
            ("cargo test; rm -rf /tmp/example", 0, None),
        ] {
            assert_eq!(
                classify_shell_evidence(command, exit),
                expected,
                "{command}"
            );
        }
    }

    #[test]
    fn evidence_record_contains_only_digest_and_metadata() {
        let result = ToolResult {
            shell: Some(ShellExecutionOutput {
                command: Some("cargo test -p allthecodes-tools".into()),
                cwd: Some(PathBuf::from("/repo")),
                stdout: "token=do-not-persist".into(),
                stderr: String::new(),
                exit_code: Some(0),
                interrupted: false,
                termination: None,
                error: None,
            }),
            data: json!({"secret": "do-not-persist"}),
            ..Default::default()
        };

        let record = evidence_from_tool_result(
            "toolu-1",
            "Bash",
            &json!({"command": "cargo test -p allthecodes-tools"}),
            &result,
        )
        .unwrap();
        assert_eq!(record.kind, EvidenceKind::Test);
        assert_eq!(record.exit_code, 0);
        assert!(!record.command_digest.contains("cargo"));
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("do-not-persist"));
    }

    #[test]
    fn interrupted_and_secret_commands_never_count() {
        let result = ToolResult {
            shell: Some(ShellExecutionOutput {
                command: Some("cargo test".into()),
                cwd: None,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
                interrupted: true,
                termination: Some("timeout".into()),
                error: None,
            }),
            ..Default::default()
        };
        assert!(evidence_from_tool_result("toolu-1", "Bash", &json!({}), &result).is_none());
        assert_eq!(classify_shell_evidence("cat ~/.ssh/id_rsa", 0), None);
    }
}
