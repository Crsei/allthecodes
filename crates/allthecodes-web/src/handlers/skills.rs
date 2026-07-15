//! Skills API handlers.
//!
//! Provides REST endpoints for listing, inspecting, and managing skills
//! discovered from bundled, user, project, and plugin sources.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use allthecodes_protocol::v1::skills::SkillsListResponse as ProtocolSkillsListResponse;
use allthecodes_protocol::v1::skills::{
    SkillProposalAction as ApiProposalAction, SkillProposalDetailResponse,
    SkillProposalDiagnostic as ApiProposalDiagnostic, SkillProposalDiffResponse,
    SkillProposalDisposition as ApiProposalDisposition, SkillProposalExpectedTarget,
    SkillProposalListQuery, SkillProposalListResponse, SkillProposalMutationParams,
    SkillProposalMutationRequest, SkillProposalMutationResponse, SkillProposalParams,
    SkillProposalScope as ApiProposalScope, SkillProposalSource as ApiProposalSource,
    SkillProposalSummary as ApiProposalSummary,
    SkillProposalValidationState as ApiProposalValidationState,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::SerializationScope;
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_config::paths;
use allthecodes_engine::services::skill_proposals::{
    ExpectedTarget, ProposalAccess, ProposalAction, ProposalDetail, ProposalDiff,
    ProposalDisposition, ProposalList, ProposalListFilter, ProposalMutationOutcome, ProposalScope,
    ProposalSource, ProposalSummary, ProposalValidationState, SkillProposalService,
    SkillProposalServiceError,
};
use allthecodes_skills::{
    find_skill, get_all_skills, get_skill_diagnostics, registry_revision, SkillContext,
    SkillDefinition, SkillDiagnostic, SkillSource,
};

type BoxResponse = Box<Response>;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SkillsListProcessor {
    state: WebState,
}

impl From<WebState> for SkillsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillsListProcessor {
    type Request = allthecodes_protocol::v1::skills::SkillsListQuery;
    type Response = ProtocolSkillsListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let skills = get_all_skills();
        let diagnostics: Vec<Value> = get_skill_diagnostics()
            .into_iter()
            .map(|d| serde_json::to_value(&d).unwrap_or(Value::String(format!("{d:?}"))))
            .collect();
        let revision = registry_revision();
        let metadata = load_metadata();

        let summaries: Vec<allthecodes_protocol::v1::skills::SkillSummary> = skills
            .into_iter()
            .map(|skill| {
                let meta = metadata.get(&skill.name).cloned().unwrap_or_default();
                allthecodes_protocol::v1::skills::SkillSummary {
                    id: skill.name.clone(),
                    name: skill.name.clone(),
                    display_name: skill.display_name().to_string(),
                    description: skill.frontmatter.description.clone(),
                    when_to_use: skill.frontmatter.when_to_use.clone(),
                    source: format!("{:?}", skill.source),
                    user_invocable: skill.is_user_invocable(),
                    model_invocable: skill.is_model_invocable(),
                    context: format!("{:?}", skill.frontmatter.context),
                    allowed_tools: skill.frontmatter.allowed_tools.clone(),
                    files: skill_file_summaries(skill.base_dir.as_deref())
                        .into_iter()
                        .map(|f| allthecodes_protocol::v1::skills::SkillFileSummary {
                            path: f.path,
                            kind: f.kind,
                            size_bytes: f.size_bytes,
                        })
                        .collect(),
                    version: skill.frontmatter.version.clone(),
                    enabled: meta.enabled,
                    pinned: meta.pinned,
                }
            })
            .collect();

        Ok(ProtocolSkillsListResponse {
            skills: summaries,
            diagnostics,
            revision,
            profile_id: None,
        })
    }
}

#[derive(Clone)]
pub struct SkillProposalsListProcessor {
    state: WebState,
}

impl From<WebState> for SkillProposalsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillProposalsListProcessor {
    type Request = SkillProposalListQuery;
    type Response = SkillProposalListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.proposals.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let filter = ProposalListFilter {
            source: params.source.map(from_api_proposal_source),
            scope: params.scope.map(from_api_proposal_scope),
            action: params.action.map(from_api_proposal_action),
            cursor: params.cursor,
            limit: params.limit,
        };
        proposal_service(&self.state)
            .list(filter)
            .map(project_proposal_list)
            .map_err(|error| proposal_api_error(error, None))
    }
}

#[derive(Clone)]
pub struct SkillProposalDetailProcessor {
    state: WebState,
}

