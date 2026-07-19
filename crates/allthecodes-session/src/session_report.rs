//! Redacted, versioned projection of canonical session evidence.
//!
//! The report is a view of record/replay facts. It is not a second transcript
//! and never contains prompts, raw tool output, credentials, or environment
//! values.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::record_replay::types::{
    EvidenceKind, RecordItem, RecordLine, RecordedMessage, VerificationStatus,
};
use crate::record_replay::SessionRecorderHandle;
use allthecodes_types::message::Attachment;

pub const SESSION_REPORT_SCHEMA_VERSION: u32 = 1;
pub const SESSION_REPORT_FILE_NAME: &str = "session-report.v1.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReportV1 {
    pub schema_version: u32,
    pub generated_by: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub human_initiator: Option<String>,
    pub goal_id: Option<String>,
    pub agent_role: Option<String>,
    pub model: Option<String>,
    pub summary: String,
    pub changed_files: Vec<String>,
    pub commands: Vec<CommandEvidenceSummary>,
    pub verification: VerificationSummary,
    pub security_gate_passed: Option<bool>,
    pub risk_events: Vec<RiskEventSummary>,
    pub unverified_assumptions: Vec<String>,
    pub cost: Option<SessionCostReportSummary>,
    #[serde(default)]
    pub recovery: RecoverySummary,
    pub human_review_required: bool,
    pub reviewed_by_human: bool,
    pub record_head_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEvidenceSummary {
    pub tool_use_id: String,
    pub risk: String,
    pub command_digest: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub policy: String,
    pub status: VerificationStatus,
    pub rounds: u8,
    pub evidence_ids: Vec<String>,
    pub missing_requirements: Vec<EvidenceKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEventSummary {
    pub rule_id: String,
    pub decision: String,
    pub payload_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCostReportSummary {
    pub total_tokens: u64,
    pub cache_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub api_calls: u64,
    pub unknown_pricing_count: u64,
    pub backfilled_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoverySummary {
    pub request_retries: u64,
    pub stream_retries: u64,
    pub model_fallbacks: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionReportOptions {
    pub human_initiator: Option<String>,
    pub goal_id: Option<String>,
    pub agent_role: Option<String>,
    pub changed_files: Vec<String>,
    pub cost: Option<SessionCostReportSummary>,
}

pub fn project_session_report(
    session_id: &str,
    lines: &[RecordLine],
    generated_by: &str,
    options: SessionReportOptions,
) -> SessionReportV1 {
    let metadata = lines.iter().find_map(|line| match &line.item {
        RecordItem::SessionMeta(metadata) => Some(metadata),
        _ => None,
    });
    let mut policy = None;
    let mut rounds = 0;
    let mut status = None;
    let mut evidence_ids = BTreeSet::new();
    let mut missing_requirements = Vec::new();
    let mut unverified_assumptions = BTreeSet::new();
    let mut commands = Vec::new();
    let mut risk_events = Vec::new();
    let mut security_decisions = Vec::new();
    let mut has_security_evidence = false;
    let mut recorded_changed_files = Vec::new();
    let mut recovery = RecoverySummary::default();

    for line in lines {
        match &line.item {
            RecordItem::VerificationStarted(record) => {
                policy = Some(record.policy.clone());
                rounds = rounds.max(record.round);
            }
            RecordItem::VerificationEvidence(record) => {
                has_security_evidence |= record.kind == EvidenceKind::SecurityGate;
                evidence_ids.insert(record.evidence_id.clone());
                commands.push(CommandEvidenceSummary {
                    tool_use_id: record.tool_use_id.clone(),
                    risk: evidence_risk(&record.kind).to_string(),
                    command_digest: record.command_digest.clone(),
                    exit_code: record.exit_code,
                    duration_ms: record.duration_ms,
                });
            }
            RecordItem::VerificationFinished(record) => {
                policy = Some(record.policy.clone());
                rounds = rounds.max(record.round);
                status = Some(record.status.clone());
                missing_requirements = record.missing_requirements.clone();
                evidence_ids.extend(record.evidence_ids.iter().cloned());
                unverified_assumptions.extend(record.unverified_assumptions.iter().cloned());
            }
            RecordItem::SecurityDecision(record) => {
                security_decisions.push(record.decision);
                for rule_id in &record.rule_ids {
                    risk_events.push(RiskEventSummary {
                        rule_id: rule_id.clone(),
                        decision: serde_json::to_string(&record.decision)
                            .unwrap_or_else(|_| "unknown".into())
                            .trim_matches('"')
                            .to_string(),
                        payload_digest: record.input_digest.clone(),
                    });
                }
            }
            RecordItem::Message(record) => {
                if let RecordedMessage::Attachment {
                    attachment: Attachment::EditedTextFile { path },
                    ..
                } = &record.message
                {
                    recorded_changed_files.push(path.clone());
                }
            }
            RecordItem::QueryEvent(
                crate::record_replay::types::QueryEventRecord::RequestStart {
                    is_retry: true,
                    retry_phase,
                    ..
                },
            ) => match retry_phase.as_deref() {
                Some("request") => recovery.request_retries += 1,
                Some("stream") => recovery.stream_retries += 1,
                Some("fallback") => recovery.model_fallbacks += 1,
                _ => {}
            },
            _ => {}
        }
    }

    commands.sort_by(|a, b| {
        (&a.tool_use_id, &a.command_digest, a.exit_code).cmp(&(
            &b.tool_use_id,
            &b.command_digest,
            b.exit_code,
        ))
    });
    commands.dedup_by(|a, b| {
        a.tool_use_id == b.tool_use_id
            && a.command_digest == b.command_digest
            && a.exit_code == b.exit_code
    });
    risk_events.sort_by(|a, b| {
        (&a.rule_id, &a.payload_digest, &a.decision).cmp(&(
            &b.rule_id,
            &b.payload_digest,
            &b.decision,
        ))
    });
    risk_events.dedup_by(|a, b| {
        a.rule_id == b.rule_id && a.payload_digest == b.payload_digest && a.decision == b.decision
    });

    let policy = policy.unwrap_or_else(|| "none".into());
    let status = status.unwrap_or_else(|| {
        if policy == "none" {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Incomplete
        }
    });
    let security_gate_passed = if security_decisions
        .iter()
        .any(|decision| *decision != allthecodes_types::security::TaintDecisionKind::Allow)
    {
        Some(false)
    } else {
        has_security_evidence.then_some(true)
    };
    recorded_changed_files.extend(options.changed_files);
    let changed_files = stable_relative_files(
        &recorded_changed_files,
        metadata.map(|value| value.cwd.as_str()),
    );
    let assumptions = unverified_assumptions.into_iter().collect::<Vec<_>>();
    let human_review_required = status != VerificationStatus::Passed
        || security_gate_passed == Some(false)
        || !assumptions.is_empty();
    let summary = format!(
        "Verification {} under policy `{}`; {} evidence item(s), {} risk event(s).",
        status_label(&status),
        policy,
        evidence_ids.len(),
        risk_events.len()
    );

    SessionReportV1 {
        schema_version: SESSION_REPORT_SCHEMA_VERSION,
        generated_by: generated_by.to_string(),
        session_id: session_id.to_string(),
        parent_session_id: metadata.and_then(|value| value.parent_session_id.clone()),
        human_initiator: options.human_initiator,
        goal_id: options.goal_id,
        agent_role: options.agent_role,
        model: metadata.and_then(|value| value.model.clone()),
        summary,
        changed_files,
        commands,
        verification: VerificationSummary {
            policy,
            status,
            rounds,
            evidence_ids: evidence_ids.into_iter().collect(),
            missing_requirements,
        },
        security_gate_passed,
        risk_events,
        unverified_assumptions: assumptions,
        cost: options.cost,
        recovery,
        human_review_required,
        reviewed_by_human: false,
        record_head_digest: record_head_digest(lines),
    }
}

/// Write a report with a same-directory temporary file and atomic rename.
pub fn write_session_report(report: &SessionReportV1) -> Result<PathBuf> {
    let path =
        allthecodes_config::paths::runs_dir(&report.session_id).join(SESSION_REPORT_FILE_NAME);
    write_session_report_to_path(report, &path)
}

pub fn write_session_report_to_path(report: &SessionReportV1, path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("session report path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(report).context("failed to serialize session report")?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        SESSION_REPORT_FILE_NAME,
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("failed to create temporary report {}", temporary.display()))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| "failed to flush session report");
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "failed to atomically install session report {}",
                path.display()
            )
        });
    }
    Ok(path.to_path_buf())
}

