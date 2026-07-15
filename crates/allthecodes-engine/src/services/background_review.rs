use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::ErrorKind;
use std::path::PathBuf;
use uuid::Uuid;

const DEFAULT_TURN_THRESHOLD: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundReviewProposal {
    pub id: String,
    pub source_session_id: String,
    pub kind: BackgroundReviewProposalKind,
    pub summary: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewProposalKind {
    MemoryAdd,
    MemoryReplace,
    SkillCreate,
    SkillPatch,
    WorkflowWarning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundReviewDisposition {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundReviewDecisionReceipt {
    pub proposal_id: String,
    pub kind: BackgroundReviewProposalKind,
    pub disposition: BackgroundReviewDisposition,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum BackgroundReviewDecisionError {
    InvalidId,
    NotFound,
    AlreadyClaimed,
    AlreadyDecided(BackgroundReviewDecisionReceipt),
    WrongDomain,
    InvalidPayload,
    Io(anyhow::Error),
}

impl fmt::Display for BackgroundReviewDecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => formatter.write_str("invalid background review proposal id"),
            Self::NotFound => formatter.write_str("background review proposal not found"),
            Self::AlreadyClaimed => {
                formatter.write_str("background review proposal is already being decided")
            }
            Self::AlreadyDecided(receipt) => write!(
                formatter,
                "background review proposal was already {:?}",
                receipt.disposition
            ),
            Self::WrongDomain => {
                formatter.write_str("background review proposal belongs to another domain")
            }
            Self::InvalidPayload => {
                formatter.write_str("background review proposal payload is invalid")
            }
            Self::Io(error) => write!(formatter, "background review proposal I/O failed: {error}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryProposalDecisionOutcome {
    pub proposal: BackgroundReviewProposal,
    pub receipt: BackgroundReviewDecisionReceipt,
    pub memory: Option<allthecodes_session::memdir::MemoryEntry>,
}

impl std::error::Error for BackgroundReviewDecisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

/// Exclusive filesystem claim for a pending background-review proposal.
///
/// Dropping an uncommitted claim restores the proposal. Committing first writes
/// a durable disposition receipt and only then removes the claimed payload, so
/// a cleanup failure cannot make an already-applied proposal pending again.
#[derive(Debug)]
pub struct BackgroundReviewClaim {
    proposal: BackgroundReviewProposal,
    pending_path: PathBuf,
    claim_path: PathBuf,
    committed: bool,
}

impl BackgroundReviewClaim {
    pub fn proposal(&self) -> &BackgroundReviewProposal {
        &self.proposal
    }

    pub fn commit(
        mut self,
        disposition: BackgroundReviewDisposition,
    ) -> std::result::Result<BackgroundReviewDecisionReceipt, BackgroundReviewDecisionError> {
        let receipt = BackgroundReviewDecisionReceipt {
            proposal_id: self.proposal.id.clone(),
            kind: self.proposal.kind,
            disposition,
            decided_at: Utc::now(),
        };
        write_decision_receipt(&receipt)?;
        self.committed = true;
        match std::fs::remove_file(&self.claim_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BackgroundReviewDecisionError::Io(
                    anyhow::Error::new(error).context(format!(
                        "failed to remove claimed proposal {}",
                        self.claim_path.display()
                    )),
                ));
            }
        }
        Ok(receipt)
    }
}

impl Drop for BackgroundReviewClaim {
    fn drop(&mut self) {
        if self.committed || !self.claim_path.exists() || self.pending_path.exists() {
            return;
        }
        let _ = std::fs::rename(&self.claim_path, &self.pending_path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundReviewConfig {
    pub enabled: bool,
    pub turn_threshold: usize,
}

impl Default for BackgroundReviewConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            turn_threshold: DEFAULT_TURN_THRESHOLD,
        }
    }
}

impl BackgroundReviewConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(value) = std::env::var("ALLTHECODES_BACKGROUND_REVIEW") {
            let normalized = value.trim().to_ascii_lowercase();
            if matches!(normalized.as_str(), "0" | "false" | "off" | "no") {
                config.enabled = false;
            } else if matches!(normalized.as_str(), "1" | "true" | "on" | "yes") {
                config.enabled = true;
            }
        }
        if let Ok(value) = std::env::var("ALLTHECODES_BACKGROUND_REVIEW_TURN_THRESHOLD") {
            if let Ok(threshold) = value.trim().parse::<usize>() {
                config.turn_threshold = threshold.max(1);
            }
        }
        config
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundReviewInput {
    pub source_session_id: String,
    pub cwd: String,
    pub turn_count: usize,
    pub replay_seq_start: Option<u64>,
    pub replay_seq_end: Option<u64>,
    pub recent_summary: String,
    pub tool_errors: Vec<String>,
    pub similar_session_hits: Vec<String>,
}

pub fn review_proposals_dir() -> PathBuf {
    allthecodes_config::paths::data_root().join("review_proposals")
}

pub fn proposal_path(id: &str) -> PathBuf {
    review_proposals_dir().join(format!("{}.json", sanitize_proposal_id(id)))
}

fn proposal_claims_dir() -> PathBuf {
    review_proposals_dir().join(".claims")
}

fn proposal_receipts_dir() -> PathBuf {
    review_proposals_dir().join(".receipts")
}

fn proposal_claim_path(id: &str) -> PathBuf {
    proposal_claims_dir().join(format!("{}.json", sanitize_proposal_id(id)))
}

fn proposal_receipt_path(id: &str) -> PathBuf {
    proposal_receipts_dir().join(format!("{}.json", sanitize_proposal_id(id)))
}

pub fn stage_background_review_if_due(
    input: BackgroundReviewInput,
    config: &BackgroundReviewConfig,
) -> Result<Option<BackgroundReviewProposal>> {
    if !config.enabled || input.turn_count < config.turn_threshold {
        return Ok(None);
    }

    let proposal = BackgroundReviewProposal {
        id: new_proposal_id(&input.source_session_id),
        source_session_id: input.source_session_id.clone(),
        kind: BackgroundReviewProposalKind::WorkflowWarning,
        summary: bounded_summary(&input.recent_summary),
        payload: serde_json::json!({
            "source_session_id": input.source_session_id,
            "cwd": input.cwd,
            "turn_count": input.turn_count,
            "replay_seq_start": input.replay_seq_start,
            "replay_seq_end": input.replay_seq_end,
            "recent_summary": bounded_summary(&input.recent_summary),
            "tool_errors": input.tool_errors.into_iter().map(|error| bounded_summary(&error)).collect::<Vec<_>>(),
            "similar_session_hits": input.similar_session_hits,
        }),
        created_at: Utc::now(),
    };

    std::fs::create_dir_all(review_proposals_dir()).with_context(|| {
        format!(
            "failed to create review proposal directory {}",
            review_proposals_dir().display()
        )
    })?;
    let path = proposal_path(&proposal.id);
    let json = serde_json::to_vec_pretty(&proposal)?;
    atomic_write(&path, &json)
        .with_context(|| format!("failed to write review proposal {}", path.display()))?;
    Ok(Some(proposal))
}

pub fn list_background_review_proposals() -> Result<Vec<BackgroundReviewProposal>> {
    let dir = review_proposals_dir();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut proposals = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read review proposal directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read review proposal {}", path.display()))?;
        let proposal: BackgroundReviewProposal = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse review proposal {}", path.display()))?;
        proposals.push(proposal);
    }
    proposals.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(proposals)
}

pub fn load_background_review_proposal(id: &str) -> Result<BackgroundReviewProposal> {
    anyhow::ensure!(valid_proposal_id(id), "invalid review proposal id");
    let path = proposal_path(id);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("review proposal '{}' not found", id))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse review proposal {}", path.display()))
}

