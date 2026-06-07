//! Quick prompt REST handlers.

use std::path::PathBuf;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::paths;

use crate::handlers::ApiError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuickPrompt {
    pub id: String,
    pub name: String,
    pub content: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickPromptStore {
    pub version: u32,
    pub prompts: Vec<QuickPrompt>,
}

#[derive(Serialize)]
pub struct PromptsListResponse {
    pub prompts: Vec<QuickPrompt>,
}

#[derive(Deserialize)]
pub struct PromptCreateRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub struct PromptUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// GET /api/prompts
pub async fn prompts_list_handler() -> Response {
    match load_store() {
        Ok(store) => Json(PromptsListResponse {
            prompts: sorted_prompts(store.prompts),
        })
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// GET /api/prompts/{id}
pub async fn prompts_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    match load_store() {
        Ok(store) => match store.prompts.into_iter().find(|prompt| prompt.id == id) {
            Some(prompt) => Json(prompt).into_response(),
            None => not_found(format!("Prompt '{}' not found", id)).into_response(),
        },
        Err(error) => internal_error(error).into_response(),
    }
}

/// POST /api/prompts
pub async fn prompts_create_handler(Json(req): Json<PromptCreateRequest>) -> Response {
    if let Err(error) = validate_prompt_name(&req.name) {
        return validation_error(error).into_response();
    }
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(error).into_response(),
    };
    let id = req
        .id
        .unwrap_or_else(|| slug_from_name(&req.name, "prompt"));
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    if store.prompts.iter().any(|prompt| prompt.id == id) {
        return conflict(format!("Prompt '{}' already exists", id)).into_response();
    }
    let now = Utc::now().timestamp();
    let prompt = QuickPrompt {
        id,
        name: req.name.trim().to_string(),
        content: req.content,
        description: req.description,
        created_at: now,
        updated_at: now,
    };
    store.prompts.push(prompt.clone());
    store.prompts = sorted_prompts(store.prompts);
    match write_store(&store) {
        Ok(()) => Json(prompt).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// PATCH /api/prompts/{id}
pub async fn prompts_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PromptUpdateRequest>,
) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(error).into_response(),
    };
    let Some(prompt) = store.prompts.iter_mut().find(|prompt| prompt.id == id) else {
        return not_found(format!("Prompt '{}' not found", id)).into_response();
    };
    if let Some(name) = req.name {
        if let Err(error) = validate_prompt_name(&name) {
            return validation_error(error).into_response();
        }
        prompt.name = name.trim().to_string();
    }
    if let Some(content) = req.content {
        prompt.content = content;
    }
    if let Some(description) = req.description {
        prompt.description = description;
    }
    prompt.updated_at = Utc::now().timestamp();
    let prompt = prompt.clone();
    store.prompts = sorted_prompts(store.prompts);
    match write_store(&store) {
        Ok(()) => Json(prompt).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// DELETE /api/prompts/{id}
pub async fn prompts_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(error).into_response(),
    };
    let before = store.prompts.len();
    store.prompts.retain(|prompt| prompt.id != id);
    if store.prompts.len() == before {
        return not_found(format!("Prompt '{}' not found", id)).into_response();
    }
    store.prompts = sorted_prompts(store.prompts);
    match write_store(&store) {
        Ok(()) => Json(PromptsListResponse {
            prompts: store.prompts,
        })
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

fn load_store() -> Result<QuickPromptStore, String> {
    let path = store_path();
    if !path.exists() {
        return Ok(QuickPromptStore {
            version: 1,
            prompts: Vec::new(),
        });
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str::<QuickPromptStore>(&raw).map_err(|err| err.to_string())
}

fn write_store(store: &QuickPromptStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(store).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn sorted_prompts(mut prompts: Vec<QuickPrompt>) -> Vec<QuickPrompt> {
    prompts.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    prompts
}

fn store_path() -> PathBuf {
    paths::data_root().join("quick-prompts.json")
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("id cannot be empty".to_string());
    }
    if id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        Ok(())
    } else {
        Err("id may only contain ASCII letters, digits, `-`, and `_`".to_string())
    }
}

fn validate_prompt_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Prompt name cannot be empty".to_string());
    }
    if trimmed.contains('/') {
        return Err("Prompt name must not contain `/`".to_string());
    }
    Ok(())
}

fn slug_from_name(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !last_dash && !out.is_empty() {
            out.push(if ch == '_' { '_' } else { '-' });
            last_dash = ch != '_';
        }
    }
    while out.ends_with('-') || out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

fn validation_error(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error,
            code: "validation_error".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn conflict(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::CONFLICT,
        Json(ApiError {
            error,
            code: "conflict".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn not_found(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error,
            code: "not_found".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn internal_error(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error,
            code: "internal_error".into(),

            details: serde_json::json!({}),
        }),
    )
}