/// Append the generated-record fact only after the report rename succeeds.
pub async fn generate_and_record_session_report(
    session_id: &str,
    lines: &[RecordLine],
    generated_by: &str,
    options: SessionReportOptions,
    recorder: &SessionRecorderHandle,
) -> Result<(PathBuf, SessionReportV1)> {
    let report = project_session_report(session_id, lines, generated_by, options);
    let path = write_session_report(&report)?;
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read generated report {}", path.display()))?;
    recorder
        .add(vec![RecordItem::SessionReportGenerated(
            crate::record_replay::types::SessionReportGeneratedRecord {
                report_version: SESSION_REPORT_SCHEMA_VERSION,
                relative_path: SESSION_REPORT_FILE_NAME.into(),
                digest: digest_bytes(&bytes),
                record_head_digest: report.record_head_digest.clone(),
            },
        )])
        .await
        .context("failed to append session report generated record")?;
    Ok((path, report))
}

/// Verify both the report bytes and the canonical head captured before the
/// `SessionReportGenerated` record was appended.
pub fn verify_session_report_file(path: &Path, lines: &[RecordLine]) -> Result<bool> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read session report {}", path.display()))?;
    let report: SessionReportV1 = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(_) => return Ok(false),
    };
    let Some((index, generated)) = lines.iter().enumerate().rev().find(|(_, line)| {
        matches!(
            &line.item,
            RecordItem::SessionReportGenerated(record)
                if record.relative_path == SESSION_REPORT_FILE_NAME
        )
    }) else {
        return Ok(false);
    };
    let RecordItem::SessionReportGenerated(generated) = &generated.item else {
        unreachable!();
    };
    Ok(generated.digest == digest_bytes(&bytes)
        && generated.record_head_digest == report.record_head_digest
        && report.record_head_digest == record_head_digest(&lines[..index]))
}

