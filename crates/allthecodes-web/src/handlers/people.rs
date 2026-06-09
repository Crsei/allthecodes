//! People profile REST handlers.

use std::path::PathBuf;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::paths;

use allthecodes_protocol::ApiError as ProtocolApiError;

use allthecodes_protocol::v1::people::PeopleListResponse as ProtocolPeopleListResponse;
use allthecodes_protocol::v1::people::PersonMutationResponse as ProtocolPersonMutationResponse;
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
pub struct PeopleListProcessor {
    state: WebState,
}

impl From<WebState> for PeopleListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PeopleListProcessor {
    type Request = NoParams;
    type Response = ProtocolPeopleListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "people.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _request: NoParams) -> Result<Self::Response, Self::Error> {
        let people = load_people().map_err(|e| ProtocolApiError::Internal { message: e })?;
        let protocol_people: Result<Vec<_>, _> = people
            .into_iter()
            .map(|p| {
                serde_json::to_value(&p).and_then(|v| {
                    serde_json::from_value::<allthecodes_protocol::v1::people::PersonProfile>(v)
                })
            })
            .collect();
        Ok(ProtocolPeopleListResponse {
            people: protocol_people.map_err(|e| ProtocolApiError::Internal {
                message: e.to_string(),
            })?,
        })
    }
}

#[derive(Clone)]
pub struct PeopleCreateProcessor {
    state: WebState,
}

impl From<WebState> for PeopleCreateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for PeopleCreateProcessor {
    type Request = allthecodes_protocol::v1::people::PersonCreateRequest;
    type Response = ProtocolPersonMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "people.create"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        use allthecodes_protocol::v1::people::PersonProfile as ProtocolPersonProfile;

        let name = request.name.trim().to_string();
        if name.is_empty() {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "Person name cannot be empty".to_string(),
            });
        }

        let id = request
            .id
            .unwrap_or_else(|| slug_from_name(&name, "person"));
        if let Err(error) = validate_id(&id) {
            return Err(ProtocolApiError::BadRequest {
                code: "validation_error",
                message: error,
            });
        }
        if person_path(&id).exists() {
            return Err(ProtocolApiError::Conflict {
                reason: format!("Person '{}' already exists", id),
            });
        }

        let now = Utc::now().timestamp();
        let person = ProtocolPersonProfile {
            id,
            name,
            telegram_id: request.telegram_id,
            discord_id: request.discord_id,
            discord_username: request.discord_username,
            feishu_id: request.feishu_id,
            username: request.username,
            profile_content: request.profile_content,
            created_at: now,
            updated_at: now,
        };

        // Bridge to handler type for persistence
        let handler_person: PersonProfile =
            serde_json::from_value(serde_json::to_value(&person).map_err(|e| {
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
            })?)
            .map_err(|e| ProtocolApiError::Internal {
                message: e.to_string(),
            })?;

        write_person(&handler_person).map_err(|e| ProtocolApiError::Internal { message: e })?;

        Ok(ProtocolPersonMutationResponse {
            ok: true,
            person: Some(person),
            error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::PeopleList, get(people_list_handler))
        .handle(ApiMethod::PeopleCreate, post(people_create_handler))
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonProfile {
    pub id: String,
    pub name: String,
    pub telegram_id: Option<String>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub feishu_id: Option<String>,
    pub username: Option<String>,
    pub profile_content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub struct PeopleListResponse {
    pub people: Vec<PersonProfile>,
}

#[derive(Deserialize)]
pub struct PersonCreateRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub telegram_id: Option<String>,
    #[serde(default)]
    pub discord_id: Option<String>,
    #[serde(default)]
    pub discord_username: Option<String>,
    #[serde(default)]
    pub feishu_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub profile_content: String,
}

#[derive(Deserialize)]
pub struct PersonUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub telegram_id: Option<Option<String>>,
    #[serde(default)]
    pub discord_id: Option<Option<String>>,
    #[serde(default)]
    pub discord_username: Option<Option<String>>,
    #[serde(default)]
    pub feishu_id: Option<Option<String>>,
    #[serde(default)]
    pub username: Option<Option<String>>,
    #[serde(default)]
    pub profile_content: Option<String>,
}

/// GET /api/people
pub async fn people_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<PeopleListProcessor>(state, ApiMethod::PeopleList, NoParams {}).await
}

/// GET /api/people/{id}
pub async fn people_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    match load_person(&id) {
        Ok(Some(person)) => Json(person).into_response(),
        Ok(None) => not_found(format!("Person '{}' not found", id)),
        Err(error) => internal_error(error),
    }
}

/// POST /api/people
pub async fn people_create_handler(
    State(state): State<WebState>,
    Json(req): Json<allthecodes_protocol::v1::people::PersonCreateRequest>,
) -> Response {
    rest_processor_response::<PeopleCreateProcessor>(state, ApiMethod::PeopleCreate, req).await
}

/// PATCH /api/people/{id}
pub async fn people_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PersonUpdateRequest>,
) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    let mut person = match load_person(&id) {
        Ok(Some(person)) => person,
        Ok(None) => return not_found(format!("Person '{}' not found", id)),
        Err(error) => return internal_error(error),
    };

    if let Some(name) = req.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return validation_error("Person name cannot be empty".to_string());
        }
        person.name = name;
    }
    if let Some(value) = req.telegram_id {
        person.telegram_id = value;
    }
    if let Some(value) = req.discord_id {
        person.discord_id = value;
    }
    if let Some(value) = req.discord_username {
        person.discord_username = value;
    }
    if let Some(value) = req.feishu_id {
        person.feishu_id = value;
    }
    if let Some(value) = req.username {
        person.username = value;
    }
    if let Some(value) = req.profile_content {
        person.profile_content = value;
    }
    person.updated_at = Utc::now().timestamp();

    match write_person(&person) {
        Ok(()) => Json(person).into_response(),
        Err(error) => internal_error(error),
    }
}

/// DELETE /api/people/{id}
pub async fn people_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error);
    }
    let path = person_path(&id);
    if !path.exists() {
        return not_found(format!("Person '{}' not found", id));
    }
    if let Err(error) = std::fs::remove_file(&path).map_err(|err| err.to_string()) {
        return internal_error(error);
    }
    match load_people() {
        Ok(people) => Json(PeopleListResponse { people }).into_response(),
        Err(error) => internal_error(error),
    }
}

fn load_people() -> Result<Vec<PersonProfile>, String> {
    let dir = people_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut people = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let person = serde_json::from_str::<PersonProfile>(&raw).map_err(|err| err.to_string())?;
        people.push(person);
    }
    people.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(people)
}

fn load_person(id: &str) -> Result<Option<PersonProfile>, String> {
    let path = person_path(id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str::<PersonProfile>(&raw)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn write_person(person: &PersonProfile) -> Result<(), String> {
    let path = person_path(&person.id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(person).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn people_dir() -> PathBuf {
    paths::data_root().join("people")
}

fn person_path(id: &str) -> PathBuf {
    people_dir().join(format!("{id}.json"))
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
        entity: "person",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
