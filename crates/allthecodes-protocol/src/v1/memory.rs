#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Global,
    Project,
    Team,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryEntry {
    /// Opaque route identifier encoding the server-resolved scope and key.
    pub id: String,
    pub scope: MemoryScope,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub value_truncated: bool,
    pub category: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,

    // Compatibility aliases retained in responses during the existing Web UI
    // migration window. They project canonical fields and are never separate
    // storage owners.
    pub content: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub workspace: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScope>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryListResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub entries: Vec<MemoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<MemoryMigrationReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryMigrationReport {
    pub imported: usize,
    pub already_present: usize,
    pub conflicts: Vec<String>,
    pub completed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryUpdateRequest {
    #[serde(default, alias = "content", skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub memory_type: Option<MemoryType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_terms: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryUpdateResponse {
    pub entry: MemoryEntry,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryDreamListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryDreamSummary {
    pub date: String,
    pub byte_size: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryDreamListResponse {
    pub items: Vec<MemoryDreamSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryDreamDetailResponse {
    pub date: String,
    pub byte_size: usize,
    pub markdown: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalKind {
    MemoryAdd,
    MemoryReplace,
    WorkflowWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryProposal {
    pub id: String,
    pub kind: MemoryProposalKind,
    pub summary: String,
    pub source_session_id: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub writes_memory_on_approval: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryProposalListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryProposalListResponse {
    pub proposals: Vec<MemoryProposal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub automatic_producer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryProposalDetailResponse {
    pub proposal: MemoryProposal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalDisposition {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MemoryProposalDecisionResponse {
    pub proposal_id: String,
    pub disposition: MemoryProposalDisposition,
    pub decided_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<MemoryEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn update_accepts_legacy_content_alias() {
        let request: MemoryUpdateRequest =
            serde_json::from_value(json!({"content": "legacy", "pinned": true})).unwrap();
        assert_eq!(request.value.as_deref(), Some("legacy"));
        assert_eq!(request.pinned, Some(true));
    }

    #[test]
    fn memory_scope_and_type_are_closed_lowercase_enums() {
        assert_eq!(
            serde_json::to_value(MemoryScope::Project).unwrap(),
            "project"
        );
        assert_eq!(
            serde_json::to_value(MemoryType::Reference).unwrap(),
            "reference"
        );
        assert!(serde_json::from_value::<MemoryScope>(json!("host-path")).is_err());
    }
}