impl From<WebState> for SkillProposalDetailProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillProposalDetailProcessor {
    type Request = SkillProposalParams;
    type Response = SkillProposalDetailResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.proposals.detail"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let proposal_id = params.proposal_id;
        proposal_service(&self.state)
            .detail(&proposal_id)
            .map(project_proposal_detail)
            .map_err(|error| proposal_api_error(error, Some(proposal_id)))
    }
}

#[derive(Clone)]
pub struct SkillProposalDiffProcessor {
    state: WebState,
}

impl From<WebState> for SkillProposalDiffProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillProposalDiffProcessor {
    type Request = SkillProposalParams;
    type Response = SkillProposalDiffResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.proposals.diff"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let proposal_id = params.proposal_id;
        proposal_service(&self.state)
            .diff(&proposal_id)
            .map(project_proposal_diff)
            .map_err(|error| proposal_api_error(error, Some(proposal_id)))
    }
}

#[derive(Clone)]
pub struct SkillProposalApproveProcessor {
    state: WebState,
}

impl From<WebState> for SkillProposalApproveProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillProposalApproveProcessor {
    type Request = SkillProposalMutationParams;
    type Response = SkillProposalMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.proposals.approve"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "proposal_id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        decide_proposal(&self.state, params, ProposalDisposition::Approved)
    }
}

#[derive(Clone)]
pub struct SkillProposalRejectProcessor {
    state: WebState,
}

impl From<WebState> for SkillProposalRejectProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for SkillProposalRejectProcessor {
    type Request = SkillProposalMutationParams;
    type Response = SkillProposalMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "skills.proposals.reject"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "proposal_id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        decide_proposal(&self.state, params, ProposalDisposition::Rejected)
    }
}

fn decide_proposal(
    state: &WebState,
    params: SkillProposalMutationParams,
    disposition: ProposalDisposition,
) -> Result<SkillProposalMutationResponse, ProtocolApiError> {
    let proposal_id = params.proposal_id;
    let request = allthecodes_engine::services::skill_proposals::ProposalMutationRequest {
        request_id: params.request_id,
        expected_proposal_digest: params.expected_proposal_digest,
        expected_target: from_api_expected_target(params.expected_target_digest),
    };
    let result = match disposition {
        ProposalDisposition::Approved => proposal_service(state).approve(&proposal_id, request),
        ProposalDisposition::Rejected => proposal_service(state).reject(&proposal_id, request),
    };
    result
        .map(project_proposal_mutation)
        .map_err(|error| proposal_api_error(error, Some(proposal_id)))
}

fn proposal_service(state: &WebState) -> SkillProposalService {
    SkillProposalService::new(
        PathBuf::from(state.engine().cwd()),
        ProposalAccess {
            project: true,
            user: true,
        },
    )
}

fn proposal_api_error(
    error: SkillProposalServiceError,
    proposal_id: Option<String>,
) -> ProtocolApiError {
    match error {
        SkillProposalServiceError::Invalid { code, message } => {
            ProtocolApiError::BadRequest { code, message }
        }
        SkillProposalServiceError::Forbidden => ProtocolApiError::Forbidden {
            code: "proposal_scope_forbidden",
            message: "skill proposal scope is forbidden".to_string(),
        },
        SkillProposalServiceError::NotFound => ProtocolApiError::NotFound {
            entity: "skill_proposal",
            id: proposal_id.unwrap_or_default(),
        },
        SkillProposalServiceError::ProposalChanged
        | SkillProposalServiceError::TargetChanged
        | SkillProposalServiceError::ProposalConsumed => ProtocolApiError::Conflict {
            reason: format!("{}: {}", error.code(), error),
        },
        SkillProposalServiceError::NotActionable => ProtocolApiError::Validation {
            field: "proposal_id".to_string(),
            message: "proposal_not_actionable: skill proposal is not actionable".to_string(),
        },
        SkillProposalServiceError::ProposalTooLarge => ProtocolApiError::PayloadTooLarge {
            code: "proposal_too_large",
            message: "skill proposal content exceeds the supported limit".to_string(),
        },
        SkillProposalServiceError::StoreUnavailable => ProtocolApiError::ServiceUnavailable {
            code: "proposal_store_unavailable",
            message: "skill proposal store is unavailable".to_string(),
        },
    }
}

