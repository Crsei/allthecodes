//! Shared native/background-review skill proposal projection and disposition.
//!
//! The two existing stores remain canonical. This facade provides one strict,
//! namespace-qualified view for commands and Web adapters without copying a
//! proposal into a third pending queue.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;

use super::background_review::{
    self, BackgroundReviewDecisionError, BackgroundReviewDisposition, BackgroundReviewProposal,
    BackgroundReviewProposalKind,
};
use allthecodes_skills::proposals::{
    self as native_store, SkillProposalStoreError, MAX_PROPOSAL_MARKDOWN_BYTES,
};
use allthecodes_skills::{SkillProposal, SkillProposalAction, SkillProposalScope};

pub const DEFAULT_PROPOSAL_LIMIT: usize = 50;
pub const MAX_PROPOSAL_LIMIT: usize = 100;
pub const MAX_DIFF_BYTES: usize = 64 * 1024;
const MAX_PUBLIC_SUMMARY_CHARS: usize = 240;
const MAX_RECEIPT_BYTES: u64 = 32 * 1024;
const MAX_BACKGROUND_PROPOSAL_FILE_BYTES: u64 = 512 * 1024;
const MAX_BACKGROUND_PROPOSAL_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSource {
    Native,
    BackgroundReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalAction {
    Create,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalScope {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalDisposition {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalValidationState {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalSummary {
    pub proposal_id: String,
    pub source: ProposalSource,
    pub action: ProposalAction,
    pub scope: ProposalScope,
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    pub created_at: String,
    pub relative_target: String,
    pub proposal_digest: String,
    pub validation_state: ProposalValidationState,
    pub actionable: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDetail {
    #[serde(flatten)]
    pub summary: ProposalSummary,
    pub markdown: String,
    pub markdown_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_target_digest: Option<String>,
    pub target_exists: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ProposalDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDiff {
    pub proposal_id: String,
    pub proposal_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_digest: Option<String>,
    pub diff: String,
    pub truncated: bool,
    pub untruncated_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProposalListFilter {
    pub source: Option<ProposalSource>,
    pub scope: Option<ProposalScope>,
    pub action: Option<ProposalAction>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalList {
    pub proposals: Vec<ProposalSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub diagnostic_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExpectedTarget {
    Absent,
    Digest { digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalMutationRequest {
    pub request_id: String,
    pub expected_proposal_digest: String,
    pub expected_target: ExpectedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalMutationOutcome {
    pub proposal_id: String,
    pub request_id: String,
    pub disposition: ProposalDisposition,
    pub skill_name: String,
    pub scope: ProposalScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_target_digest: Option<String>,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalAccess {
    pub project: bool,
    pub user: bool,
}

impl ProposalAccess {
    pub const TRUSTED_LOCAL: Self = Self {
        project: true,
        user: true,
    };
}

#[derive(Debug, thiserror::Error)]
pub enum SkillProposalServiceError {
    #[error("invalid skill proposal request: {message}")]
    Invalid { code: &'static str, message: String },
    #[error("skill proposal scope is forbidden")]
    Forbidden,
    #[error("skill proposal not found")]
    NotFound,
    #[error("skill proposal changed")]
    ProposalChanged,
    #[error("skill target changed")]
    TargetChanged,
    #[error("skill proposal was already consumed")]
    ProposalConsumed,
    #[error("skill proposal is not actionable")]
    NotActionable,
    #[error("skill proposal content is too large")]
    ProposalTooLarge,
    #[error("skill proposal store is unavailable")]
    StoreUnavailable,
}

impl SkillProposalServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { code, .. } => code,
            Self::Forbidden => "proposal_scope_forbidden",
            Self::NotFound => "proposal_not_found",
            Self::ProposalChanged => "proposal_changed",
            Self::TargetChanged => "target_changed",
            Self::ProposalConsumed => "proposal_consumed",
            Self::NotActionable => "proposal_not_actionable",
            Self::ProposalTooLarge => "proposal_too_large",
            Self::StoreUnavailable => "proposal_store_unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillProposalService {
    workspace: PathBuf,
    access: ProposalAccess,
}

#[derive(Debug, Clone)]
struct Projection {
    source: ProposalSource,
    proposal: SkillProposal,
    public_id: String,
    summary: String,
    workspace_trusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MutationReceipt {
    proposal_id: String,
    request_id: String,
    expected_proposal_digest: String,
    expected_target: ExpectedTarget,
    operation: ProposalDisposition,
    outcome: ProposalMutationOutcome,
}

#[derive(Debug)]
struct BackgroundProposalScan {
    proposals: Vec<BackgroundReviewProposal>,
    diagnostic_count: usize,
    truncated: bool,
}

impl SkillProposalService {
    pub fn new(workspace: impl Into<PathBuf>, access: ProposalAccess) -> Self {
        Self {
            workspace: workspace.into(),
            access,
        }
    }

    pub fn trusted_local(workspace: impl Into<PathBuf>) -> Self {
        Self::new(workspace, ProposalAccess::TRUSTED_LOCAL)
    }

    pub fn list(
        &self,
        filter: ProposalListFilter,
    ) -> Result<ProposalList, SkillProposalServiceError> {
        let limit = filter.limit.unwrap_or(DEFAULT_PROPOSAL_LIMIT);
        if limit == 0 || limit > MAX_PROPOSAL_LIMIT {
            return Err(SkillProposalServiceError::Invalid {
                code: "invalid_limit",
                message: format!("limit must be between 1 and {MAX_PROPOSAL_LIMIT}"),
            });
        }

        let mut projections = Vec::new();
        let mut diagnostic_count = 0usize;
        let mut source_truncated = false;
        if filter
            .source
            .is_none_or(|source| source == ProposalSource::Native)
        {
            for scope in [SkillProposalScope::Project, SkillProposalScope::User] {
                if !self.can_access(public_scope(scope)) {
                    continue;
                }
                let native = native_store::list_pending_skill_proposals(scope, &self.workspace)
                    .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
                diagnostic_count = diagnostic_count.saturating_add(native.diagnostic_count);
                source_truncated |= native.truncated;
                projections.extend(native.proposals.into_iter().map(|proposal| Projection {
                    public_id: native_id(proposal.scope, &proposal.id),
                    summary: format!(
                        "{} skill '{}'",
                        action_label(proposal.action),
                        proposal.skill_name
                    ),
                    source: ProposalSource::Native,
                    proposal,
                    workspace_trusted: true,
                }));
            }
        }
        if filter
            .source
            .is_none_or(|source| source == ProposalSource::BackgroundReview)
        {
            let review = scan_background_review_proposals()?;
            diagnostic_count = diagnostic_count.saturating_add(review.diagnostic_count);
            source_truncated |= review.truncated;
            for proposal in review
                .proposals
                .into_iter()
                .filter(is_background_skill_proposal)
            {
                match self.project_background(proposal) {
                    Ok(projection) if self.can_access(public_scope(projection.proposal.scope)) => {
                        projections.push(projection);
                    }
                    Ok(_) => {}
                    Err(SkillProposalServiceError::NotActionable) => {
                        diagnostic_count = diagnostic_count.saturating_add(1);
                    }
                    Err(error) => return Err(error),
                }
            }
        }

        projections.retain(|projection| {
            filter
                .scope
                .is_none_or(|scope| scope == public_scope(projection.proposal.scope))
                && filter
                    .action
                    .is_none_or(|action| action == public_action(projection.proposal.action))
        });
        projections.sort_by(|left, right| {
            right
                .proposal
                .created_at
                .cmp(&left.proposal.created_at)
                .then_with(|| left.public_id.cmp(&right.public_id))
        });

        let start = match filter.cursor.as_deref() {
            None => 0,
            Some(cursor) => projections
                .iter()
                .position(|projection| projection.public_id == cursor)
                .map(|index| index + 1)
                .ok_or_else(|| SkillProposalServiceError::Invalid {
                    code: "invalid_cursor",
                    message: "cursor does not identify a visible proposal".to_string(),
                })?,
        };
        let remaining = projections.len().saturating_sub(start);
        let truncated = source_truncated || remaining > limit;
        let selected = projections.into_iter().skip(start).take(limit);
        let mut summaries = Vec::new();
        for projection in selected {
            match self.detail_from_projection(projection) {
                Ok(detail) => {
                    diagnostic_count += detail.diagnostics.len();
                    summaries.push(detail.summary);
                }
                Err(SkillProposalServiceError::ProposalTooLarge) => {
                    diagnostic_count += 1;
                }
                Err(error) => return Err(error),
            }
        }
        let next_cursor = truncated
            .then(|| summaries.last().map(|summary| summary.proposal_id.clone()))
            .flatten();
        Ok(ProposalList {
            proposals: summaries,
            next_cursor,
            diagnostic_count,
            truncated,
        })
    }

    pub fn detail(&self, public_id: &str) -> Result<ProposalDetail, SkillProposalServiceError> {
        self.detail_from_projection(self.resolve(public_id)?)
    }

    pub fn diff(&self, public_id: &str) -> Result<ProposalDiff, SkillProposalServiceError> {
        let projection = self.resolve(public_id)?;
        let detail = self.detail_from_projection(projection.clone())?;
        let (baseline, snapshot) = match projection.proposal.action {
            SkillProposalAction::Create => (
                String::new(),
                native_store::inspect_skill_proposal_target(&projection.proposal, &self.workspace)
                    .map_err(map_native_store_error)?,
            ),
            SkillProposalAction::Patch => {
                native_store::read_skill_proposal_baseline(&projection.proposal, &self.workspace)
                    .map_err(map_native_store_error)?
            }
        };
        let full = TextDiff::from_lines(&baseline, &detail.markdown)
            .unified_diff()
            .context_radius(3)
            .header("a/SKILL.md", "b/SKILL.md")
            .to_string();
        let untruncated_bytes = full.len();
        let (diff, truncated) = truncate_utf8_bytes(&full, MAX_DIFF_BYTES);
        Ok(ProposalDiff {
            proposal_id: public_id.to_string(),
            proposal_digest: detail.summary.proposal_digest,
            baseline_digest: snapshot.digest,
            diff,
            truncated,
            untruncated_bytes,
        })
    }

    pub fn approve(
        &self,
        public_id: &str,
        request: ProposalMutationRequest,
    ) -> Result<ProposalMutationOutcome, SkillProposalServiceError> {
        self.decide(public_id, ProposalDisposition::Approved, request)
    }

    pub fn reject(
        &self,
        public_id: &str,
        request: ProposalMutationRequest,
    ) -> Result<ProposalMutationOutcome, SkillProposalServiceError> {
        self.decide(public_id, ProposalDisposition::Rejected, request)
    }

    pub fn decide_current(
        &self,
        public_id: &str,
        disposition: ProposalDisposition,
        request_id: impl Into<String>,
    ) -> Result<ProposalMutationOutcome, SkillProposalServiceError> {
        let detail = self.detail(public_id)?;
        let expected_target = match detail.summary.action {
            ProposalAction::Create => ExpectedTarget::Absent,
            ProposalAction::Patch => ExpectedTarget::Digest {
                digest: detail
                    .current_target_digest
                    .clone()
                    .ok_or(SkillProposalServiceError::NotActionable)?,
            },
        };
        self.decide(
            public_id,
            disposition,
            ProposalMutationRequest {
                request_id: request_id.into(),
                expected_proposal_digest: detail.summary.proposal_digest,
                expected_target,
            },
        )
    }

    fn decide(
        &self,
        public_id: &str,
        disposition: ProposalDisposition,
        request: ProposalMutationRequest,
    ) -> Result<ProposalMutationOutcome, SkillProposalServiceError> {
        validate_mutation_request(&request)?;
        if let Some(receipt) = self.load_receipt(public_id, disposition, &request.request_id)? {
            if receipt.expected_proposal_digest != request.expected_proposal_digest
                || receipt.expected_target != request.expected_target
            {
                return Err(SkillProposalServiceError::Invalid {
                    code: "idempotency_mismatch",
                    message: "request_id was already used with different preconditions".to_string(),
                });
            }
            let mut outcome = receipt.outcome;
            outcome.idempotent_replay = true;
            return Ok(outcome);
        }

        let detail = self.detail(public_id)?;
        self.authorize(detail.summary.scope)?;
        if !detail.summary.actionable {
            return Err(SkillProposalServiceError::NotActionable);
        }
        if detail.summary.proposal_digest != request.expected_proposal_digest {
            return Err(SkillProposalServiceError::ProposalChanged);
        }
        validate_expected_target(&detail, &request.expected_target)?;

        let outcome = match decode_id(public_id)? {
            DecodedId::Native { scope, raw_id } => {
                let claim = native_store::claim_skill_proposal(&raw_id, scope, &self.workspace)
                    .map_err(|error| {
                        self.map_native_claim_error(error, public_id, disposition, &request)
                    })?;
                let claimed = Projection {
                    source: ProposalSource::Native,
                    public_id: public_id.to_string(),
                    summary: format!(
                        "{} skill '{}'",
                        action_label(claim.proposal().action),
                        claim.proposal().skill_name
                    ),
                    proposal: claim.proposal().clone(),
                    workspace_trusted: true,
                };
                let claimed_detail = self.detail_from_projection(claimed)?;
                revalidate_claimed(&claimed_detail, &request)?;
                let target_digest = match disposition {
                    ProposalDisposition::Approved => Some(
                        native_store::install_skill_proposal(claim.proposal(), &self.workspace)
                            .map_err(map_native_store_error)?,
                    ),
                    ProposalDisposition::Rejected => claimed_detail.current_target_digest.clone(),
                };
                let proposal = claim.proposal().clone();
                claim
                    .commit(native_disposition(disposition), target_digest.clone())
                    .map_err(map_native_store_error)?;
                mutation_outcome(
                    public_id,
                    &request.request_id,
                    disposition,
                    &proposal,
                    target_digest,
                )
            }
            DecodedId::Background { raw_id } => {
                let claim = background_review::claim_background_review_proposal(&raw_id)
                    .map_err(map_background_claim_error)?;
                if !is_background_skill_proposal(claim.proposal()) {
                    return Err(SkillProposalServiceError::NotFound);
                }
                let claimed = self.project_background(claim.proposal().clone())?;
                let claimed_detail = self.detail_from_projection(claimed.clone())?;
                revalidate_claimed(&claimed_detail, &request)?;
                let target_digest = match disposition {
                    ProposalDisposition::Approved => Some(
                        native_store::install_skill_proposal(&claimed.proposal, &self.workspace)
                            .map_err(map_native_store_error)?,
                    ),
                    ProposalDisposition::Rejected => claimed_detail.current_target_digest.clone(),
                };
                claim
                    .commit(background_disposition(disposition))
                    .map_err(map_background_claim_error)?;
                mutation_outcome(
                    public_id,
                    &request.request_id,
                    disposition,
                    &claimed.proposal,
                    target_digest,
                )
            }
        };

        self.write_receipt(MutationReceipt {
            proposal_id: public_id.to_string(),
            request_id: request.request_id,
            expected_proposal_digest: request.expected_proposal_digest,
            expected_target: request.expected_target,
            operation: disposition,
            outcome: outcome.clone(),
        })?;
        Ok(outcome)
    }

    fn resolve(&self, public_id: &str) -> Result<Projection, SkillProposalServiceError> {
        match decode_id(public_id)? {
            DecodedId::Native { scope, raw_id } => {
                self.authorize(public_scope(scope))?;
                let proposal =
                    native_store::load_skill_proposal_in_scope(&raw_id, scope, &self.workspace)
                        .map_err(map_native_store_error)?;
                Ok(Projection {
                    public_id: public_id.to_string(),
                    summary: format!(
                        "{} skill '{}'",
                        action_label(proposal.action),
                        proposal.skill_name
                    ),
                    source: ProposalSource::Native,
                    proposal,
                    workspace_trusted: true,
                })
            }
            DecodedId::Background { raw_id } => {
                let proposal = load_background_review_proposal_bounded(&raw_id)?;
                if !is_background_skill_proposal(&proposal) {
                    return Err(SkillProposalServiceError::NotFound);
                }
                let projection = self.project_background(proposal)?;
                self.authorize(public_scope(projection.proposal.scope))?;
                Ok(projection)
            }
        }
    }

    fn project_background(
        &self,
        source: BackgroundReviewProposal,
    ) -> Result<Projection, SkillProposalServiceError> {
        if !is_background_skill_proposal(&source) {
            return Err(SkillProposalServiceError::NotFound);
        }
        let payload = source.payload.get("skill").unwrap_or(&source.payload);
        let skill_name = payload
            .get("skill_name")
            .and_then(serde_json::Value::as_str)
            .ok_or(SkillProposalServiceError::NotActionable)?
            .to_string();
        let markdown = payload
            .get("markdown")
            .and_then(serde_json::Value::as_str)
            .ok_or(SkillProposalServiceError::NotActionable)?
            .to_string();
        let scope = match payload
            .get("scope")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("project")
        {
            "user" => SkillProposalScope::User,
            "project" => SkillProposalScope::Project,
            _ => return Err(SkillProposalServiceError::NotActionable),
        };
        let action = match source.kind {
            BackgroundReviewProposalKind::SkillCreate => SkillProposalAction::Create,
            BackgroundReviewProposalKind::SkillPatch => SkillProposalAction::Patch,
            _ => return Err(SkillProposalServiceError::NotFound),
        };
        let proposed_path = skill_target(scope, &self.workspace, &skill_name);
        let workspace_trusted = scope == SkillProposalScope::User
            || background_workspace(&source)
                .is_some_and(|workspace| same_workspace(workspace, &self.workspace));
        let public_id = background_id(&source.id);
        // Native target validation intentionally has a narrower identifier
        // alphabet. Keep the canonical background id only in `public_id` and
        // project it to a deterministic native-safe transaction id.
        let internal_id = format!("background-{}", &hash_text(&source.id)[..32]);
        let summary = bound_text(&source.summary, MAX_PUBLIC_SUMMARY_CHARS);
        Ok(Projection {
            source: ProposalSource::BackgroundReview,
            proposal: SkillProposal {
                id: internal_id,
                action,
                scope,
                skill_name,
                source_session_id: Some(source.source_session_id),
                proposed_path,
                markdown,
                created_at: source.created_at.to_rfc3339(),
            },
            public_id,
            summary,
            workspace_trusted,
        })
    }

    fn detail_from_projection(
        &self,
        projection: Projection,
    ) -> Result<ProposalDetail, SkillProposalServiceError> {
        if projection.proposal.markdown.len() > MAX_PROPOSAL_MARKDOWN_BYTES {
            return Err(SkillProposalServiceError::ProposalTooLarge);
        }
        let digest = sourced_digest(projection.source, &projection.proposal);
        let mut diagnostics = Vec::new();
        let mut actionable = projection.workspace_trusted;
        if !projection.workspace_trusted {
            diagnostics.push(ProposalDiagnostic {
                code: "untrusted-workspace".to_string(),
                message: "Project provenance does not match the active workspace.".to_string(),
            });
        }

        match native_store::validate_skill_proposal(&projection.proposal, &self.workspace) {
            Ok(validation) => {
                actionable &= validation.actionable;
                diagnostics.extend(validation.diagnostics.into_iter().map(|diagnostic| {
                    ProposalDiagnostic {
                        code: diagnostic.code,
                        message: diagnostic.message,
                    }
                }));
            }
            Err(SkillProposalStoreError::ProposalTooLarge) => {
                return Err(SkillProposalServiceError::ProposalTooLarge);
            }
            Err(
                SkillProposalStoreError::InvalidProposal(_) | SkillProposalStoreError::UnsafeTarget,
            ) => {
                actionable = false;
                diagnostics.push(ProposalDiagnostic {
                    code: "unsafe-proposal".to_string(),
                    message: "Proposal metadata or target is unsafe.".to_string(),
                });
            }
            Err(error) => return Err(map_native_store_error(error)),
        }

        let (target_exists, target_digest) = match native_store::inspect_skill_proposal_target(
            &projection.proposal,
            &self.workspace,
        ) {
            Ok(snapshot) => (snapshot.exists, snapshot.digest),
            Err(SkillProposalStoreError::UnsafeTarget) => {
                actionable = false;
                diagnostics.push(ProposalDiagnostic {
                    code: "unsafe-target".to_string(),
                    message: "Skill target fails path containment checks.".to_string(),
                });
                (false, None)
            }
            Err(error) => return Err(map_native_store_error(error)),
        };
        match projection.proposal.action {
            SkillProposalAction::Create if target_exists => {
                actionable = false;
                diagnostics.push(ProposalDiagnostic {
                    code: "target-exists".to_string(),
                    message: "Create proposal target already exists.".to_string(),
                });
            }
            SkillProposalAction::Patch if !target_exists => {
                actionable = false;
                diagnostics.push(ProposalDiagnostic {
                    code: "target-missing".to_string(),
                    message: "Patch proposal target does not exist.".to_string(),
                });
            }
            _ => {}
        }
        diagnostics.truncate(20);
        let validation_state = if actionable {
            ProposalValidationState::Valid
        } else {
            ProposalValidationState::Invalid
        };
        Ok(ProposalDetail {
            summary: ProposalSummary {
                proposal_id: projection.public_id,
                source: projection.source,
                action: public_action(projection.proposal.action),
                scope: public_scope(projection.proposal.scope),
                skill_name: bound_text(&projection.proposal.skill_name, 128),
                source_session_id: projection
                    .proposal
                    .source_session_id
                    .map(|value| bound_text(&value, 160)),
                created_at: projection.proposal.created_at,
                relative_target: relative_target(
                    projection.proposal.scope,
                    &projection.proposal.skill_name,
                ),
                proposal_digest: digest,
                validation_state,
                actionable,
                summary: projection.summary,
            },
            markdown_bytes: projection.proposal.markdown.len(),
            markdown: projection.proposal.markdown,
            current_target_digest: target_digest,
            target_exists,
            diagnostics,
        })
    }

    fn authorize(&self, scope: ProposalScope) -> Result<(), SkillProposalServiceError> {
        self.can_access(scope)
            .then_some(())
            .ok_or(SkillProposalServiceError::Forbidden)
    }

    fn can_access(&self, scope: ProposalScope) -> bool {
        match scope {
            ProposalScope::Project => self.access.project,
            ProposalScope::User => self.access.user,
        }
    }

    fn receipt_path(
        &self,
        public_id: &str,
        disposition: ProposalDisposition,
        request_id: &str,
    ) -> PathBuf {
        let workspace = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());
        let key = format!(
            "{}\0{public_id}\0{}\0{request_id}",
            workspace.display(),
            disposition_label(disposition)
        );
        allthecodes_config::paths::data_root()
            .join("skill_proposal_receipts")
            .join(format!("{}.json", hash_text(&key)))
    }

    fn load_receipt(
        &self,
        public_id: &str,
        disposition: ProposalDisposition,
        request_id: &str,
    ) -> Result<Option<MutationReceipt>, SkillProposalServiceError> {
        let path = self.receipt_path(public_id, disposition, request_id);
        read_receipt(&path)
    }

    fn write_receipt(&self, receipt: MutationReceipt) -> Result<(), SkillProposalServiceError> {
        let path = self.receipt_path(&receipt.proposal_id, receipt.operation, &receipt.request_id);
        write_receipt(&path, &receipt)
    }

    fn map_native_claim_error(
        &self,
        error: SkillProposalStoreError,
        public_id: &str,
        disposition: ProposalDisposition,
        request: &ProposalMutationRequest,
    ) -> SkillProposalServiceError {
        if matches!(
            error,
            SkillProposalStoreError::AlreadyClaimed | SkillProposalStoreError::AlreadyDecided(_)
        ) {
            for _ in 0..20 {
                if self
                    .load_receipt(public_id, disposition, &request.request_id)
                    .ok()
                    .flatten()
                    .is_some()
                {
                    // The caller will retry and receive the durable replay.
                    return SkillProposalServiceError::ProposalConsumed;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        map_native_store_error(error)
    }
}

#[derive(Debug)]
enum DecodedId {
    Native {
        scope: SkillProposalScope,
        raw_id: String,
    },
    Background {
        raw_id: String,
    },
}

fn decode_id(value: &str) -> Result<DecodedId, SkillProposalServiceError> {
    if let Some(raw_id) = value.strip_prefix("native:project:") {
        if native_store::valid_proposal_id(raw_id) {
            return Ok(DecodedId::Native {
                scope: SkillProposalScope::Project,
                raw_id: raw_id.to_string(),
            });
        }
    }
    if let Some(raw_id) = value.strip_prefix("native:user:") {
        if native_store::valid_proposal_id(raw_id) {
            return Ok(DecodedId::Native {
                scope: SkillProposalScope::User,
                raw_id: raw_id.to_string(),
            });
        }
    }
    if let Some(raw_id) = value.strip_prefix("background_review:") {
        if valid_background_proposal_id(raw_id) {
            return Ok(DecodedId::Background {
                raw_id: raw_id.to_string(),
            });
        }
    }
    Err(SkillProposalServiceError::Invalid {
        code: "invalid_proposal",
        message: "proposal_id must include a valid owner namespace".to_string(),
    })
}

fn native_id(scope: SkillProposalScope, raw_id: &str) -> String {
    match scope {
        SkillProposalScope::Project => format!("native:project:{raw_id}"),
        SkillProposalScope::User => format!("native:user:{raw_id}"),
    }
}

fn background_id(raw_id: &str) -> String {
    format!("background_review:{raw_id}")
}

fn valid_background_proposal_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn scan_background_review_proposals() -> Result<BackgroundProposalScan, SkillProposalServiceError> {
    let directory = background_review::review_proposals_dir();
    reject_background_symlink_components(&directory)?;
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SkillProposalServiceError::StoreUnavailable);
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(BackgroundProposalScan {
                proposals: Vec::new(),
                diagnostic_count: 0,
                truncated: false,
            });
        }
        Err(_) => return Err(SkillProposalServiceError::StoreUnavailable),
    }

    let entries =
        std::fs::read_dir(&directory).map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
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
        if scanned == MAX_BACKGROUND_PROPOSAL_ENTRIES {
            truncated = true;
            break;
        }
        scanned += 1;
        match read_background_review_proposal_file(&path, None) {
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
    Ok(BackgroundProposalScan {
        proposals,
        diagnostic_count,
        truncated,
    })
}

fn load_background_review_proposal_bounded(
    id: &str,
) -> Result<BackgroundReviewProposal, SkillProposalServiceError> {
    if !valid_background_proposal_id(id) {
        return Err(SkillProposalServiceError::Invalid {
            code: "invalid_proposal",
            message: "background proposal id is invalid".to_string(),
        });
    }
    read_background_review_proposal_file(&background_review::proposal_path(id), Some(id))
}

fn read_background_review_proposal_file(
    path: &Path,
    expected_id: Option<&str>,
) -> Result<BackgroundReviewProposal, SkillProposalServiceError> {
    reject_background_symlink_components(path)?;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(SkillProposalServiceError::NotFound);
        }
        Err(_) => return Err(SkillProposalServiceError::StoreUnavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SkillProposalServiceError::StoreUnavailable);
    }
    if metadata.len() > MAX_BACKGROUND_PROPOSAL_FILE_BYTES {
        return Err(SkillProposalServiceError::ProposalTooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(MAX_BACKGROUND_PROPOSAL_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
    if bytes.len() as u64 > MAX_BACKGROUND_PROPOSAL_FILE_BYTES {
        return Err(SkillProposalServiceError::ProposalTooLarge);
    }
    let proposal: BackgroundReviewProposal =
        serde_json::from_slice(&bytes).map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
    if !valid_background_proposal_id(&proposal.id)
        || expected_id.is_some_and(|expected| expected != proposal.id.as_str())
        || background_review::proposal_path(&proposal.id) != path
    {
        return Err(SkillProposalServiceError::StoreUnavailable);
    }
    Ok(proposal)
}

fn reject_background_symlink_components(path: &Path) -> Result<(), SkillProposalServiceError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(SkillProposalServiceError::StoreUnavailable);
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(SkillProposalServiceError::StoreUnavailable),
        }
    }
    Ok(())
}

fn is_background_skill_proposal(proposal: &BackgroundReviewProposal) -> bool {
    matches!(
        proposal.kind,
        BackgroundReviewProposalKind::SkillCreate | BackgroundReviewProposalKind::SkillPatch
    )
}

fn background_workspace(proposal: &BackgroundReviewProposal) -> Option<&str> {
    let skill = proposal.payload.get("skill").unwrap_or(&proposal.payload);
    skill
        .get("cwd")
        .or_else(|| skill.get("workspace"))
        .or_else(|| proposal.payload.get("cwd"))
        .or_else(|| proposal.payload.get("workspace"))
        .and_then(serde_json::Value::as_str)
}

fn same_workspace(candidate: &str, active: &Path) -> bool {
    let candidate = Path::new(candidate);
    match (candidate.canonicalize(), active.canonicalize()) {
        (Ok(candidate), Ok(active)) => candidate == active,
        _ => candidate == active,
    }
}

fn skill_target(scope: SkillProposalScope, cwd: &Path, skill_name: &str) -> PathBuf {
    match scope {
        SkillProposalScope::User => allthecodes_config::paths::skills_dir_global(),
        SkillProposalScope::Project => allthecodes_config::paths::project_skills_dir(cwd),
    }
    .join(skill_name)
    .join("SKILL.md")
}

fn relative_target(scope: SkillProposalScope, skill_name: &str) -> String {
    match scope {
        SkillProposalScope::User => format!("skills/{skill_name}/SKILL.md"),
        SkillProposalScope::Project => format!(".allthecodes/skills/{skill_name}/SKILL.md"),
    }
}

fn public_source_digest(source: ProposalSource, proposal: &SkillProposal) -> String {
    let owner = match source {
        ProposalSource::Native => "native",
        ProposalSource::BackgroundReview => "background_review",
    };
    hash_text(&format!(
        "{owner}\0{}",
        native_store::proposal_digest(proposal)
    ))
}

fn sourced_digest(source: ProposalSource, proposal: &SkillProposal) -> String {
    format!("sha256:{}", public_source_digest(source, proposal))
}

fn hash_text(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn public_action(action: SkillProposalAction) -> ProposalAction {
    match action {
        SkillProposalAction::Create => ProposalAction::Create,
        SkillProposalAction::Patch => ProposalAction::Patch,
    }
}

fn public_scope(scope: SkillProposalScope) -> ProposalScope {
    match scope {
        SkillProposalScope::User => ProposalScope::User,
        SkillProposalScope::Project => ProposalScope::Project,
    }
}

fn native_disposition(disposition: ProposalDisposition) -> native_store::SkillProposalDisposition {
    match disposition {
        ProposalDisposition::Approved => native_store::SkillProposalDisposition::Approved,
        ProposalDisposition::Rejected => native_store::SkillProposalDisposition::Rejected,
    }
}

fn background_disposition(disposition: ProposalDisposition) -> BackgroundReviewDisposition {
    match disposition {
        ProposalDisposition::Approved => BackgroundReviewDisposition::Approved,
        ProposalDisposition::Rejected => BackgroundReviewDisposition::Rejected,
    }
}

fn mutation_outcome(
    public_id: &str,
    request_id: &str,
    disposition: ProposalDisposition,
    proposal: &SkillProposal,
    target_digest: Option<String>,
) -> ProposalMutationOutcome {
    ProposalMutationOutcome {
        proposal_id: public_id.to_string(),
        request_id: request_id.to_string(),
        disposition,
        skill_name: proposal.skill_name.clone(),
        scope: public_scope(proposal.scope),
        resulting_target_digest: target_digest,
        idempotent_replay: false,
    }
}

fn action_label(action: SkillProposalAction) -> &'static str {
    match action {
        SkillProposalAction::Create => "Create",
        SkillProposalAction::Patch => "Patch",
    }
}

fn disposition_label(disposition: ProposalDisposition) -> &'static str {
    match disposition {
        ProposalDisposition::Approved => "approve",
        ProposalDisposition::Rejected => "reject",
    }
}

fn validate_mutation_request(
    request: &ProposalMutationRequest,
) -> Result<(), SkillProposalServiceError> {
    if request.request_id.is_empty()
        || request.request_id.len() > 160
        || !request
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(SkillProposalServiceError::Invalid {
            code: "invalid_request_id",
            message: "request_id contains unsupported characters".to_string(),
        });
    }
    if !valid_digest(&request.expected_proposal_digest) {
        return Err(SkillProposalServiceError::Invalid {
            code: "invalid_proposal_digest",
            message: "expected_proposal_digest must be a SHA-256 digest".to_string(),
        });
    }
    if let ExpectedTarget::Digest { digest } = &request.expected_target {
        if !valid_digest(digest) {
            return Err(SkillProposalServiceError::Invalid {
                code: "invalid_target_digest",
                message: "target digest must be a SHA-256 digest".to_string(),
            });
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn validate_expected_target(
    detail: &ProposalDetail,
    expected: &ExpectedTarget,
) -> Result<(), SkillProposalServiceError> {
    match (&detail.summary.action, expected) {
        (ProposalAction::Create, ExpectedTarget::Absent) if !detail.target_exists => Ok(()),
        (ProposalAction::Patch, ExpectedTarget::Digest { digest })
            if detail.current_target_digest.as_deref() == Some(digest.as_str()) =>
        {
            Ok(())
        }
        _ => Err(SkillProposalServiceError::TargetChanged),
    }
}

fn revalidate_claimed(
    detail: &ProposalDetail,
    request: &ProposalMutationRequest,
) -> Result<(), SkillProposalServiceError> {
    if !detail.summary.actionable {
        return Err(SkillProposalServiceError::NotActionable);
    }
    if detail.summary.proposal_digest != request.expected_proposal_digest {
        return Err(SkillProposalServiceError::ProposalChanged);
    }
    validate_expected_target(detail, &request.expected_target)
}

fn map_native_store_error(error: SkillProposalStoreError) -> SkillProposalServiceError {
    match error {
        SkillProposalStoreError::InvalidId => SkillProposalServiceError::Invalid {
            code: "invalid_proposal",
            message: "native proposal id is invalid".to_string(),
        },
        SkillProposalStoreError::NotFound | SkillProposalStoreError::ScopeMismatch => {
            SkillProposalServiceError::NotFound
        }
        SkillProposalStoreError::AlreadyClaimed
        | SkillProposalStoreError::AlreadyDecided(_)
        | SkillProposalStoreError::TargetBusy => SkillProposalServiceError::ProposalConsumed,
        SkillProposalStoreError::ProposalTooLarge | SkillProposalStoreError::TargetTooLarge => {
            SkillProposalServiceError::ProposalTooLarge
        }
        SkillProposalStoreError::InvalidProposal(_)
        | SkillProposalStoreError::UnsafeTarget
        | SkillProposalStoreError::TargetStateMismatch => SkillProposalServiceError::NotActionable,
        SkillProposalStoreError::Io { .. } | SkillProposalStoreError::Json(_) => {
            SkillProposalServiceError::StoreUnavailable
        }
    }
}

fn map_background_claim_error(error: BackgroundReviewDecisionError) -> SkillProposalServiceError {
    match error {
        BackgroundReviewDecisionError::InvalidId => SkillProposalServiceError::Invalid {
            code: "invalid_proposal",
            message: "background proposal id is invalid".to_string(),
        },
        BackgroundReviewDecisionError::NotFound | BackgroundReviewDecisionError::WrongDomain => {
            SkillProposalServiceError::NotFound
        }
        BackgroundReviewDecisionError::AlreadyClaimed
        | BackgroundReviewDecisionError::AlreadyDecided(_) => {
            SkillProposalServiceError::ProposalConsumed
        }
        BackgroundReviewDecisionError::InvalidPayload => SkillProposalServiceError::NotActionable,
        BackgroundReviewDecisionError::Io(_) => SkillProposalServiceError::StoreUnavailable,
    }
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

fn bound_text(value: &str, max_chars: usize) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\t') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut chars = value.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}

fn read_receipt(path: &Path) -> Result<Option<MutationReceipt>, SkillProposalServiceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SkillProposalServiceError::StoreUnavailable),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_RECEIPT_BYTES
    {
        return Err(SkillProposalServiceError::StoreUnavailable);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_RECEIPT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(SkillProposalServiceError::StoreUnavailable);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| SkillProposalServiceError::StoreUnavailable)
}

fn write_receipt(path: &Path, receipt: &MutationReceipt) -> Result<(), SkillProposalServiceError> {
    let parent = path
        .parent()
        .ok_or(SkillProposalServiceError::StoreUnavailable)?;
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SkillProposalServiceError::StoreUnavailable);
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            std::fs::create_dir_all(parent)
                .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
        }
        Err(_) => return Err(SkillProposalServiceError::StoreUnavailable),
    }
    let bytes =
        serde_json::to_vec(receipt).map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(SkillProposalServiceError::StoreUnavailable);
    }
    let temporary = path.with_file_name(format!(
        ".receipt-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| SkillProposalServiceError::StoreUnavailable)?;
        match std::fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let existing = read_receipt(path)?;
                if existing.as_ref() == Some(receipt) {
                    Ok(())
                } else {
                    Err(SkillProposalServiceError::StoreUnavailable)
                }
            }
            Err(_) => Err(SkillProposalServiceError::StoreUnavailable),
        }
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.old.take() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn stage(cwd: &Path, name: &str, action: SkillProposalAction) -> SkillProposal {
        allthecodes_skills::stage_skill_proposal(
            allthecodes_skills::SkillProposalDraft {
                action,
                scope: SkillProposalScope::Project,
                skill_name: name.to_string(),
                source_session_id: Some("session-1".to_string()),
                markdown: format!("---\ndescription: {name} proposal.\n---\nUse {name}."),
            },
            cwd,
        )
        .unwrap()
    }

    #[test]
    #[serial]
    fn native_create_lists_diffs_approves_and_replays() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let proposal = stage(&cwd, "native-review", SkillProposalAction::Create);
        let public_id = native_id(proposal.scope, &proposal.id);
        let service = SkillProposalService::trusted_local(&cwd);

        let listed = service.list(ProposalListFilter::default()).unwrap();
        assert_eq!(listed.proposals[0].proposal_id, public_id);
        assert!(listed.proposals[0].actionable);
        let detail = service.detail(&public_id).unwrap();
        assert!(!detail.summary.relative_target.starts_with('/'));
        let diff = service.diff(&public_id).unwrap();
        assert!(diff.diff.contains("native-review proposal"));

        let request = ProposalMutationRequest {
            request_id: "request-1".to_string(),
            expected_proposal_digest: detail.summary.proposal_digest,
            expected_target: ExpectedTarget::Absent,
        };
        let approved = service.approve(&public_id, request.clone()).unwrap();
        assert!(!approved.idempotent_replay);
        let replay = service.approve(&public_id, request).unwrap();
        assert!(replay.idempotent_replay);
        assert_eq!(
            approved.resulting_target_digest,
            replay.resulting_target_digest
        );
    }

    #[test]
    #[serial]
    fn workflow_warning_is_invisible_and_not_consumed() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let warning = background_review::stage_background_review_if_due(
            background_review::BackgroundReviewInput {
                source_session_id: "session-warning".to_string(),
                cwd: cwd.display().to_string(),
                turn_count: 1,
                replay_seq_start: None,
                replay_seq_end: None,
                recent_summary: "workflow warning".to_string(),
                tool_errors: Vec::new(),
                similar_session_hits: Vec::new(),
            },
            &background_review::BackgroundReviewConfig {
                enabled: true,
                turn_threshold: 1,
            },
        )
        .unwrap()
        .unwrap();
        let service = SkillProposalService::trusted_local(&cwd);

        assert!(service
            .list(ProposalListFilter::default())
            .unwrap()
            .proposals
            .is_empty());
        assert!(matches!(
            service.detail(&background_id(&warning.id)),
            Err(SkillProposalServiceError::NotFound)
        ));
        assert!(background_review::load_background_review_proposal(&warning.id).is_ok());
    }

    #[test]
    #[serial]
    fn background_skill_requires_matching_workspace_and_never_stages_native_copy() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("project");
        let other = temp.path().join("other");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let proposal = BackgroundReviewProposal {
            id: "review-skill-create".to_string(),
            source_session_id: "session-1".to_string(),
            kind: BackgroundReviewProposalKind::SkillCreate,
            summary: "Create reviewed skill".to_string(),
            payload: serde_json::json!({
                "skill_name": "background-skill",
                "scope": "project",
                "cwd": other,
                "markdown": "---\ndescription: Background skill.\n---\nRun review."
            }),
            created_at: chrono::Utc::now(),
        };
        std::fs::create_dir_all(background_review::review_proposals_dir()).unwrap();
        std::fs::write(
            background_review::proposal_path(&proposal.id),
            serde_json::to_vec_pretty(&proposal).unwrap(),
        )
        .unwrap();
        let service = SkillProposalService::trusted_local(&cwd);
        let public_id = background_id(&proposal.id);
        assert!(!service.detail(&public_id).unwrap().summary.actionable);

        let mut trusted = proposal.clone();
        trusted.payload["cwd"] = serde_json::Value::String(cwd.display().to_string());
        std::fs::write(
            background_review::proposal_path(&proposal.id),
            serde_json::to_vec_pretty(&trusted).unwrap(),
        )
        .unwrap();
        let detail = service.detail(&public_id).unwrap();
        let outcome = service
            .approve(
                &public_id,
                ProposalMutationRequest {
                    request_id: "background-request".to_string(),
                    expected_proposal_digest: detail.summary.proposal_digest,
                    expected_target: ExpectedTarget::Absent,
                },
            )
            .unwrap();
        assert!(outcome.resulting_target_digest.is_some());
        assert!(cwd
            .join(".allthecodes/skills/background-skill/SKILL.md")
            .is_file());
        let native_owner = cwd.join(".allthecodes/skill_proposals");
        assert!(!native_owner.join(format!("{}.json", proposal.id)).exists());
        assert!(std::fs::read_dir(native_owner).unwrap().all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            != Some("json")));
    }

    #[test]
    #[serial]
    fn namespace_is_strict_and_scope_authorization_is_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let proposal = stage(&cwd, "scoped-skill", SkillProposalAction::Create);
        let service = SkillProposalService::new(
            &cwd,
            ProposalAccess {
                project: false,
                user: true,
            },
        );
        assert!(matches!(
            service.detail(&proposal.id),
            Err(SkillProposalServiceError::Invalid { .. })
        ));
        let public_id = native_id(proposal.scope, &proposal.id);
        assert!(matches!(
            service.detail(&public_id),
            Err(SkillProposalServiceError::Forbidden)
        ));
        assert!(service
            .list(ProposalListFilter::default())
            .unwrap()
            .proposals
            .is_empty());
        let detail = SkillProposalService::trusted_local(&cwd)
            .detail(&public_id)
            .unwrap();
        assert!(matches!(
            service.approve(
                &public_id,
                ProposalMutationRequest {
                    request_id: "forbidden-request".to_string(),
                    expected_proposal_digest: detail.summary.proposal_digest,
                    expected_target: ExpectedTarget::Absent,
                }
            ),
            Err(SkillProposalServiceError::Forbidden)
        ));
    }

    #[test]
    #[serial]
    fn background_scan_isolates_corrupt_records_and_accepts_dot_ids() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let proposal = BackgroundReviewProposal {
            id: "review.session-skill".to_string(),
            source_session_id: "session.with-dot".to_string(),
            kind: BackgroundReviewProposalKind::SkillCreate,
            summary: "Create a background skill".to_string(),
            payload: serde_json::json!({
                "skill_name": "dot-background-skill",
                "scope": "project",
                "cwd": cwd,
                "markdown": "---\ndescription: Dot background skill.\n---\nRun it."
            }),
            created_at: chrono::Utc::now(),
        };
        std::fs::create_dir_all(background_review::review_proposals_dir()).unwrap();
        std::fs::write(
            background_review::proposal_path(&proposal.id),
            serde_json::to_vec(&proposal).unwrap(),
        )
        .unwrap();
        std::fs::write(
            background_review::review_proposals_dir().join("corrupt.json"),
            b"{not-json",
        )
        .unwrap();

        let service = SkillProposalService::trusted_local(&cwd);
        let listed = service.list(ProposalListFilter::default()).unwrap();

        assert_eq!(listed.proposals.len(), 1);
        assert_eq!(
            listed.proposals[0].proposal_id,
            "background_review:review.session-skill"
        );
        assert_eq!(listed.diagnostic_count, 1);
        assert!(
            service
                .detail("background_review:review.session-skill")
                .unwrap()
                .summary
                .actionable
        );
        assert!(matches!(
            service.detail("review.session-skill"),
            Err(SkillProposalServiceError::Invalid { .. })
        ));
    }
}
