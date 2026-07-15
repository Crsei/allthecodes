#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Skill Summary
// ---------------------------------------------------------------------------

/// Summary of a skill for list and detail views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    pub source: String,
    pub user_invocable: bool,
    pub model_invocable: bool,
    pub context: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SkillFileSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub enabled: bool,
    pub pinned: bool,
}

// ---------------------------------------------------------------------------
// Skill File
// ---------------------------------------------------------------------------

/// Metadata for a file associated with a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillFileSummary {
    pub path: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// Response body for `GET /api/skills`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillsListResponse {
    pub skills: Vec<SkillSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Value>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// Query parameters for `GET /api/skills`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillsListQuery {
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub refresh: Option<bool>,
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

/// Response body for `GET /api/skills/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillDetailResponse {
    pub skill: SkillSummary,
    pub prompt_body: String,
    pub files: Vec<SkillFileSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

/// Request body for `PATCH /api/skills/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillPatchRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
}

// ---------------------------------------------------------------------------
// Skill proposal review
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalSource {
    Native,
    BackgroundReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalAction {
    Create,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalScope {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalValidationState {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillProposalDisposition {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalSummary {
    /// Opaque owner-qualified identifier. Clients must not parse this value.
    pub proposal_id: String,
    pub source: SkillProposalSource,
    pub action: SkillProposalAction,
    pub scope: SkillProposalScope,
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    pub created_at: String,
    /// Display-only target relative to the server-selected skill root.
    pub relative_target: String,
    pub proposal_digest: String,
    pub validation_state: SkillProposalValidationState,
    pub actionable: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SkillProposalSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SkillProposalScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<SkillProposalAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalListResponse {
    pub proposals: Vec<SkillProposalSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub diagnostic_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalParams {
    pub proposal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalDetailResponse {
    pub proposal: SkillProposalSummary,
    pub markdown: String,
    pub markdown_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_target_digest: Option<String>,
    pub target_exists: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SkillProposalDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalDiffResponse {
    pub proposal_id: String,
    pub proposal_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_digest: Option<String>,
    pub diff: String,
    pub truncated: bool,
    pub untruncated_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SkillProposalExpectedTarget {
    Absent,
    Digest { digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalMutationRequest {
    pub request_id: String,
    pub expected_proposal_digest: String,
    pub expected_target_digest: SkillProposalExpectedTarget,
}

/// Transport-neutral mutation params used by dispatchers after combining the
/// REST path parameter with the request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalMutationParams {
    pub proposal_id: String,
    pub request_id: String,
    pub expected_proposal_digest: String,
    pub expected_target_digest: SkillProposalExpectedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkillProposalMutationResponse {
    pub proposal_id: String,
    pub request_id: String,
    pub disposition: SkillProposalDisposition,
    pub skill_name: String,
    pub scope: SkillProposalScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resulting_target_digest: Option<String>,
    pub idempotent_replay: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn proposal_contract_uses_closed_enums_and_tagged_target_state() {
        let request: SkillProposalMutationRequest = serde_json::from_value(json!({
            "request_id": "request-1",
            "expected_proposal_digest": format!("sha256:{}", "a".repeat(64)),
            "expected_target_digest": {
                "state": "digest",
                "digest": format!("sha256:{}", "b".repeat(64))
            }
        }))
        .unwrap();
        assert!(matches!(
            request.expected_target_digest,
            SkillProposalExpectedTarget::Digest { .. }
        ));
        assert!(serde_json::from_value::<SkillProposalSource>(json!("unknown_owner")).is_err());
        assert_eq!(
            serde_json::to_value(SkillProposalSource::BackgroundReview).unwrap(),
            json!("background_review")
        );
    }

    #[test]
    fn proposal_list_query_rejects_unknown_filter_values() {
        assert!(serde_json::from_value::<SkillProposalListQuery>(json!({
            "scope": "host_path"
        }))
        .is_err());
    }
}