fn project_proposal_list(list: ProposalList) -> SkillProposalListResponse {
    SkillProposalListResponse {
        proposals: list
            .proposals
            .into_iter()
            .map(project_proposal_summary)
            .collect(),
        next_cursor: list.next_cursor,
        diagnostic_count: list.diagnostic_count,
        truncated: list.truncated,
    }
}

fn project_proposal_detail(detail: ProposalDetail) -> SkillProposalDetailResponse {
    SkillProposalDetailResponse {
        proposal: project_proposal_summary(detail.summary),
        markdown: detail.markdown,
        markdown_bytes: detail.markdown_bytes,
        current_target_digest: detail.current_target_digest,
        target_exists: detail.target_exists,
        diagnostics: detail
            .diagnostics
            .into_iter()
            .map(|diagnostic| ApiProposalDiagnostic {
                code: diagnostic.code,
                message: diagnostic.message,
            })
            .collect(),
    }
}

fn project_proposal_diff(diff: ProposalDiff) -> SkillProposalDiffResponse {
    SkillProposalDiffResponse {
        proposal_id: diff.proposal_id,
        proposal_digest: diff.proposal_digest,
        baseline_digest: diff.baseline_digest,
        diff: diff.diff,
        truncated: diff.truncated,
        untruncated_bytes: diff.untruncated_bytes,
    }
}

fn project_proposal_mutation(outcome: ProposalMutationOutcome) -> SkillProposalMutationResponse {
    SkillProposalMutationResponse {
        proposal_id: outcome.proposal_id,
        request_id: outcome.request_id,
        disposition: match outcome.disposition {
            ProposalDisposition::Approved => ApiProposalDisposition::Approved,
            ProposalDisposition::Rejected => ApiProposalDisposition::Rejected,
        },
        skill_name: outcome.skill_name,
        scope: project_proposal_scope(outcome.scope),
        resulting_target_digest: outcome.resulting_target_digest,
        idempotent_replay: outcome.idempotent_replay,
    }
}

fn project_proposal_summary(summary: ProposalSummary) -> ApiProposalSummary {
    ApiProposalSummary {
        proposal_id: summary.proposal_id,
        source: match summary.source {
            ProposalSource::Native => ApiProposalSource::Native,
            ProposalSource::BackgroundReview => ApiProposalSource::BackgroundReview,
        },
        action: match summary.action {
            ProposalAction::Create => ApiProposalAction::Create,
            ProposalAction::Patch => ApiProposalAction::Patch,
        },
        scope: project_proposal_scope(summary.scope),
        skill_name: summary.skill_name,
        source_session_id: summary.source_session_id,
        created_at: summary.created_at,
        relative_target: summary.relative_target,
        proposal_digest: summary.proposal_digest,
        validation_state: match summary.validation_state {
            ProposalValidationState::Valid => ApiProposalValidationState::Valid,
            ProposalValidationState::Invalid => ApiProposalValidationState::Invalid,
        },
        actionable: summary.actionable,
        summary: summary.summary,
    }
}

fn project_proposal_scope(scope: ProposalScope) -> ApiProposalScope {
    match scope {
        ProposalScope::User => ApiProposalScope::User,
        ProposalScope::Project => ApiProposalScope::Project,
    }
}

fn from_api_proposal_source(source: ApiProposalSource) -> ProposalSource {
    match source {
        ApiProposalSource::Native => ProposalSource::Native,
        ApiProposalSource::BackgroundReview => ProposalSource::BackgroundReview,
    }
}

fn from_api_proposal_action(action: ApiProposalAction) -> ProposalAction {
    match action {
        ApiProposalAction::Create => ProposalAction::Create,
        ApiProposalAction::Patch => ProposalAction::Patch,
    }
}

fn from_api_proposal_scope(scope: ApiProposalScope) -> ProposalScope {
    match scope {
        ApiProposalScope::User => ProposalScope::User,
        ApiProposalScope::Project => ProposalScope::Project,
    }
}

fn from_api_expected_target(target: SkillProposalExpectedTarget) -> ExpectedTarget {
    match target {
        SkillProposalExpectedTarget::Absent => ExpectedTarget::Absent,
        SkillProposalExpectedTarget::Digest { digest } => ExpectedTarget::Digest { digest },
    }
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Summary of a skill for the list view.
#[derive(Serialize)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    pub source: SkillSource,
    pub user_invocable: bool,
    pub model_invocable: bool,
    pub context: SkillContext,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SkillFileSummary>,
    pub version: Option<String>,
    pub enabled: bool,
    pub pinned: bool,
}

