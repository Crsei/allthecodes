//! Quick prompt REST handlers.

use std::path::PathBuf;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::paths;

use allthecodes_protocol::ApiError as ProtocolApiError;

use allthecodes_protocol::v1::prompts::PromptMutationResponse as ProtocolPromptMutationResponse;
use allthecodes_protocol::v1::prompts::PromptsListResponse as ProtocolPromptsListResponse;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::NoParams;
use async_trait::async_trait;
use axum::extract::State;
use axum::routing::{get, post};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PromptsListProcessor {
    state: WebState,
}

impl From<WebState> for PromptsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PromptsListProcessor {
    type Request = NoParams;
    type Response = ProtocolPromptsListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "prompts.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let store = load_store().map_err(|e| ProtocolApiError::Internal { message: e })?;
        let prompts: Vec<allthecodes_protocol::v1::prompts::QuickPrompt> =
            sorted_prompts(store.prompts)
                .into_iter()
                .map(|p| serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap())
                .collect();
        Ok(ProtocolPromptsListResponse { prompts })
    }
}

#[derive(Clone)]
pub struct PromptsCreateProcessor {
    state: WebState,
}

impl From<WebState> for PromptsCreateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PromptsCreateProcessor {
    type Request = allthecodes_protocol::v1::prompts::PromptCreateRequest;
    type Response = ProtocolPromptMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "prompts.create"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        use allthecodes_protocol::v1::prompts::QuickPrompt as ProtocolQuickPrompt;

        let trimmed = request.name.trim();
        if trimmed.is_empty() {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "Prompt name cannot be empty".to_string(),
            });
        }
        if trimmed.contains('/') {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "Prompt name must not contain `/`".to_string(),
            });
        }

        let mut store = load_store().map_err(|e| ProtocolApiError::Internal { message: e })?;
        let id = request
            .id
            .unwrap_or_else(|| slug_from_name(&request.name, "prompt"));
        if id.is_empty() {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "id cannot be empty".to_string(),
            });
        }
        if id
            .chars()
            .any(|ch| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "id may only contain ASCII letters, digits, `-`, and `_`".to_string(),
            });
        }
        if store.prompts.iter().any(|p| p.id == id) {
            return Err(ProtocolApiError::Conflict {
                reason: format!("Prompt '{}' already exists", id),
            });
        }

        let now = Utc::now().timestamp();
        let prompt = ProtocolQuickPrompt {
            id,
            name: trimmed.to_string(),
            content: request.content,
            description: request.description,
            created_at: now,
            updated_at: now,
        };

        // Bridge to handler type for persistence
        let handler_prompt: QuickPrompt =
            serde_json::from_value(serde_json::to_value(&prompt).map_err(|e| {
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
            })?)
            .map_err(|e| ProtocolApiError::Internal {
                message: e.to_string(),
            })?;

        store.prompts.push(handler_prompt);
        store.prompts = sorted_prompts(store.prompts);
        write_store(&store).map_err(|e| ProtocolApiError::Internal { message: e })?;

        Ok(ProtocolPromptMutationResponse {
            ok: true,
            prompt: Some(prompt),
            error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::PromptsList, get(prompts_list_handler))
        .handle(ApiMethod::PromptsCreate, post(prompts_create_handler))
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

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
pub async fn prompts_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PromptsListProcessor>(state, ApiMethod::PromptsList, NoParams {})
        .await
}

/// GET /api/prompts/{id}
pub async fn prompts_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    match load_store() {
        Ok(store) => match store.prompts.into_iter().find(|prompt| prompt.id == id) {
            Some(prompt) => Json(prompt).into_response(),
            None => not_found(format!("Prompt '{}' not found", id)),
        },
        Err(error) => internal_error(error),
    }
}

/// POST /api/prompts
pub async fn prompts_create_handler(
    State(state): State<WebState>,
    Json(req): Json<allthecodes_protocol::v1::prompts::PromptCreateRequest>,
) -> Response {
    rest_processor_response::<PromptsCreateProcessor>(state, ApiMethod::PromptsCreate, req).await
}

/// PATCH /api/prompts/{id}
pub async fn prompts_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PromptUpdateRequest>,
) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(error),
    };
    let Some(prompt) = store.prompts.iter_mut().find(|prompt| prompt.id == id) else {
        return not_found(format!("Prompt '{}' not found", id));
    };
    if let Some(name) = req.name {
        if let Err(error) = validate_prompt_name(&name) {
            return validation_error(error);
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
        Err(error) => internal_error(error),
    }
}

/// DELETE /api/prompts/{id}
pub async fn prompts_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    let mut store = match load_store() {
        Ok(store) => store,
        Err(error) => return internal_error(error),
    };
    let before = store.prompts.len();
    store.prompts.retain(|prompt| prompt.id != id);
    if store.prompts.len() == before {
        return not_found(format!("Prompt '{}' not found", id));
    }
    store.prompts = sorted_prompts(store.prompts);
    match write_store(&store) {
        Ok(()) => Json(PromptsListResponse {
            prompts: store.prompts,
        })
        .into_response(),
        Err(error) => internal_error(error),
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

fn validation_error(error: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "validation_error",
        message: error,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn conflict(error: String) -> Response {
    let body = ProtocolApiError::Conflict { reason: error }.into_body();
    (StatusCode::CONFLICT, Json(body)).into_response()
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "prompt",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