/// Hash the ordered canonical lines. A report points to the head immediately
/// before its own generated-record line.
pub fn record_head_digest(lines: &[RecordLine]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        if let Ok(bytes) = serde_json::to_vec(line) {
            hasher.update(bytes);
            hasher.update(b"\n");
        }
    }
    format!("{:x}", hasher.finalize())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn stable_relative_files(files: &[String], cwd: Option<&str>) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| {
            let path = Path::new(file);
            if path.is_absolute() {
                let cwd = cwd.map(Path::new)?;
                path.strip_prefix(cwd)
                    .ok()
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            } else {
                Some(file.replace('\\', "/"))
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn evidence_risk(kind: &EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Build => "build",
        EvidenceKind::Test => "build",
        EvidenceKind::Lint => "build",
        EvidenceKind::SecurityGate => "security_gate",
        EvidenceKind::ApiSmoke => "read",
        EvidenceKind::BrowserSmoke => "read",
    }
}

fn status_label(status: &VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Passed => "passed",
        VerificationStatus::Failed => "failed",
        VerificationStatus::Incomplete => "incomplete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_replay::types::{
        QueryEventRecord, SecurityDecisionRecord, SessionMetaRecord, VerificationEvidenceRecord,
        VerificationFinishedRecord, VerificationStartedRecord,
    };
    use allthecodes_types::security::{TaintDecisionKind, TaintSink};
    use chrono::Utc;

    fn lines() -> Vec<RecordLine> {
        vec![
            RecordLine::new(
                "session-1",
                0,
                RecordItem::SessionMeta(SessionMetaRecord {
                    created_at: Utc::now(),
                    cwd: "/private/repo".into(),
                    workspace_key: None,
                    workspace_root: None,
                    workspace_name: None,
                    model: Some("model-x".into()),
                    config_summary: None,
                    parent_session_id: Some("parent-1".into()),
                    branch_from_seq: None,
                    migrated_from: None,
                }),
            ),
            RecordLine::new(
                "session-1",
                1,
                RecordItem::VerificationStarted(VerificationStartedRecord {
                    verification_id: "verify-1".into(),
                    policy: "targeted_tests".into(),
                    round: 1,
                }),
            ),
            RecordLine::new(
                "session-1",
                2,
                RecordItem::VerificationEvidence(VerificationEvidenceRecord {
                    evidence_id: "evidence-1".into(),
                    kind: EvidenceKind::Test,
                    tool_use_id: "tool-1".into(),
                    command_digest: "digest-command".into(),
                    exit_code: 0,
                    duration_ms: 12,
                    artifact_ids: vec![],
                }),
            ),
            RecordLine::new(
                "session-1",
                3,
                RecordItem::VerificationFinished(VerificationFinishedRecord {
                    verification_id: "verify-1".into(),
                    policy: "targeted_tests".into(),
                    round: 1,
                    status: VerificationStatus::Passed,
                    evidence_ids: vec!["evidence-1".into()],
                    missing_requirements: vec![],
                    unverified_assumptions: vec![],
                }),
            ),
            RecordLine::new(
                "session-1",
                4,
                RecordItem::SecurityDecision(SecurityDecisionRecord {
                    tool_use_id: "tool-2".into(),
                    tool_name: "Bash".into(),
                    input_digest: "digest-input".into(),
                    sink: TaintSink::Shell,
                    decision: TaintDecisionKind::Allow,
                    rule_ids: vec!["rule-1".into()],
                    source_digests: vec!["digest-source".into()],
                    user_override: true,
                }),
            ),
        ]
    }

    #[test]
    fn projection_is_sorted_and_redacted() {
        let report = project_session_report(
            "session-1",
            &lines(),
            "test",
            SessionReportOptions {
                changed_files: vec!["z.rs".into(), "/private/repo/a.rs".into(), "a.rs".into()],
                ..Default::default()
            },
        );
        assert_eq!(report.verification.status, VerificationStatus::Passed);
        assert_eq!(report.changed_files, vec!["a.rs", "z.rs"]);
        assert_eq!(report.commands[0].command_digest, "digest-command");
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/repo"));
        assert!(!json.contains("digest-source"));
        assert_eq!(report.security_gate_passed, None);
    }

    #[test]
    fn projection_counts_request_stream_and_fallback_recovery_separately() {
        let mut records = lines();
        for (offset, phase) in ["request", "stream", "stream", "fallback"]
            .into_iter()
            .enumerate()
        {
            records.push(RecordLine::new(
                "session-1",
                5 + offset as u64,
                RecordItem::QueryEvent(QueryEventRecord::RequestStart {
                    provider: Some("openai-codex".to_string()),
                    model: Some("gpt-test".to_string()),
                    attempt: offset as u32 + 2,
                    is_retry: true,
                    retry_phase: Some(phase.to_string()),
                }),
            ));
        }

        let report = project_session_report(
            "session-1",
            &records,
            "test",
            SessionReportOptions::default(),
        );
        assert_eq!(report.recovery.request_retries, 1);
        assert_eq!(report.recovery.stream_retries, 2);
        assert_eq!(report.recovery.model_fallbacks, 1);
    }

    #[test]
    fn projection_uses_canonical_edit_and_security_evidence() {
        let mut records = lines();
        records.push(RecordLine::new(
            "session-1",
            5,
            RecordItem::Message(crate::record_replay::types::MessageRecord::from_message(
                &allthecodes_types::message::Message::Attachment(
                    allthecodes_types::message::AttachmentMessage {
                        uuid: uuid::Uuid::new_v4(),
                        timestamp: 0,
                        attachment: Attachment::EditedTextFile {
                            path: "/private/repo/src/lib.rs".into(),
                        },
                    },
                ),
            )),
        ));
        records.push(RecordLine::new(
            "session-1",
            6,
            RecordItem::VerificationEvidence(VerificationEvidenceRecord {
                evidence_id: "security-1".into(),
                kind: EvidenceKind::SecurityGate,
                tool_use_id: "tool-security".into(),
                command_digest: "security-digest".into(),
                exit_code: 0,
                duration_ms: 3,
                artifact_ids: vec![],
            }),
        ));

        let report = project_session_report("session-1", &records, "test", Default::default());

        assert_eq!(report.changed_files, vec!["src/lib.rs"]);
        assert_eq!(report.security_gate_passed, Some(true));
    }

    #[test]
    fn report_file_hash_and_record_head_detect_tampering() {
        let temp = tempfile::tempdir().unwrap();
        let base_lines = lines();
        let report = project_session_report("session-1", &base_lines, "test", Default::default());
        let path = temp.path().join(SESSION_REPORT_FILE_NAME);
        write_session_report_to_path(&report, &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut linked_lines = base_lines;
        linked_lines.push(RecordLine::new(
            "session-1",
            5,
            RecordItem::SessionReportGenerated(
                crate::record_replay::types::SessionReportGeneratedRecord {
                    report_version: 1,
                    relative_path: SESSION_REPORT_FILE_NAME.into(),
                    digest: digest_bytes(&bytes),
                    record_head_digest: report.record_head_digest.clone(),
                },
            ),
        ));
        // The report was projected from the first four lines, so the linkage
        // is valid until either the file or canonical evidence changes.
        assert!(verify_session_report_file(&path, &linked_lines).unwrap());
        fs::write(&path, br#"{"tampered":true}"#).unwrap();
        assert!(!verify_session_report_file(&path, &linked_lines).unwrap());
    }
}