pub fn reject_background_review_proposal(id: &str) -> Result<BackgroundReviewProposal> {
    let claim = claim_background_review_proposal(id).map_err(anyhow::Error::new)?;
    let proposal = claim.proposal().clone();
    claim
        .commit(BackgroundReviewDisposition::Rejected)
        .map_err(anyhow::Error::new)?;
    Ok(proposal)
}

pub fn claim_background_review_proposal(
    id: &str,
) -> std::result::Result<BackgroundReviewClaim, BackgroundReviewDecisionError> {
    if !valid_proposal_id(id) {
        return Err(BackgroundReviewDecisionError::InvalidId);
    }
    if let Some(receipt) = load_decision_receipt(id)? {
        return Err(BackgroundReviewDecisionError::AlreadyDecided(receipt));
    }

    let pending_path = proposal_path(id);
    let claim_path = proposal_claim_path(id);
    std::fs::create_dir_all(proposal_claims_dir()).map_err(|error| {
        BackgroundReviewDecisionError::Io(anyhow::Error::new(error).context(format!(
            "failed to create proposal claim directory {}",
            proposal_claims_dir().display()
        )))
    })?;
    // A hard-link create is atomic and fails when the claim path already
    // exists. Unlike `rename`, it cannot replace another reviewer's claim on
    // Unix. Both paths live under the same data root, so they share a
    // filesystem.
    match std::fs::hard_link(&pending_path, &claim_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return Err(BackgroundReviewDecisionError::AlreadyClaimed);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Some(receipt) = load_decision_receipt(id)? {
                return Err(BackgroundReviewDecisionError::AlreadyDecided(receipt));
            }
            if claim_path.exists() {
                return Err(BackgroundReviewDecisionError::AlreadyClaimed);
            }
            return Err(BackgroundReviewDecisionError::NotFound);
        }
        Err(error) => {
            return Err(BackgroundReviewDecisionError::Io(
                anyhow::Error::new(error).context(format!(
                    "failed to claim review proposal {}",
                    pending_path.display()
                )),
            ));
        }
    }
    if let Err(error) = std::fs::remove_file(&pending_path) {
        let _ = std::fs::remove_file(&claim_path);
        return Err(BackgroundReviewDecisionError::Io(
            anyhow::Error::new(error).context(format!(
                "failed to remove pending review proposal after claiming {}",
                pending_path.display()
            )),
        ));
    }

    let proposal = match read_proposal_file(&claim_path) {
        Ok(proposal) if proposal.id == id => proposal,
        Ok(_) => {
            let _ = std::fs::rename(&claim_path, &pending_path);
            return Err(BackgroundReviewDecisionError::InvalidId);
        }
        Err(error) => {
            let _ = std::fs::rename(&claim_path, &pending_path);
            return Err(BackgroundReviewDecisionError::Io(error));
        }
    };

    Ok(BackgroundReviewClaim {
        proposal,
        pending_path,
        claim_path,
        committed: false,
    })
}

