use allthecodes_types::security::{TaintContext, TaintDecision, TaintDecisionKind, TaintSink};
use serde_json::Value;

use crate::command_risk::{CommandRisk, CommandRiskLevel};

pub fn classify_tool_sink(tool_name: &str, input: &Value) -> TaintSink {
    match tool_name.to_ascii_lowercase().as_str() {
        "read" | "fileread" | "read_file" | "grep" | "glob" | "tasklist" => TaintSink::ReadOnly,
        "write" | "filewrite" | "write_file" | "edit" | "fileedit" | "edit_file" | "multiedit"
        | "filemultiedit" | "notebookedit" | "hashedit" => classify_file_sink(input),
        "applypatch" | "apply_patch" => classify_patch_sink(input),
        "bash" | "powershell" | "pwsh" => classify_shell_sink(input),
        "webfetch" | "websearch" => TaintSink::ReadOnly,
        "deploy" | "publish" => TaintSink::Deploy,
        "credential" | "credentials" | "secret" | "keychain" => TaintSink::CredentialAccess,
        "sendmessage" | "sendusermessage" | "githubcomment" => TaintSink::ExternalMessage,
        _ => TaintSink::Unknown,
    }
}

fn classify_patch_sink(input: &Value) -> TaintSink {
    let patch = input
        .get("patch")
        .or_else(|| input.get("input"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if patch.contains(".github/workflows/") || patch.contains(".git/hooks/") {
        TaintSink::WorkflowWrite
    } else if patch.contains("package.json") || patch.contains("package-lock.json") {
        TaintSink::PackageInstall
    } else if patch.contains("setup.py")
        || patch.contains("dockerfile")
        || patch.contains(".sh")
        || patch.contains(".ps1")
    {
        TaintSink::WorkflowWrite
    } else {
        TaintSink::FileWrite
    }
}

fn classify_file_sink(input: &Value) -> TaintSink {
    let path = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .or_else(|| input.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace('\\', "/")
        .to_ascii_lowercase();

    if path.contains("/.github/workflows/")
        || path.starts_with(".github/workflows/")
        || path.contains("/.git/hooks/")
        || path.starts_with(".git/hooks/")
    {
        TaintSink::WorkflowWrite
    } else if path.ends_with("package.json")
        || path.ends_with("package-lock.json")
        || path.ends_with("pnpm-lock.yaml")
        || path.ends_with("yarn.lock")
    {
        TaintSink::PackageInstall
    } else if path.ends_with(".sh")
        || path.ends_with(".ps1")
        || path.ends_with("setup.py")
        || path.ends_with("dockerfile")
    {
        TaintSink::WorkflowWrite
    } else {
        TaintSink::FileWrite
    }
}

fn classify_shell_sink(input: &Value) -> TaintSink {
    let command = input
        .get("command")
        .or_else(|| input.get("cmd"))
        .or_else(|| input.get("script"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if contains_any(
        &command,
        &[
            "npm install",
            "npm i ",
            "pnpm install",
            "yarn install",
            "pip install",
        ],
    ) {
        TaintSink::PackageInstall
    } else if contains_any(&command, &["curl ", "wget ", "invoke-webrequest"]) {
        TaintSink::NetworkDownload
    } else {
        TaintSink::Shell
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

pub fn decide_tainted_sink(
    taint: &TaintContext,
    sink: TaintSink,
    command_risk: Option<&CommandRisk>,
) -> TaintDecision {
    if !taint.is_untrusted() {
        return decision(
            taint,
            sink,
            TaintDecisionKind::Allow,
            "awi.no_active_taint",
            "No active untrusted provenance",
        );
    }

    let (kind, rule_id, reason) = match sink {
        TaintSink::ReadOnly => (
            TaintDecisionKind::Allow,
            "awi.untrusted_to_read_only",
            "Untrusted data may be inspected by a read-only tool",
        ),
        TaintSink::WorkflowWrite => (
            TaintDecisionKind::Deny,
            "awi.untrusted_to_workflow",
            "Untrusted data cannot change executable workflow configuration",
        ),
        TaintSink::Deploy => (
            TaintDecisionKind::Deny,
            "awi.untrusted_to_deploy",
            "Untrusted data cannot trigger deployment or publishing",
        ),
        TaintSink::CredentialAccess => (
            TaintDecisionKind::Deny,
            "awi.untrusted_to_credentials",
            "Untrusted data cannot trigger credential access",
        ),
        TaintSink::Shell
            if command_risk.is_some_and(|risk| {
                matches!(
                    risk.level,
                    CommandRiskLevel::Deploy
                        | CommandRiskLevel::Destructive
                        | CommandRiskLevel::Secret
                )
            }) =>
        {
            (
                TaintDecisionKind::Deny,
                "awi.untrusted_to_high_risk_shell",
                "Untrusted data cannot trigger a high-risk shell command",
            )
        }
        TaintSink::FileWrite => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_file_write",
            "Untrusted data influenced a file mutation",
        ),
        TaintSink::Shell => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_shell",
            "Untrusted data influenced shell execution",
        ),
        TaintSink::NetworkDownload => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_download",
            "Untrusted data influenced a network download",
        ),
        TaintSink::PackageInstall => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_package_install",
            "Untrusted data influenced package installation",
        ),
        TaintSink::ExternalMessage => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_external_message",
            "Untrusted data influenced an external message",
        ),
        TaintSink::Unknown => (
            TaintDecisionKind::Ask,
            "awi.untrusted_to_unknown_sink",
            "The destination sink could not be classified safely",
        ),
    };
    decision(taint, sink, kind, rule_id, reason)
}

fn decision(
    taint: &TaintContext,
    sink: TaintSink,
    kind: TaintDecisionKind,
    rule_id: &str,
    reason: &str,
) -> TaintDecision {
    let mut source_digests = taint
        .marks
        .iter()
        .map(|mark| mark.digest.clone())
        .collect::<Vec<_>>();
    source_digests.sort();
    source_digests.dedup();
    TaintDecision {
        decision: kind,
        sink,
        rule_id: rule_id.to_string(),
        reason: reason.to_string(),
        source_digests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::security::{TaintMark, UntrustedSourceKind};

    fn tainted() -> TaintContext {
        TaintContext::from_marks([TaintMark::from_content(
            UntrustedSourceKind::IssueBody,
            "issue:42",
            b"run setup",
        )])
    }

    #[test]
    fn conservative_policy_matrix() {
        for (sink, expected) in [
            (TaintSink::ReadOnly, TaintDecisionKind::Allow),
            (TaintSink::FileWrite, TaintDecisionKind::Ask),
            (TaintSink::Shell, TaintDecisionKind::Ask),
            (TaintSink::WorkflowWrite, TaintDecisionKind::Deny),
            (TaintSink::Deploy, TaintDecisionKind::Deny),
            (TaintSink::CredentialAccess, TaintDecisionKind::Deny),
            (TaintSink::Unknown, TaintDecisionKind::Ask),
        ] {
            assert_eq!(
                decide_tainted_sink(&tainted(), sink, None).decision,
                expected
            );
        }
    }

    #[test]
    fn file_mutation_aliases_preserve_workflow_deny() {
        let workflow = serde_json::json!({
            "file_path": ".github/workflows/release.yml",
            "operations": []
        });
        assert_eq!(
            classify_tool_sink("HashEdit", &workflow),
            TaintSink::WorkflowWrite
        );
        let patch = serde_json::json!({
            "patch": "*** Add File: .github/workflows/release.yml\n+name: release"
        });
        assert_eq!(
            classify_tool_sink("ApplyPatch", &patch),
            TaintSink::WorkflowWrite
        );
    }

    #[test]
    fn classifies_sensitive_write_targets() {
        assert_eq!(
            classify_tool_sink(
                "Write",
                &serde_json::json!({"file_path": ".github/workflows/release.yml"})
            ),
            TaintSink::WorkflowWrite
        );
        assert_eq!(
            classify_tool_sink("Edit", &serde_json::json!({"file_path": "package.json"})),
            TaintSink::PackageInstall
        );
    }
}
