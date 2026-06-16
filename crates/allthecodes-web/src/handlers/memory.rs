//! Memory API handler.
//!
//! GET    /api/memory?profile_id= -- list entries (newest first)
//! PATCH  /api/memory/:id         -- update content, tags, pinned
//!
//! Persistence: ALLTHECODES_HOME/memory/entries.json (Serde JSON array).

use std::path::PathBuf;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::paths;

use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length of `content` in a memory entry (plain text).
const MAX_CONTENT_LENGTH: usize = 100_000;

/// Maximum number of tags per entry.
const MAX_TAG_COUNT: usize = 50;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single persisted memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    /// Timestamp of the original observation (ISO 8601).
    pub timestamp: String,
    pub session_id: String,
    pub workspace: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    /// Timestamp of the most recent update (ISO 8601).
    pub updated_at: String,
}

/// The in-file wrapper.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct MemoryStore {
    #[serde(default)]
    pub(crate) entries: Vec<MemoryEntry>,
}

/// GET /api/memory response.
#[derive(Debug, Serialize)]
pub struct MemoryListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub entries: Vec<MemoryEntry>,
}

/// PATCH /api/memory/:id request body.
#[derive(Debug, Deserialize)]
pub struct MemoryUpdateRequest {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub pinned: Option<bool>,
}

/// PATCH /api/memory/:id response.
#[derive(Debug, Serialize)]
pub struct MemoryUpdateResponse {
    pub entry: MemoryEntry,
}

/// Query parameters for GET /api/memory.
#[derive(Debug, Deserialize)]
pub struct MemoryListQuery {
    pub profile_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Path helper
// ---------------------------------------------------------------------------

fn entries_path() -> PathBuf {
    paths::data_root().join("memory").join("entries.json")
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

fn load_store() -> MemoryStore {
    let path = entries_path();
    if !path.exists() {
        return MemoryStore {
            entries: Vec::new(),
        };
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => MemoryStore {
            entries: Vec::new(),
        },
    }
}

pub(crate) fn save_store(store: &MemoryStore) -> Result<(), String> {
    let path = entries_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())
}

/// Normalize tags: trim whitespace, remove empty strings, deduplicate, cap count.
fn normalize_tags(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<String> = raw
        .into_iter()
        .filter_map(|t| {
            let trimmed = t.trim().to_string();
            if trimmed.is_empty() || !seen.insert(trimmed.clone()) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect();
    out.truncate(MAX_TAG_COUNT);
    out
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/memory?profile_id=
pub async fn memory_list_handler(
    State(_state): State<WebState>,
    Query(query): Query<MemoryListQuery>,
) -> Json<MemoryListResponse> {
    let store = load_store();
    let mut entries = store.entries;
    // Sort newest first by updated_at (fallback to timestamp).
    entries.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    Json(MemoryListResponse {
        profile_id: query.profile_id,
        entries,
    })
}

/// PATCH /api/memory/:id
pub async fn memory_update_handler(
    AxumPath(id): AxumPath<String>,
    State(_state): State<WebState>,
    Json(req): Json<MemoryUpdateRequest>,
) -> Result<Json<MemoryUpdateResponse>, Response> {
    // Validate content length if provided.
    if let Some(ref content) = req.content {
        if content.len() > MAX_CONTENT_LENGTH {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(
                    ProtocolApiError::Validation {
                        field: "content".to_string(),
                        message: format!(
                            "Content exceeds maximum length of {MAX_CONTENT_LENGTH} characters"
                        ),
                    }
                    .into_body(),
                ),
            )
                .into_response());
        }
    }

    // Normalize tags if provided.
    let effective_tags = req.tags.map(normalize_tags);

    let mut store = load_store();

    let entry = store
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(
                    ProtocolApiError::NotFound {
                        entity: "memory",
                        id: format!("Memory entry '{}' not found", id),
                    }
                    .into_body(),
                ),
            )
                .into_response()
        })?;

    // Update fields, preserving timestamp/session_id/workspace.
    if let Some(content) = req.content {
        entry.content = content;
    }
    if let Some(tags) = effective_tags {
        entry.tags = tags;
    }
    if let Some(pinned) = req.pinned {
        entry.pinned = pinned;
    }
    entry.updated_at = Utc::now().to_rfc3339();

    // Clone the updated entry before saving (store is borrowed mutably above).
    let updated = entry.clone();

    save_store(&store).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                ProtocolApiError::Internal {
                    message: format!("Failed to persist memory store: {e}"),
                }
                .into_body(),
            ),
        )
            .into_response()
    })?;

    Ok(Json(MemoryUpdateResponse { entry: updated }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tags_trims_and_dedupes() {
        let raw = vec![
            "  foo  ".to_string(),
            "bar".to_string(),
            "  ".to_string(),
            "foo".to_string(),
            "BAR".to_string(),
            "bar".to_string(),
        ];
        let result = normalize_tags(raw);
        assert_eq!(result, vec!["foo", "bar", "BAR"]);
    }

    #[test]
    fn normalize_tags_caps_at_max() {
        let raw: Vec<String> = (0..100).map(|i| format!("tag-{i}")).collect();
        let result = normalize_tags(raw);
        assert_eq!(result.len(), MAX_TAG_COUNT);
    }

    #[test]
    fn normalize_tags_removes_empty() {
        let raw = vec![
            "a".to_string(),
            "".to_string(),
            "  ".to_string(),
            "b".to_string(),
        ];
        let result = normalize_tags(raw);
        assert_eq!(result, vec!["a", "b"]);
    }
}

#[cfg(test)]
#[path = "memory_api_tests.rs"]
mod api_tests;