pub fn load_background_review_decision_receipt(
    id: &str,
) -> std::result::Result<Option<BackgroundReviewDecisionReceipt>, BackgroundReviewDecisionError> {
    if !valid_proposal_id(id) {
        return Err(BackgroundReviewDecisionError::InvalidId);
    }
    load_decision_receipt(id)
}

/// Decide a Memory-domain proposal through the shared exclusive claim path.
/// Skill proposals are rejected without consuming them. A retry after a memory
/// write but before receipt persistence recognizes the proposal's durable
/// `approval_id` and finalizes without writing the entry a second time.
pub fn decide_memory_background_review_proposal(
    id: &str,
    cwd: &std::path::Path,
    disposition: BackgroundReviewDisposition,
) -> std::result::Result<MemoryProposalDecisionOutcome, BackgroundReviewDecisionError> {
    let claim = claim_background_review_proposal(id)?;
    let proposal = claim.proposal().clone();
    if !is_memory_review_proposal(&proposal) {
        return Err(BackgroundReviewDecisionError::WrongDomain);
    }

    let memory = match (disposition, proposal.kind) {
        (BackgroundReviewDisposition::Approved, BackgroundReviewProposalKind::WorkflowWarning) => {
            None
        }
        (
            BackgroundReviewDisposition::Approved,
            BackgroundReviewProposalKind::MemoryAdd | BackgroundReviewProposalKind::MemoryReplace,
        ) => {
            let write = memory_write_from_review(&proposal)
                .ok_or(BackgroundReviewDecisionError::InvalidPayload)?;
            let scope = write.target.default_scope();
            let existing = allthecodes_session::memdir::read_memory(&write.key, scope, cwd)
                .ok()
                .filter(|entry| entry.approval_id.as_deref() == Some(proposal.id.as_str()));
            Some(match existing {
                Some(entry) => entry,
                None => allthecodes_session::memdir::write_curated_memory(write, cwd)
                    .map_err(BackgroundReviewDecisionError::Io)?,
            })
        }
        (BackgroundReviewDisposition::Rejected, _) => None,
        _ => return Err(BackgroundReviewDecisionError::WrongDomain),
    };

    let receipt = claim.commit(disposition)?;
    Ok(MemoryProposalDecisionOutcome {
        proposal,
        receipt,
        memory,
    })
}

