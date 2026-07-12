use std::path::PathBuf;

use allthecodes_permissions::setup_chain::scan_setup_chain;
use allthecodes_permissions::supply_chain::scan_github_workflow;
use allthecodes_types::security::TaintDecisionKind;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/setup_chain")
}

#[test]
fn malicious_setup_fixture_is_denied_without_execution() {
    let findings = scan_setup_chain(fixture_root(), "python setup.py").unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.rule_id == "setup.dns_txt_payload"));
    assert!(findings
        .iter()
        .any(|finding| finding.rule_id == "setup.hidden_postinstall_download"));
    assert!(findings
        .iter()
        .any(|finding| finding.decision == TaintDecisionKind::Deny));
}

#[test]
fn pinned_workflow_has_no_floating_action_finding() {
    let root = fixture_root();
    let floating = scan_github_workflow(root.join("floating.yml")).unwrap();
    let pinned = scan_github_workflow(root.join("pinned.yml")).unwrap();
    assert!(floating
        .iter()
        .any(|finding| finding.rule_id == "supply.action_floating_ref"));
    assert!(!pinned
        .iter()
        .any(|finding| finding.rule_id == "supply.action_floating_ref"));
}
