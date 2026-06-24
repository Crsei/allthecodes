#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MentionSuggestion {
    /// One of: session, file, skill
    pub kind: String,
    /// Display name shown in the chip
    pub name: String,
    /// Resolved target (session id, file path, skill id)
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MentionAutocompleteResponse {
    pub suggestions: Vec<MentionSuggestion>,
}

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct MentionAutocompleteQuery {
    /// Search prefix to match against names / ids / paths
    pub q: String,
    /// Optional kind filter: "session", "file", "skill"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Max suggestions to return (default 10)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