pub fn is_memory_review_proposal(proposal: &BackgroundReviewProposal) -> bool {
    matches!(
        proposal.kind,
        BackgroundReviewProposalKind::MemoryAdd
            | BackgroundReviewProposalKind::MemoryReplace
            | BackgroundReviewProposalKind::WorkflowWarning
    )
}

pub fn memory_write_from_review(
    proposal: &BackgroundReviewProposal,
) -> Option<allthecodes_session::memdir::CuratedMemoryWrite> {
    let memory = proposal.payload.get("memory").unwrap_or(&proposal.payload);
    let target = match memory.get("target")?.as_str()? {
        "user" => allthecodes_session::memdir::CuratedMemoryTarget::User,
        "project" => allthecodes_session::memdir::CuratedMemoryTarget::Project,
        "reference" => allthecodes_session::memdir::CuratedMemoryTarget::Reference,
        "feedback" => allthecodes_session::memdir::CuratedMemoryTarget::Feedback,
        _ => return None,
    };
    let key = memory.get("key")?.as_str()?.trim();
    let value = memory.get("value")?.as_str()?;
    if key.is_empty()
        || key.chars().count() > 200
        || key.chars().any(char::is_control)
        || value.len() > 100_000
        || proposal.source_session_id.chars().count() > 200
    {
        return None;
    }
    Some(allthecodes_session::memdir::CuratedMemoryWrite {
        target,
        key: key.to_string(),
        value: value.to_string(),
        source_session_id: Some(proposal.source_session_id.clone()),
        approval_id: Some(proposal.id.clone()),
    })
}

fn read_proposal_file(
    path: &std::path::Path,
) -> std::result::Result<BackgroundReviewProposal, anyhow::Error> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read review proposal {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse review proposal {}", path.display()))
}

fn write_decision_receipt(
    receipt: &BackgroundReviewDecisionReceipt,
) -> std::result::Result<(), BackgroundReviewDecisionError> {
    std::fs::create_dir_all(proposal_receipts_dir()).map_err(|error| {
        BackgroundReviewDecisionError::Io(anyhow::Error::new(error).context(format!(
            "failed to create proposal receipt directory {}",
            proposal_receipts_dir().display()
        )))
    })?;
    let path = proposal_receipt_path(&receipt.proposal_id);
    if path.exists() {
        let Some(existing) = load_decision_receipt(&receipt.proposal_id)? else {
            return Err(BackgroundReviewDecisionError::Io(anyhow::anyhow!(
                "proposal receipt disappeared during decision"
            )));
        };
        return Err(BackgroundReviewDecisionError::AlreadyDecided(existing));
    }
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| BackgroundReviewDecisionError::Io(anyhow::Error::new(error)))?;
    atomic_write(&path, &bytes).map_err(BackgroundReviewDecisionError::Io)
}

