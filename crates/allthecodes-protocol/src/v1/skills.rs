use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Skill Summary
// ---------------------------------------------------------------------------

/// Summary of a skill for list and detail views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillsListResponse {
    pub skills: Vec<SkillSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Value>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// Query parameters for `GET /api/skills`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillPatchRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
}
