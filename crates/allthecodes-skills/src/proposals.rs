//! Durable and symlink-safe primitives for native skill proposals.
//!
//! This module owns transaction state for the existing native proposal store.
//! Higher-level API/command adapters live in `allthecodes-engine` and must use
//! an explicit proposal scope instead of falling through between stores.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    loader, skill_proposals_dir, skills_root_for_scope, SkillProposal, SkillProposalAction,
    SkillProposalScope, SkillSource,
};

pub const MAX_PROPOSAL_MARKDOWN_BYTES: usize = 256 * 1024;
pub const MAX_TARGET_MARKDOWN_BYTES: usize = 256 * 1024;
const MAX_PROPOSAL_FILE_BYTES: u64 = (MAX_PROPOSAL_MARKDOWN_BYTES as u64) + 64 * 1024;
const MAX_PENDING_PROPOSAL_ENTRIES: usize = 10_000;
const MAX_DIAGNOSTICS: usize = 20;
const MAX_DIAGNOSTIC_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalDisposition {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProposalDecisionReceipt {
    pub proposal_id: String,
    pub scope: SkillProposalScope,
    pub action: SkillProposalAction,
    pub skill_name: String,
    pub disposition: SkillProposalDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_digest: Option<String>,
    pub decided_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProposalDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProposalValidation {
    pub actionable: bool,
    pub diagnostics: Vec<SkillProposalDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProposalTargetSnapshot {
    pub relative_target: String,
    pub exists: bool,
    pub digest: Option<String>,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSkillProposalList {
    pub proposals: Vec<SkillProposal>,
    pub diagnostic_count: usize,
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillProposalStoreError {
    #[error("invalid native skill proposal id")]
    InvalidId,
    #[error("native skill proposal not found")]
    NotFound,
    #[error("native skill proposal is already being decided")]
    AlreadyClaimed,
    #[error("native skill proposal was already decided")]
    AlreadyDecided(SkillProposalDecisionReceipt),
    #[error("native skill proposal scope does not match its owner")]
    ScopeMismatch,
    #[error("skill proposal is invalid: {0}")]
    InvalidProposal(String),
    #[error("skill proposal markdown exceeds the 256 KiB limit")]
    ProposalTooLarge,
    #[error("existing skill target exceeds the 256 KiB limit")]
    TargetTooLarge,
    #[error("skill proposal target is unsafe")]
    UnsafeTarget,
    #[error("skill proposal target is already being changed")]
    TargetBusy,
    #[error("skill proposal target state does not match the requested action")]
    TargetStateMismatch,
    #[error("skill proposal store failed while attempting to {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("skill proposal store contains invalid JSON")]
    Json(#[source] serde_json::Error),
}

#[derive(Debug)]
pub struct SkillProposalClaim {
    proposal: SkillProposal,
    pending_path: PathBuf,
    claim_path: PathBuf,
    receipt_path: PathBuf,
    committed: bool,
}

impl SkillProposalClaim {
    pub fn proposal(&self) -> &SkillProposal {
        &self.proposal
    }

    pub fn commit(
        mut self,
        disposition: SkillProposalDisposition,
        target_digest: Option<String>,
    ) -> Result<SkillProposalDecisionReceipt, SkillProposalStoreError> {
        let receipt = SkillProposalDecisionReceipt {
            proposal_id: self.proposal.id.clone(),
            scope: self.proposal.scope,
            action: self.proposal.action,
            skill_name: self.proposal.skill_name.clone(),
            disposition,
            target_digest,
            decided_at: chrono::Utc::now().to_rfc3339(),
        };
        write_json_atomic(&self.receipt_path, &receipt)?;
        self.committed = true;
        match std::fs::remove_file(&self.claim_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(SkillProposalStoreError::Io {
                    action: "remove a committed claim",
                    source,
                });
            }
        }
        Ok(receipt)
    }
}

impl Drop for SkillProposalClaim {
    fn drop(&mut self) {
        if self.committed || !self.claim_path.exists() || self.pending_path.exists() {
            return;
        }
        let _ = std::fs::rename(&self.claim_path, &self.pending_path);
    }
}

#[derive(Debug)]
struct TargetLock {
    path: PathBuf,
}

impl Drop for TargetLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn valid_proposal_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn load_skill_proposal_in_scope(
    id: &str,
    scope: SkillProposalScope,
    cwd: &Path,
) -> Result<SkillProposal, SkillProposalStoreError> {
    if !valid_proposal_id(id) {
        return Err(SkillProposalStoreError::InvalidId);
    }
    let dir = proposal_owner_dir(scope, cwd, false)?;
    let path = dir.join(format!("{id}.json"));
    read_pending_proposal(&path, id, scope)
}

/// List one native proposal owner without allowing a corrupt or oversized
/// record to suppress valid siblings. The scan is bounded independently from
/// API pagination so an attacker cannot force an unbounded directory walk.
pub fn list_pending_skill_proposals(
    scope: SkillProposalScope,
    cwd: &Path,
) -> Result<PendingSkillProposalList, SkillProposalStoreError> {
    let dir = proposal_owner_dir(scope, cwd, false)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(PendingSkillProposalList {
                proposals: Vec::new(),
                diagnostic_count: 0,
                truncated: false,
            });
        }
        Err(source) => {
            return Err(SkillProposalStoreError::Io {
                action: "list pending skill proposals",
                source,
            });
        }
    };

    let mut proposals = Vec::new();
    let mut diagnostic_count = 0usize;
    let mut scanned = 0usize;
    let mut truncated = false;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                diagnostic_count = diagnostic_count.saturating_add(1);
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if scanned == MAX_PENDING_PROPOSAL_ENTRIES {
            truncated = true;
            break;
        }
        scanned += 1;
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            diagnostic_count = diagnostic_count.saturating_add(1);
            continue;
        };
        if !valid_proposal_id(id) {
            diagnostic_count = diagnostic_count.saturating_add(1);
            continue;
        }
        match read_pending_proposal(&path, id, scope) {
            Ok(proposal) => proposals.push(proposal),
            Err(_) => diagnostic_count = diagnostic_count.saturating_add(1),
        }
    }
    proposals.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(PendingSkillProposalList {
        proposals,
        diagnostic_count,
        truncated,
    })
}

pub fn load_skill_proposal_receipt(
    id: &str,
    scope: SkillProposalScope,
    cwd: &Path,
) -> Result<Option<SkillProposalDecisionReceipt>, SkillProposalStoreError> {
    if !valid_proposal_id(id) {
        return Err(SkillProposalStoreError::InvalidId);
    }
    let dir = proposal_owner_dir(scope, cwd, false)?;
    let path = dir.join(".receipts").join(format!("{id}.json"));
    match read_bounded(&path, MAX_PROPOSAL_FILE_BYTES) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(SkillProposalStoreError::Json),
        Err(SkillProposalStoreError::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn claim_skill_proposal(
    id: &str,
    scope: SkillProposalScope,
    cwd: &Path,
) -> Result<SkillProposalClaim, SkillProposalStoreError> {
    if !valid_proposal_id(id) {
        return Err(SkillProposalStoreError::InvalidId);
    }
    if let Some(receipt) = load_skill_proposal_receipt(id, scope, cwd)? {
        return Err(SkillProposalStoreError::AlreadyDecided(receipt));
    }

    let owner_dir = proposal_owner_dir(scope, cwd, true)?;
    let claims_dir = secure_directory(&owner_dir.join(".claims"))?;
    let receipts_dir = secure_directory(&owner_dir.join(".receipts"))?;
    let pending_path = owner_dir.join(format!("{id}.json"));
    let claim_path = claims_dir.join(format!("{id}.json"));
    reject_symlink_if_present(&pending_path)?;
    reject_symlink_if_present(&claim_path)?;

    match std::fs::rename(&pending_path, &claim_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Some(receipt) = load_skill_proposal_receipt(id, scope, cwd)? {
                return Err(SkillProposalStoreError::AlreadyDecided(receipt));
            }
            if claim_path.exists() {
                return Err(SkillProposalStoreError::AlreadyClaimed);
            }
            return Err(SkillProposalStoreError::NotFound);
        }
        Err(source) => {
            return Err(SkillProposalStoreError::Io {
                action: "claim a pending proposal",
                source,
            });
        }
    }

    let proposal = match read_pending_proposal(&claim_path, id, scope) {
        Ok(proposal) => proposal,
        Err(error) => {
            let _ = std::fs::rename(&claim_path, &pending_path);
            return Err(error);
        }
    };
    Ok(SkillProposalClaim {
        proposal,
        pending_path,
        claim_path,
        receipt_path: receipts_dir.join(format!("{id}.json")),
        committed: false,
    })
}

pub fn write_pending_skill_proposal(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<(), SkillProposalStoreError> {
    if !valid_proposal_id(&proposal.id) {
        return Err(SkillProposalStoreError::InvalidId);
    }
    validate_proposal_identity(proposal, cwd)?;
    let validation = validate_skill_proposal(proposal, cwd)?;
    if !validation.actionable {
        return Err(SkillProposalStoreError::InvalidProposal(
            validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "proposal is not actionable".to_string()),
        ));
    }
    let owner_dir = proposal_owner_dir(proposal.scope, cwd, true)?;
    write_json_atomic(&owner_dir.join(format!("{}.json", proposal.id)), proposal)
}

pub fn validate_skill_proposal(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<SkillProposalValidation, SkillProposalStoreError> {
    validate_proposal_identity(proposal, cwd)?;
    if proposal.markdown.len() > MAX_PROPOSAL_MARKDOWN_BYTES {
        return Err(SkillProposalStoreError::ProposalTooLarge);
    }
    if proposal.markdown.trim().is_empty() {
        return Err(SkillProposalStoreError::InvalidProposal(
            "proposal markdown is empty".to_string(),
        ));
    }

    let source = match proposal.scope {
        SkillProposalScope::User => SkillSource::User,
        SkillProposalScope::Project => SkillSource::Project,
    };
    let (skill, raw_diagnostics) =
        loader::load_skill_from_content(&proposal.markdown, &proposal.skill_name, source);
    let mut actionable = !raw_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error());
    let mut diagnostics = raw_diagnostics
        .into_iter()
        .take(MAX_DIAGNOSTICS)
        .map(|diagnostic| SkillProposalDiagnostic {
            code: bound_text(&diagnostic.code, 80),
            message: bound_text(&diagnostic.message, MAX_DIAGNOSTIC_CHARS),
        })
        .collect::<Vec<_>>();
    // `load_skill_from_content` intentionally keeps compatibility defaults;
    // remote approval requires the essential package fields explicitly valid.
    if skill.frontmatter.description.trim().is_empty() {
        actionable = false;
        diagnostics.push(SkillProposalDiagnostic {
            code: "missing-description".to_string(),
            message: "Skill description cannot be empty.".to_string(),
        });
    }
    if let Some(frontmatter_name) = skill.frontmatter.name.as_deref() {
        if frontmatter_name != proposal.skill_name {
            actionable = false;
            diagnostics.push(SkillProposalDiagnostic {
                code: "name-mismatch".to_string(),
                message: "Frontmatter name must match the proposed skill name.".to_string(),
            });
        }
    }
    diagnostics.truncate(MAX_DIAGNOSTICS);
    Ok(SkillProposalValidation {
        actionable,
        diagnostics,
    })
}

pub fn inspect_skill_proposal_target(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<SkillProposalTargetSnapshot, SkillProposalStoreError> {
    let target = validate_proposal_identity(proposal, cwd)?;
    reject_symlink_components(&target)?;
    match std::fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(SkillProposalStoreError::UnsafeTarget);
            }
            if metadata.len() > MAX_TARGET_MARKDOWN_BYTES as u64 {
                return Err(SkillProposalStoreError::TargetTooLarge);
            }
            let bytes = read_bounded(&target, MAX_TARGET_MARKDOWN_BYTES as u64)?;
            Ok(SkillProposalTargetSnapshot {
                relative_target: relative_target(proposal.scope, &proposal.skill_name),
                exists: true,
                digest: Some(content_digest(&bytes)),
                bytes: bytes.len(),
            })
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(SkillProposalTargetSnapshot {
            relative_target: relative_target(proposal.scope, &proposal.skill_name),
            exists: false,
            digest: None,
            bytes: 0,
        }),
        Err(source) => Err(SkillProposalStoreError::Io {
            action: "inspect a skill target",
            source,
        }),
    }
}

pub fn read_skill_proposal_baseline(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<(String, SkillProposalTargetSnapshot), SkillProposalStoreError> {
    let snapshot = inspect_skill_proposal_target(proposal, cwd)?;
    if !snapshot.exists {
        return Ok((String::new(), snapshot));
    }
    let bytes = read_bounded(
        &validate_proposal_identity(proposal, cwd)?,
        MAX_TARGET_MARKDOWN_BYTES as u64,
    )?;
    let markdown = String::from_utf8(bytes).map_err(|_| {
        SkillProposalStoreError::InvalidProposal(
            "existing skill target is not valid UTF-8".to_string(),
        )
    })?;
    Ok((markdown, snapshot))
}

pub fn install_skill_proposal(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<String, SkillProposalStoreError> {
    let validation = validate_skill_proposal(proposal, cwd)?;
    if !validation.actionable {
        return Err(SkillProposalStoreError::InvalidProposal(
            validation
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "proposal is not actionable".to_string()),
        ));
    }
    let _lock = acquire_target_lock(proposal.scope, &proposal.skill_name, cwd)?;
    let target = validate_proposal_identity(proposal, cwd)?;
    let root = secure_directory(&skills_root_for_scope(proposal.scope, cwd))?;
    let skill_dir = secure_directory(&root.join(&proposal.skill_name))?;
    if !skill_dir.starts_with(&root) {
        return Err(SkillProposalStoreError::UnsafeTarget);
    }
    reject_symlink_components(&target)?;

    let exists = match std::fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(SkillProposalStoreError::UnsafeTarget);
            }
            true
        }
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(source) => {
            return Err(SkillProposalStoreError::Io {
                action: "inspect a skill target before replacement",
                source,
            });
        }
    };
    if matches!(proposal.action, SkillProposalAction::Create) == exists {
        return Err(SkillProposalStoreError::TargetStateMismatch);
    }

    let temporary = skill_dir.join(format!(
        ".SKILL.md.tmp-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| SkillProposalStoreError::Io {
                action: "create a temporary skill file",
                source,
            })?;
        file.write_all(proposal.markdown.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| SkillProposalStoreError::Io {
                action: "flush a temporary skill file",
                source,
            })?;

        // Recheck after writing the sibling temporary file and immediately
        // before replacement to fail closed on a swapped symlink.
        reject_symlink_components(&skill_dir)?;
        let canonical_root = root
            .canonicalize()
            .map_err(|source| SkillProposalStoreError::Io {
                action: "canonicalize the skills root",
                source,
            })?;
        let canonical_parent =
            skill_dir
                .canonicalize()
                .map_err(|source| SkillProposalStoreError::Io {
                    action: "canonicalize the skill directory",
                    source,
                })?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(SkillProposalStoreError::UnsafeTarget);
        }
        reject_symlink_if_present(&target)?;
        std::fs::rename(&temporary, &target).map_err(|source| SkillProposalStoreError::Io {
            action: "atomically replace a skill target",
            source,
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    Ok(content_digest(proposal.markdown.as_bytes()))
}

pub fn proposal_digest(proposal: &SkillProposal) -> String {
    let action = match proposal.action {
        SkillProposalAction::Create => "create",
        SkillProposalAction::Patch => "patch",
    };
    let scope = match proposal.scope {
        SkillProposalScope::User => "user",
        SkillProposalScope::Project => "project",
    };
    let canonical = serde_json::json!({
        "id": proposal.id,
        "action": action,
        "scope": scope,
        "skill_name": proposal.skill_name,
        "source_session_id": proposal.source_session_id,
        "markdown": proposal.markdown,
        "created_at": proposal.created_at,
    });
    content_digest(canonical.to_string().as_bytes())
}

pub fn content_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn validate_proposal_identity(
    proposal: &SkillProposal,
    cwd: &Path,
) -> Result<PathBuf, SkillProposalStoreError> {
    if !valid_proposal_id(&proposal.id) {
        return Err(SkillProposalStoreError::InvalidId);
    }
    if !crate::is_valid_skill_name(&proposal.skill_name)
        || Path::new(&proposal.skill_name).components().count() != 1
        || proposal.skill_name.contains(['/', '\\'])
    {
        return Err(SkillProposalStoreError::InvalidProposal(
            "invalid proposed skill name".to_string(),
        ));
    }
    let expected = skills_root_for_scope(proposal.scope, cwd)
        .join(&proposal.skill_name)
        .join("SKILL.md");
    if proposal.proposed_path != expected {
        return Err(SkillProposalStoreError::UnsafeTarget);
    }
    Ok(expected)
}

fn read_pending_proposal(
    path: &Path,
    id: &str,
    scope: SkillProposalScope,
) -> Result<SkillProposal, SkillProposalStoreError> {
    reject_symlink_if_present(path)?;
    let bytes = read_bounded(path, MAX_PROPOSAL_FILE_BYTES)?;
    let proposal: SkillProposal =
        serde_json::from_slice(&bytes).map_err(SkillProposalStoreError::Json)?;
    if proposal.id != id {
        return Err(SkillProposalStoreError::InvalidId);
    }
    if proposal.scope != scope {
        return Err(SkillProposalStoreError::ScopeMismatch);
    }
    Ok(proposal)
}

fn proposal_owner_dir(
    scope: SkillProposalScope,
    cwd: &Path,
    create: bool,
) -> Result<PathBuf, SkillProposalStoreError> {
    let dir = skill_proposals_dir(scope, cwd);
    if create {
        return secure_directory(&dir);
    }
    match std::fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(SkillProposalStoreError::UnsafeTarget)
        }
        Ok(_) => {
            reject_symlink_components(&dir)?;
            Ok(dir)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(dir),
        Err(source) => Err(SkillProposalStoreError::Io {
            action: "inspect a proposal owner directory",
            source,
        }),
    }
}

