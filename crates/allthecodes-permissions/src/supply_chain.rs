//! Static supply-chain checks for GitHub Actions and similar workflow files.

use std::fs;
use std::io::Read;
use std::path::Path;

use allthecodes_types::security::TaintDecisionKind;

use crate::setup_chain::{normalize_scanner_findings, ScannerFinding};

const MAX_WORKFLOW_BYTES: usize = 1024 * 1024;

/// Scan a workflow file without parsing or executing YAML expressions.
pub fn scan_github_workflow(path: impl AsRef<Path>) -> anyhow::Result<Vec<ScannerFinding>> {
    let path = path.as_ref();
    let file = fs::File::open(path).map_err(|error| {
        anyhow::anyhow!("failed to read workflow {}: {}", path.display(), error)
    })?;
    let mut content = Vec::with_capacity(MAX_WORKFLOW_BYTES.min(64 * 1024));
    file.take(MAX_WORKFLOW_BYTES as u64)
        .read_to_end(&mut content)
        .map_err(|error| {
            anyhow::anyhow!("failed to read workflow {}: {}", path.display(), error)
        })?;
    Ok(scan_github_workflow_content(
        &String::from_utf8_lossy(&content),
        Some(path.to_string_lossy().to_string()),
    ))
}

/// Content variant used by the security gate when a workflow is being written
/// before it exists on disk.
pub fn scan_github_workflow_content(
    content: &str,
    location: Option<String>,
) -> Vec<ScannerFinding> {
    let mut findings = Vec::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(uses) = lower.split_once("uses:").map(|(_, value)| value) {
            let reference = uses.trim().rsplit_once('@').map(|(_, r)| r).unwrap_or("");
            if !is_full_sha(reference) {
                findings.push(ScannerFinding::new(
                    "supply.action_floating_ref",
                    TaintDecisionKind::Ask,
                    "GitHub Action references a mutable tag or branch instead of a commit SHA",
                    Some(format_location(&location, line_number + 1)),
                ));
            }
        }

        if lower.contains("permissions: write-all")
            || lower.contains("contents: write")
            || lower.contains("actions: write")
            || lower.contains("pull-requests: write")
        {
            findings.push(ScannerFinding::new(
                "supply.runner_unrestricted_egress",
                TaintDecisionKind::Ask,
                "Workflow grants broad write capability to the runner",
                Some(format_location(&location, line_number + 1)),
            ));
        }

        if lower.starts_with("run:")
            && (lower.contains("curl ")
                || lower.contains("wget ")
                || lower.contains("invoke-webrequest"))
        {
            findings.push(ScannerFinding::new(
                "setup.dynamic_download",
                TaintDecisionKind::Ask,
                "Workflow execution performs a dynamic network download",
                Some(format_location(&location, line_number + 1)),
            ));
        }
    }
    normalize_scanner_findings(findings)
}

fn is_full_sha(reference: &str) -> bool {
    reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn format_location(location: &Option<String>, line: usize) -> String {
    location
        .as_deref()
        .map(|path| format!("{path}:{line}"))
        .unwrap_or_else(|| format!("workflow:{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn floating_action_ref_requires_approval() {
        let findings = scan_github_workflow_content(
            "jobs:\n  build:\n    steps:\n      - uses: owner/action@v4\n",
            None,
        );
        assert!(findings
            .iter()
            .any(|finding| finding.rule_id == "supply.action_floating_ref"));
    }

    #[test]
    fn pinned_action_ref_is_not_flagged() {
        let mut fixture = NamedTempFile::new().unwrap();
        writeln!(
            fixture,
            "jobs:\n  build:\n    steps:\n      - uses: owner/action@0123456789abcdef0123456789abcdef01234567"
        )
        .unwrap();
        let findings = scan_github_workflow(fixture.path()).unwrap();
        assert!(!findings
            .iter()
            .any(|finding| finding.rule_id == "supply.action_floating_ref"));
    }
}
