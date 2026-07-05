use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    let json = serde_json::to_string_pretty(&proposal)?;
    std::fs::write(&path, json)
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
    let path = proposal_path(id);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("review proposal '{}' not found", id))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse review proposal {}", path.display()))
}

pub fn reject_background_review_proposal(id: &str) -> Result<BackgroundReviewProposal> {
    let proposal = load_background_review_proposal(id)?;
    let path = proposal_path(id);
    std::fs::remove_file(&path)
        .with_context(|| format!("failed to remove review proposal {}", path.display()))?;
    Ok(proposal)
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
}