fn secure_directory(path: &Path) -> Result<PathBuf, SkillProposalStoreError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|source| SkillProposalStoreError::Io {
        action: "create a secure proposal directory",
        source,
    })?;
    reject_symlink_components(path)?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|source| SkillProposalStoreError::Io {
            action: "inspect a secure proposal directory",
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SkillProposalStoreError::UnsafeTarget);
    }
    Ok(path.to_path_buf())
}

fn reject_symlink_components(path: &Path) -> Result<(), SkillProposalStoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(SkillProposalStoreError::UnsafeTarget);
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(SkillProposalStoreError::Io {
                    action: "inspect path containment",
                    source,
                });
            }
        }
    }
    Ok(())
}

fn reject_symlink_if_present(path: &Path) -> Result<(), SkillProposalStoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(SkillProposalStoreError::UnsafeTarget)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SkillProposalStoreError::Io {
            action: "inspect a proposal path",
            source,
        }),
    }
}

fn acquire_target_lock(
    scope: SkillProposalScope,
    skill_name: &str,
    cwd: &Path,
) -> Result<TargetLock, SkillProposalStoreError> {
    let owner = proposal_owner_dir(scope, cwd, true)?;
    let locks = secure_directory(&owner.join(".target-locks"))?;
    let key = content_digest(skill_name.as_bytes()).replace(':', "-");
    let path = locks.join(format!("{key}.lock"));
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", std::process::id());
            let _ = file.sync_all();
            Ok(TargetLock { path })
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            Err(SkillProposalStoreError::TargetBusy)
        }
        Err(source) => Err(SkillProposalStoreError::Io {
            action: "acquire a skill target lock",
            source,
        }),
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), SkillProposalStoreError> {
    let parent = path.parent().ok_or(SkillProposalStoreError::UnsafeTarget)?;
    secure_directory(parent)?;
    reject_symlink_if_present(path)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(SkillProposalStoreError::Json)?;
    let temporary = path.with_file_name(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("proposal"),
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| SkillProposalStoreError::Io {
                action: "create a temporary proposal file",
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| SkillProposalStoreError::Io {
                action: "flush a temporary proposal file",
                source,
            })?;
        reject_symlink_if_present(path)?;
        std::fs::rename(&temporary, path).map_err(|source| SkillProposalStoreError::Io {
            action: "atomically replace a proposal file",
            source,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, SkillProposalStoreError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(SkillProposalStoreError::NotFound);
        }
        Err(source) => {
            return Err(SkillProposalStoreError::Io {
                action: "inspect a proposal file",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SkillProposalStoreError::UnsafeTarget);
    }
    if metadata.len() > max_bytes {
        return Err(SkillProposalStoreError::ProposalTooLarge);
    }
    let file = File::open(path).map_err(|source| SkillProposalStoreError::Io {
        action: "open a proposal file",
        source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| SkillProposalStoreError::Io {
            action: "read a proposal file",
            source,
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(SkillProposalStoreError::ProposalTooLarge);
    }
    Ok(bytes)
}

fn relative_target(scope: SkillProposalScope, skill_name: &str) -> String {
    match scope {
        SkillProposalScope::User => format!("skills/{skill_name}/SKILL.md"),
        SkillProposalScope::Project => format!(".allthecodes/skills/{skill_name}/SKILL.md"),
    }
}

fn bound_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SkillProposalDraft;

    fn project(root: &Path) -> PathBuf {
        let cwd = root.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        cwd
    }

    fn draft(name: &str) -> SkillProposalDraft {
        SkillProposalDraft {
            action: SkillProposalAction::Create,
            scope: SkillProposalScope::Project,
            skill_name: name.to_string(),
            source_session_id: Some("session-1".to_string()),
            markdown: format!("---\ndescription: {name} skill.\n---\nUse {name}."),
        }
    }

    #[test]
    fn claim_is_exclusive_and_drop_restores_pending_proposal() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let proposal = crate::stage_skill_proposal(draft("claim-test"), &cwd).unwrap();

        let claim = claim_skill_proposal(&proposal.id, SkillProposalScope::Project, &cwd).unwrap();
        assert!(matches!(
            claim_skill_proposal(&proposal.id, SkillProposalScope::Project, &cwd),
            Err(SkillProposalStoreError::AlreadyClaimed)
        ));
        drop(claim);
        assert_eq!(
            load_skill_proposal_in_scope(&proposal.id, SkillProposalScope::Project, &cwd).unwrap(),
            proposal
        );
    }

    #[test]
    fn commit_is_single_use_and_writes_bounded_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let proposal = crate::stage_skill_proposal(draft("receipt-test"), &cwd).unwrap();
        let claim = claim_skill_proposal(&proposal.id, SkillProposalScope::Project, &cwd).unwrap();
        let receipt = claim
            .commit(SkillProposalDisposition::Rejected, None)
            .unwrap();

        assert_eq!(receipt.skill_name, "receipt-test");
        assert!(matches!(
            claim_skill_proposal(&proposal.id, SkillProposalScope::Project, &cwd),
            Err(SkillProposalStoreError::AlreadyDecided(_))
        ));
    }

    #[test]
    fn pending_list_isolates_corrupt_and_oversized_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let proposal = crate::stage_skill_proposal(draft("valid-pending"), &cwd).unwrap();
        let owner = crate::skill_proposals_dir(SkillProposalScope::Project, &cwd);
        std::fs::write(owner.join("corrupt.json"), b"{not-json").unwrap();
        std::fs::write(
            owner.join("oversized.json"),
            vec![b'x'; (MAX_PROPOSAL_FILE_BYTES + 1) as usize],
        )
        .unwrap();

        let listed = list_pending_skill_proposals(SkillProposalScope::Project, &cwd).unwrap();

        assert_eq!(listed.proposals, vec![proposal]);
        assert_eq!(listed.diagnostic_count, 2);
        assert!(!listed.truncated);
    }

    #[test]
    fn install_rejects_symlinked_skill_directory() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let proposal = crate::stage_skill_proposal(draft("unsafe-target"), &cwd).unwrap();
        let skills_root = crate::skills_root_for_scope(SkillProposalScope::Project, &cwd);
        std::fs::create_dir_all(&skills_root).unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, skills_root.join("unsafe-target")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, skills_root.join("unsafe-target")).unwrap();

        assert!(matches!(
            install_skill_proposal(&proposal, &cwd),
            Err(SkillProposalStoreError::UnsafeTarget)
        ));
        assert!(!outside.join("SKILL.md").exists());
    }

    #[test]
    fn patch_requires_existing_regular_target_and_changes_digest() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let mut proposal = crate::stage_skill_proposal(draft("patch-target"), &cwd).unwrap();
        let target = proposal.proposed_path.clone();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "---\ndescription: Old skill.\n---\nOld body.").unwrap();
        proposal.action = SkillProposalAction::Patch;
        proposal.markdown = "---\ndescription: New skill.\n---\nNew body.".to_string();
        let before = inspect_skill_proposal_target(&proposal, &cwd).unwrap();
        let after = install_skill_proposal(&proposal, &cwd).unwrap();

        assert_ne!(before.digest.as_deref(), Some(after.as_str()));
        assert_eq!(std::fs::read_to_string(target).unwrap(), proposal.markdown);
    }

    #[test]
    fn malformed_or_oversized_markdown_is_not_installable() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = project(temp.path());
        let mut proposal = crate::stage_skill_proposal(draft("invalid-markdown"), &cwd).unwrap();
        proposal.markdown = "---\ndescription broken".to_string();
        assert!(!validate_skill_proposal(&proposal, &cwd).unwrap().actionable);

        proposal.markdown = "x".repeat(MAX_PROPOSAL_MARKDOWN_BYTES + 1);
        assert!(matches!(
            validate_skill_proposal(&proposal, &cwd),
            Err(SkillProposalStoreError::ProposalTooLarge)
        ));
    }
}