/// Response body for `GET /api/skills`.
#[derive(Serialize)]
pub struct SkillsListResponse {
    pub skills: Vec<SkillSummary>,
    pub diagnostics: Vec<SkillDiagnostic>,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// File metadata used by list/detail views.
#[derive(Clone, Serialize)]
pub struct SkillFileSummary {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Full skill detail returned by `GET /api/skills/{id}`.
#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub skill: SkillSummary,
    pub prompt_body: String,
    pub files: Vec<SkillFileSummary>,
    pub diagnostics: Vec<String>,
}

/// Query parameters for `GET /api/skills`.
#[derive(Deserialize)]
pub struct SkillsListQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

/// Query parameters for `GET /api/skills/{id}/files`.
#[derive(Deserialize)]
pub struct SkillFileQuery {
    pub path: String,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

/// Response body for `GET /api/skills/{id}/files`.
#[derive(Serialize)]
pub struct SkillFileResponse {
    pub skill_id: String,
    pub path: String,
    pub content: String,
    pub truncated: bool,
    pub is_binary: bool,
    pub media_type: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// Request body for `PATCH /api/skills/{id}`.
#[derive(Deserialize)]
pub struct SkillPatchRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
}

/// Response body for `PATCH /api/skills/{id}`.
#[derive(Serialize)]
pub struct SkillPatchResponse {
    pub ok: bool,
    pub skill: SkillSummary,
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::SkillsList, get(skills_list_handler))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/skills` -- list summaries for all discovered skills.
pub async fn skills_list_handler(
    State(state): State<WebState>,
    Query(_query): Query<SkillsListQuery>,
) -> Response {
    rest_processor_response::<SkillsListProcessor>(
        state,
        ApiMethod::SkillsList,
        allthecodes_protocol::v1::skills::SkillsListQuery {
            filter: None,
            refresh: None,
        },
    )
    .await
}

/// `GET /api/skills/proposals` -- list bounded, owner-qualified proposals.
pub async fn skill_proposals_list_handler(
    State(state): State<WebState>,
    Query(query): Query<SkillProposalListQuery>,
) -> Response {
    rest_processor_response::<SkillProposalsListProcessor>(
        state,
        ApiMethod::SkillProposalsList,
        query,
    )
    .await
}

/// `GET /api/skills/proposals/{proposal_id}` -- validated proposal detail.
pub async fn skill_proposal_detail_handler(
    AxumPath(proposal_id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    rest_processor_response::<SkillProposalDetailProcessor>(
        state,
        ApiMethod::SkillProposalDetail,
        SkillProposalParams { proposal_id },
    )
    .await
}

/// `GET /api/skills/proposals/{proposal_id}/diff` -- server-generated diff.
pub async fn skill_proposal_diff_handler(
    AxumPath(proposal_id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    rest_processor_response::<SkillProposalDiffProcessor>(
        state,
        ApiMethod::SkillProposalDiff,
        SkillProposalParams { proposal_id },
    )
    .await
}

/// `POST /api/skills/proposals/{proposal_id}/approve` -- consume and install.
///
/// The route must be registered behind the privileged-write middleware.
pub async fn skill_proposal_approve_handler(
    AxumPath(proposal_id): AxumPath<String>,
    State(state): State<WebState>,
    Json(request): Json<SkillProposalMutationRequest>,
) -> Response {
    rest_processor_response::<SkillProposalApproveProcessor>(
        state,
        ApiMethod::SkillProposalApprove,
        mutation_params(proposal_id, request),
    )
    .await
}

/// `POST /api/skills/proposals/{proposal_id}/reject` -- consume without install.
///
/// The route must be registered behind the privileged-write middleware.
pub async fn skill_proposal_reject_handler(
    AxumPath(proposal_id): AxumPath<String>,
    State(state): State<WebState>,
    Json(request): Json<SkillProposalMutationRequest>,
) -> Response {
    rest_processor_response::<SkillProposalRejectProcessor>(
        state,
        ApiMethod::SkillProposalReject,
        mutation_params(proposal_id, request),
    )
    .await
}

fn mutation_params(
    proposal_id: String,
    request: SkillProposalMutationRequest,
) -> SkillProposalMutationParams {
    SkillProposalMutationParams {
        proposal_id,
        request_id: request.request_id,
        expected_proposal_digest: request.expected_proposal_digest,
        expected_target_digest: request.expected_target_digest,
    }
}

/// `GET /api/skills/{id}` -- detail with prompt body, frontmatter, metadata.
pub async fn skills_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    let Some(skill) = find_skill(&id) else {
        return not_found(format!("Skill '{}' not found", id));
    };
    let metadata = load_metadata();
    let meta = metadata.get(&id).cloned().unwrap_or_default();
    let summary = skill_summary(&skill, meta);

    Json(SkillDetailResponse {
        files: summary.files.clone(),
        skill: summary,
        prompt_body: skill.prompt_body,
        diagnostics: Vec::new(),
    })
    .into_response()
}

/// `GET /api/skills/{id}/files?path=` -- resolve a path in the skill root.
///
/// Returns the file content with media type detection. Rejects path traversal,
/// hidden files, and directories. Caps read size and sets `truncated` when
/// the file exceeds the limit.
pub async fn skills_files_handler(
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SkillFileQuery>,
) -> Response {
    let Some(skill) = find_skill(&id) else {
        return not_found(format!("Skill '{}' not found", id));
    };

    let Some(base_dir) = skill.base_dir else {
        return bad_request(
            "Skill has no base directory; bundled skills do not have files on disk".to_string(),
        )
        .into_response();
    };

    // Path traversal prevention
    let resolved = match resolve_skill_path(&base_dir, &query.path) {
        Ok(path) => path,
        Err(error) => return (*error).into_response(),
    };

    // Reject directories
    if resolved.is_dir() {
        return bad_request(format!("'{}' is a directory, not a file", query.path));
    }

    // Do not expose hidden / credential files
    if is_hidden_file(&resolved) {
        return forbidden("Access to hidden files is not allowed".to_string());
    }

    let max_bytes = query.max_bytes.unwrap_or(10 * 1024 * 1024);
    let mut response = match read_skill_file(&id, &query.path, &resolved, max_bytes) {
        Ok(r) => r,
        Err(error) => return (*error).into_response(),
    };
    response.profile_id = query.profile_id;
    Json(response).into_response()
}

/// `PATCH /api/skills/{id}` -- update enabled/pinned flags.
pub async fn skills_patch_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<SkillPatchRequest>,
) -> Response {
    // Verify the skill exists
    let Some(skill) = find_skill(&id) else {
        return not_found(format!("Skill '{}' not found", id));
    };

    let mut metadata = load_metadata();
    let entry = metadata.entry(id.clone()).or_default();

    if let Some(enabled) = req.enabled {
        entry.enabled = enabled;
    }
    if let Some(pinned) = req.pinned {
        entry.pinned = pinned;
    }
    entry.updated_at = Utc::now().timestamp();
    let updated_meta = entry.clone();

    match write_metadata(&metadata) {
        Ok(()) => Json(SkillPatchResponse {
            ok: true,
            skill: skill_summary(&skill, updated_meta),
        })
        .into_response(),
        Err(error) => internal_error(error),
    }
}

// ---------------------------------------------------------------------------
// Metadata persistence for enabled / pinned flags
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillUiMetadata {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillMetadataFile {
    #[serde(default)]
    skills: HashMap<String, SkillUiMetadata>,
}

fn metadata_path() -> PathBuf {
    paths::data_root().join("web").join("skills.json")
}

fn load_metadata() -> HashMap<String, SkillUiMetadata> {
    let path = metadata_path();
    if !path.exists() {
        return HashMap::new();
    }
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    if contents.trim().is_empty() {
        return HashMap::new();
    }
    serde_json::from_str::<SkillMetadataFile>(&contents)
        .map(|file| file.skills)
        .unwrap_or_default()
}

fn write_metadata(skills: &HashMap<String, SkillUiMetadata>) -> Result<(), String> {
    let path = metadata_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = SkillMetadataFile {
        skills: skills.clone(),
    };
    let data =
        serde_json::to_string_pretty(&file).map_err(|e| format!("Serialization error: {}", e))?;
    // Atomic write via temp file
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn skill_summary(skill: &SkillDefinition, meta: SkillUiMetadata) -> SkillSummary {
    SkillSummary {
        id: skill.name.clone(),
        name: skill.name.clone(),
        display_name: skill.display_name().to_string(),
        description: skill.frontmatter.description.clone(),
        when_to_use: skill.frontmatter.when_to_use.clone(),
        source: skill.source.clone(),
        user_invocable: skill.is_user_invocable(),
        model_invocable: skill.is_model_invocable(),
        context: skill.frontmatter.context.clone(),
        allowed_tools: skill.frontmatter.allowed_tools.clone(),
        files: skill_file_summaries(skill.base_dir.as_deref()),
        version: skill.frontmatter.version.clone(),
        enabled: meta.enabled,
        pinned: meta.pinned,
    }
}

fn skill_file_summaries(base_dir: Option<&Path>) -> Vec<SkillFileSummary> {
    let Some(base_dir) = base_dir else {
        return Vec::new();
    };
    let mut files = Vec::new();
    if base_dir.join("SKILL.md").is_file() {
        files.push(file_summary(base_dir, "SKILL.md"));
    }
    if let Ok(entries) = std::fs::read_dir(base_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            let Ok(relative) = path.strip_prefix(base_dir) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            files.push(file_summary(base_dir, relative));
            if files.len() >= 24 {
                break;
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn file_summary(base_dir: &Path, relative_path: &str) -> SkillFileSummary {
    let path = base_dir.join(relative_path);
    let metadata = std::fs::metadata(&path).ok();
    SkillFileSummary {
        path: relative_path.replace('\\', "/"),
        kind: if metadata.as_ref().is_some_and(|meta| meta.is_dir()) {
            "directory".to_string()
        } else if relative_path.ends_with(".md") || relative_path.ends_with(".markdown") {
            "markdown".to_string()
        } else {
            "asset".to_string()
        },
        size_bytes: metadata
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len()),
    }
}

// ---------------------------------------------------------------------------
// Path resolution and file reading helpers
// ---------------------------------------------------------------------------

/// Resolve `request_path` relative to `base_dir`, rejecting traversal.
fn resolve_skill_path(base_dir: &Path, request_path: &str) -> Result<PathBuf, BoxResponse> {
    let base = base_dir.canonicalize().map_err(|_| {
        Box::new(internal_error(
            "Failed to resolve skill base directory".to_string(),
        ))
    })?;

    let cleaned = request_path.trim_start_matches('/');
    let joined = base.join(cleaned);

    // Check existence first to give a precise error.
    if !joined.exists() {
        return Err(Box::new(not_found(format!(
            "File '{}' not found in skill",
            request_path
        ))));
    }

    let resolved = joined.canonicalize().map_err(|_| {
        Box::new(bad_request(format!(
            "Cannot resolve path '{}'",
            request_path
        )))
    })?;

    if !resolved.starts_with(&base) {
        return Err(Box::new(forbidden("Path traversal detected".to_string())));
    }

    Ok(resolved)
}

/// Returns true if the file name starts with a dot (hidden file).
fn is_hidden_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.') && name != "." && name != "..")
        .unwrap_or(false)
}

/// Read a file up to `max_bytes`, detect binary content, and return the
/// response payload.
fn read_skill_file(
    skill_id: &str,
    request_path: &str,
    path: &Path,
    max_bytes: u64,
) -> Result<SkillFileResponse, BoxResponse> {
    let meta = std::fs::metadata(path).map_err(|e| {
        Box::new(internal_error(format!(
            "Failed to read file metadata: {}",
            e
        )))
    })?;

    let size = meta.len();
    let file = std::fs::File::open(path)
        .map_err(|e| Box::new(internal_error(format!("Failed to open file: {}", e))))?;

    let mut buf = Vec::new();
    file.take(max_bytes)
        .read_to_end(&mut buf)
        .map_err(|e| Box::new(internal_error(format!("Failed to read file: {}", e))))?;

    // Probe first 8 KB for null bytes to detect binary content
    let is_binary = !buf.is_empty() && buf[..buf.len().min(8192)].contains(&0x00);

    let truncated = size > max_bytes;
    let content = if is_binary {
        String::new()
    } else {
        String::from_utf8_lossy(&buf).to_string()
    };

    let media_type = detect_media_type(path);

    Ok(SkillFileResponse {
        skill_id: skill_id.to_string(),
        path: request_path.to_string(),
        content,
        truncated,
        is_binary,
        media_type: media_type.to_string(),
        size,
        profile_id: None,
    })
}

/// Infer a media type from the file extension.
fn detect_media_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/x-yaml",
        "toml" => "application/toml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "ts" => "application/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "sh" | "bash" => "application/x-sh",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn bad_request(error: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "bad_request",
        message: error,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn forbidden(error: String) -> Response {
    let body = ProtocolApiError::Forbidden {
        code: "path_traversal",
        message: error,
    }
    .into_body();
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "skill",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