fn load_decision_receipt(
    id: &str,
) -> std::result::Result<Option<BackgroundReviewDecisionReceipt>, BackgroundReviewDecisionError> {
    let path = proposal_receipt_path(id);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(BackgroundReviewDecisionError::Io(
                anyhow::Error::new(error).context(format!(
                    "failed to read proposal receipt {}",
                    path.display()
                )),
            ));
        }
    };
    serde_json::from_str(&content).map(Some).map_err(|error| {
        BackgroundReviewDecisionError::Io(anyhow::Error::new(error).context(format!(
            "failed to parse proposal receipt {}",
            path.display()
        )))
    })
}

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("review proposal path has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("proposal"),
        Uuid::new_v4().simple()
    ));
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write temporary file {}", temporary.display()))?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(anyhow::Error::new(error)
            .context(format!("failed to install review file {}", path.display())));
    }
    Ok(())
}

fn valid_proposal_id(value: &str) -> bool {
    !value.is_empty() && value == sanitize_proposal_id(value)
}

fn new_proposal_id(session_id: &str) -> String {
    format!(
        "review-{}-{}",
        sanitize_proposal_id(session_id),
        Uuid::new_v4().simple()
    )
}

fn sanitize_proposal_id(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('-').is_empty() {
        "proposal".to_string()
    } else {
        sanitized
    }
}

