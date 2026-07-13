use allthecodes_session::record_replay::types::{EvidenceKind, VerificationEvidenceRecord};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationPolicy {
    pub name: String,
    pub required: Vec<EvidenceKind>,
    pub max_rounds: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationDecision {
    Passed {
        evidence_ids: Vec<String>,
    },
    Continue {
        round: u8,
        missing: Vec<EvidenceKind>,
        failures: Vec<String>,
    },
    Incomplete {
        round: u8,
        missing: Vec<EvidenceKind>,
        failures: Vec<String>,
    },
}

pub fn policy_by_name(name: &str) -> Result<VerificationPolicy> {
    match name {
        "none" => Ok(VerificationPolicy {
            name: name.into(),
            required: vec![],
            max_rounds: 0,
        }),
        "targeted_tests" => Ok(VerificationPolicy {
            name: name.into(),
            required: vec![EvidenceKind::Test],
            max_rounds: 3,
        }),
        "build_and_test" => Ok(VerificationPolicy {
            name: name.into(),
            required: vec![EvidenceKind::Build, EvidenceKind::Test],
            max_rounds: 3,
        }),
        "release_gate" => Ok(VerificationPolicy {
            name: name.into(),
            required: vec![
                EvidenceKind::Build,
                EvidenceKind::Test,
                EvidenceKind::Lint,
                EvidenceKind::SecurityGate,
            ],
            max_rounds: 3,
        }),
        other => bail!("unknown verification policy: {other}"),
    }
}

pub fn evaluate(
    policy: &VerificationPolicy,
    evidence: &[VerificationEvidenceRecord],
    round: u8,
    failures: &[String],
) -> VerificationDecision {
    let present = evidence
        .iter()
        .map(|record| &record.kind)
        .collect::<std::collections::HashSet<_>>();
    let missing = policy
        .required
        .iter()
        .filter(|kind| !present.contains(kind))
        .cloned()
        .collect::<Vec<_>>();
    let evidence_ids = evidence
        .iter()
        .filter(|record| policy.required.contains(&record.kind))
        .map(|record| record.evidence_id.clone())
        .collect::<Vec<_>>();

    if missing.is_empty() {
        VerificationDecision::Passed { evidence_ids }
    } else if round < policy.max_rounds {
        VerificationDecision::Continue {
            round: round.saturating_add(1),
            missing,
            failures: failures.to_vec(),
        }
    } else {
        VerificationDecision::Incomplete {
            round,
            missing,
            failures: failures.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(kind: EvidenceKind) -> VerificationEvidenceRecord {
        VerificationEvidenceRecord {
            evidence_id: format!("evidence-{kind:?}"),
            kind,
            tool_use_id: "toolu-1".into(),
            command_digest: "digest".into(),
            exit_code: 0,
            duration_ms: 1,
            artifact_ids: vec![],
        }
    }

    #[test]
    fn named_policies_have_bounded_rounds() {
        assert_eq!(policy_by_name("targeted_tests").unwrap().max_rounds, 3);
        assert_eq!(policy_by_name("none").unwrap().required, vec![]);
        assert!(policy_by_name("unknown").is_err());
    }

    #[test]
    fn evaluation_passes_or_terminates_at_round_cap() {
        let policy = policy_by_name("targeted_tests").unwrap();
        assert!(matches!(
            evaluate(&policy, &[evidence(EvidenceKind::Test)], 1, &[]),
            VerificationDecision::Passed { .. }
        ));
        assert!(matches!(
            evaluate(&policy, &[], 3, &["exit=101".into()]),
            VerificationDecision::Incomplete { round: 3, .. }
        ));
    }
}
