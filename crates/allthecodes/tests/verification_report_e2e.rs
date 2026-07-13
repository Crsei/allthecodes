//! Deterministic acceptance coverage for verification evidence and reports.
//! These tests use the canonical record/projection contracts and never invoke
//! a shell, network, or model provider.

use allthecodes_engine::types::tool::ToolResult;
use allthecodes_engine::verification::{classify_shell_evidence, evaluate, policy_by_name};
use allthecodes_session::record_replay::types::{
    EvidenceKind, RecordItem, RecordLine, SessionReportGeneratedRecord, VerificationEvidenceRecord,
    VerificationStartedRecord,
};
use allthecodes_session::session_report::{
    project_session_report, record_head_digest, verify_session_report_file,
    write_session_report_to_path, SessionCostReportSummary, SessionReportOptions,
    SESSION_REPORT_FILE_NAME,
};
use allthecodes_types::ShellExecutionOutput;
use serde_json::json;
use sha2::Digest;

fn line(seq: u64, item: RecordItem) -> RecordLine {
    RecordLine::new("verification-e2e", seq, item)
}

#[test]
fn failed_test_retry_then_pass_is_recorded_as_positive_evidence() {
    let policy = policy_by_name("targeted_tests").unwrap();
    let command = "cargo test -p allthecodes-tools";
    assert_eq!(classify_shell_evidence(command, 101), None);
    let kind = classify_shell_evidence(command, 0);
    assert_eq!(kind, Some(EvidenceKind::Test));

    let evidence = VerificationEvidenceRecord {
        evidence_id: "evidence-retry".into(),
        kind: kind.unwrap(),
        tool_use_id: "tool-retry".into(),
        command_digest: "digest-only".into(),
        exit_code: 0,
        duration_ms: 42,
        artifact_ids: vec![],
    };
    assert!(matches!(
        evaluate(&policy, &[evidence], 2, &["exit=101".into()]),
        allthecodes_engine::verification::VerificationDecision::Passed { .. }
    ));
}

#[test]
fn compound_or_masked_commands_never_count_as_evidence() {
    assert_eq!(classify_shell_evidence("cargo test || true", 0), None);
    assert_eq!(
        classify_shell_evidence("cargo test; rm -rf /tmp/example", 0),
        None
    );
}

#[test]
fn missing_evidence_ends_incomplete_after_three_rounds() {
    let policy = policy_by_name("targeted_tests").unwrap();
    assert!(matches!(
        evaluate(&policy, &[], 1, &[]),
        allthecodes_engine::verification::VerificationDecision::Continue { round: 2, .. }
    ));
    assert!(matches!(
        evaluate(&policy, &[], 2, &[]),
        allthecodes_engine::verification::VerificationDecision::Continue { round: 3, .. }
    ));
    assert!(matches!(
        evaluate(&policy, &[], 3, &[]),
        allthecodes_engine::verification::VerificationDecision::Incomplete { round: 3, .. }
    ));
}

#[test]
fn cancelled_shell_execution_never_counts_as_evidence() {
    let result = ToolResult {
        shell: Some(ShellExecutionOutput {
            command: Some("cargo test".into()),
            cwd: None,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            interrupted: true,
            termination: None,
            error: None,
        }),
        ..Default::default()
    };
    assert!(allthecodes_engine::verification::evidence_from_tool_result(
        "tool-cancelled",
        "Bash",
        &json!({"command":"cargo test"}),
        &result,
    )
    .is_none());
}

#[test]
fn report_is_redacted_and_projects_existing_cost_summary() {
    let lines = vec![
        line(
            0,
            RecordItem::VerificationStarted(VerificationStartedRecord {
                verification_id: "verify-1".into(),
                policy: "targeted_tests".into(),
                round: 1,
            }),
        ),
        line(
            1,
            RecordItem::VerificationEvidence(VerificationEvidenceRecord {
                evidence_id: "evidence-1".into(),
                kind: EvidenceKind::Test,
                tool_use_id: "tool-1".into(),
                command_digest: "sha256:digest".into(),
                exit_code: 0,
                duration_ms: 10,
                artifact_ids: vec![],
            }),
        ),
    ];
    let report = project_session_report(
        "verification-e2e",
        &lines,
        "test",
        SessionReportOptions {
            changed_files: vec!["src/lib.rs".into(), "/home/private/secret.rs".into()],
            cost: Some(SessionCostReportSummary {
                total_tokens: 10,
                cache_tokens: 2,
                reasoning_tokens: 1,
                cost_usd: 0.012,
                api_calls: 1,
                unknown_pricing_count: 0,
                backfilled_count: 0,
            }),
            ..Default::default()
        },
    );
    let encoded = serde_json::to_string(&report).unwrap();
    assert_eq!(report.changed_files, vec!["src/lib.rs"]);
    assert!(encoded.contains("sha256:digest"));
    assert!(!encoded.contains("/home/private"));
    assert!(!encoded.contains("sk-ant-"));
    assert_eq!(report.cost.as_ref().unwrap().cost_usd, 0.012);
}

#[test]
fn report_hash_mismatch_is_detected() {
    let temp = tempfile::tempdir().unwrap();
    let base = vec![line(
        0,
        RecordItem::VerificationStarted(VerificationStartedRecord {
            verification_id: "verify-1".into(),
            policy: "targeted_tests".into(),
            round: 1,
        }),
    )];
    let report = project_session_report("verification-e2e", &base, "test", Default::default());
    let path = temp.path().join(SESSION_REPORT_FILE_NAME);
    write_session_report_to_path(&report, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut linked = base.clone();
    linked.push(line(
        1,
        RecordItem::SessionReportGenerated(SessionReportGeneratedRecord {
            report_version: 1,
            relative_path: SESSION_REPORT_FILE_NAME.into(),
            digest: format!("{:x}", sha2::Sha256::digest(&bytes)),
            record_head_digest: record_head_digest(&base),
        }),
    ));
    assert!(verify_session_report_file(&path, &linked).unwrap());

    linked.insert(
        1,
        line(
            99,
            RecordItem::ArtifactCreated(
                allthecodes_session::record_replay::types::ArtifactCreatedRecord {
                    artifact_id: "artifact-1".into(),
                    kind: "test".into(),
                    media_type: "text/plain".into(),
                    digest: "digest".into(),
                    byte_len: 0,
                    redaction: "metadata".into(),
                    relative_path: "artifacts/test.txt".into(),
                },
            ),
        ),
    );
    assert!(!verify_session_report_file(&path, &linked).unwrap());
}