fn bounded_summary(value: &str) -> String {
    let mut summary = value.trim().chars().take(500).collect::<String>();
    if summary.is_empty() {
        summary =
            "Review recent session behavior for possible memory or skill updates.".to_string();
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn review_input(turn_count: usize) -> BackgroundReviewInput {
        BackgroundReviewInput {
            source_session_id: "review-session".to_string(),
            cwd: "/repo".to_string(),
            turn_count,
            replay_seq_start: Some(10),
            replay_seq_end: Some(20),
            recent_summary: "changed memory and skills flow".to_string(),
            tool_errors: vec!["cargo test failed once".to_string()],
            similar_session_hits: Vec::new(),
        }
    }

    fn write_fixture(proposal: &BackgroundReviewProposal) {
        let bytes = serde_json::to_vec_pretty(proposal).unwrap();
        atomic_write(&proposal_path(&proposal.id), &bytes).unwrap();
    }

    #[test]
    #[serial]
    fn background_review_does_not_trigger_before_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let config = BackgroundReviewConfig {
            enabled: true,
            turn_threshold: 3,
        };

        let proposal = stage_background_review_if_due(review_input(2), &config).unwrap();

        assert!(proposal.is_none());
        assert!(list_background_review_proposals().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn background_review_writes_proposal_after_threshold_and_survives_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let config = BackgroundReviewConfig {
            enabled: true,
            turn_threshold: 3,
        };

        let proposal = stage_background_review_if_due(review_input(3), &config)
            .unwrap()
            .expect("proposal after threshold");

        assert_eq!(proposal.source_session_id, "review-session");
        assert_eq!(proposal.kind, BackgroundReviewProposalKind::WorkflowWarning);
        assert!(proposal_path(&proposal.id).is_file());

        let listed = list_background_review_proposals().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, proposal.id);

        let loaded = load_background_review_proposal(&proposal.id).unwrap();
        assert_eq!(loaded.summary, proposal.summary);
        assert_eq!(loaded.payload["replay_seq_start"], 10);
    }

    #[test]
    #[serial]
    fn background_review_does_not_mutate_memory_or_skills_before_approval() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let config = BackgroundReviewConfig {
            enabled: true,
            turn_threshold: 1,
        };

        let proposal = stage_background_review_if_due(review_input(1), &config)
            .unwrap()
            .expect("proposal after threshold");

        assert!(proposal_path(&proposal.id).is_file());
        assert!(!allthecodes_config::paths::memory_dir_global().exists());
        assert!(!allthecodes_config::paths::skills_dir_global().exists());
    }

    #[test]
    #[serial]
    fn proposal_claim_is_exclusive_and_drop_restores_pending_record() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let proposal = stage_background_review_if_due(
            review_input(1),
            &BackgroundReviewConfig {
                enabled: true,
                turn_threshold: 1,
            },
        )
        .unwrap()
        .unwrap();

        let claim = claim_background_review_proposal(&proposal.id).unwrap();
        assert!(matches!(
            claim_background_review_proposal(&proposal.id),
            Err(BackgroundReviewDecisionError::AlreadyClaimed)
        ));
        assert!(load_background_review_proposal(&proposal.id).is_err());
        drop(claim);
        assert_eq!(
            load_background_review_proposal(&proposal.id).unwrap().id,
            proposal.id
        );
    }

    #[test]
    #[serial]
    fn committed_claim_writes_receipt_and_cannot_be_replayed() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let proposal = stage_background_review_if_due(
            review_input(1),
            &BackgroundReviewConfig {
                enabled: true,
                turn_threshold: 1,
            },
        )
        .unwrap()
        .unwrap();

        let receipt = claim_background_review_proposal(&proposal.id)
            .unwrap()
            .commit(BackgroundReviewDisposition::Approved)
            .unwrap();
        assert_eq!(receipt.proposal_id, proposal.id);
        assert_eq!(receipt.disposition, BackgroundReviewDisposition::Approved);
        assert_eq!(
            load_background_review_decision_receipt(&proposal.id)
                .unwrap()
                .unwrap(),
            receipt
        );
        assert!(matches!(
            claim_background_review_proposal(&proposal.id),
            Err(BackgroundReviewDecisionError::AlreadyDecided(existing))
                if existing == receipt
        ));
        assert!(list_background_review_proposals().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn invalid_ids_do_not_alias_sanitized_proposal_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        assert!(matches!(
            claim_background_review_proposal("../proposal"),
            Err(BackgroundReviewDecisionError::InvalidId)
        ));
        assert!(load_background_review_proposal("../proposal").is_err());
    }

    #[test]
    #[serial]
    fn memory_approval_writes_provenance_once_and_replay_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let cwd = tmp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let proposal = BackgroundReviewProposal {
            id: "review-memory-approval".to_string(),
            source_session_id: "session-memory".to_string(),
            kind: BackgroundReviewProposalKind::MemoryAdd,
            summary: "remember release contract".to_string(),
            payload: serde_json::json!({
                "cwd": cwd,
                "memory": {
                    "target": "project",
                    "key": "release-contract",
                    "value": "run the API contract gate"
                }
            }),
            created_at: Utc::now(),
        };
        write_fixture(&proposal);

        let outcome = decide_memory_background_review_proposal(
            &proposal.id,
            &cwd,
            BackgroundReviewDisposition::Approved,
        )
        .unwrap();
        let entry = outcome.memory.unwrap();
        assert_eq!(entry.approval_id.as_deref(), Some(proposal.id.as_str()));
        assert_eq!(
            entry.source_session_id.as_deref(),
            Some(proposal.source_session_id.as_str())
        );
        assert!(matches!(
            decide_memory_background_review_proposal(
                &proposal.id,
                &cwd,
                BackgroundReviewDisposition::Approved,
            ),
            Err(BackgroundReviewDecisionError::AlreadyDecided(_))
        ));
        assert_eq!(
            allthecodes_session::memdir::list_memories(
                allthecodes_session::memdir::MemoryScope::Project,
                &cwd,
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    #[serial]
    fn skill_proposal_cannot_be_consumed_by_memory_decision() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let proposal = BackgroundReviewProposal {
            id: "review-skill-domain".to_string(),
            source_session_id: "session-skill".to_string(),
            kind: BackgroundReviewProposalKind::SkillCreate,
            summary: "skill proposal".to_string(),
            payload: serde_json::json!({"name": "example"}),
            created_at: Utc::now(),
        };
        write_fixture(&proposal);

        assert!(matches!(
            decide_memory_background_review_proposal(
                &proposal.id,
                tmp.path(),
                BackgroundReviewDisposition::Rejected,
            ),
            Err(BackgroundReviewDecisionError::WrongDomain)
        ));
        assert_eq!(
            load_background_review_proposal(&proposal.id).unwrap(),
            proposal
        );
    }
}
