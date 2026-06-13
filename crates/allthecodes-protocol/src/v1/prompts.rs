#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// QuickPrompt
// ---------------------------------------------------------------------------

/// Quick prompt entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct QuickPrompt {
    pub id: String,
    pub name: String,
    pub content: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Prompts List Response
// ---------------------------------------------------------------------------

/// Response for listing all prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PromptsListResponse {
    pub prompts: Vec<QuickPrompt>,
}

// ---------------------------------------------------------------------------
// Prompt Create Request
// ---------------------------------------------------------------------------

/// Request to create a new prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PromptCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub description: String,
}

// ---------------------------------------------------------------------------
// Prompt Update Request
// ---------------------------------------------------------------------------

/// Request to update an existing prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PromptUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Prompt Mutation Response
// ---------------------------------------------------------------------------

/// Response for prompt create / update / delete mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PromptMutationResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<QuickPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
