//! People profile REST handlers.

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
pub async fn people_list_handler() -> Response {
    match load_people() {
        Ok(people) => Json(PeopleListResponse { people }).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// GET /api/people/{id}
pub async fn people_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    match load_person(&id) {
        Ok(Some(person)) => Json(person).into_response(),
        Ok(None) => not_found(format!("Person '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// POST /api/people
pub async fn people_create_handler(Json(req): Json<PersonCreateRequest>) -> Response {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return validation_error("Person name cannot be empty".to_string()).into_response();
    }

    let id = req.id.unwrap_or_else(|| slug_from_name(&name, "person"));
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    if person_path(&id).exists() {
        return conflict(format!("Person '{}' already exists", id)).into_response();
    }

    let now = Utc::now().timestamp();
    let person = PersonProfile {
        id,
        name,
        telegram_id: req.telegram_id,
        discord_id: req.discord_id,
        discord_username: req.discord_username,
        feishu_id: req.feishu_id,
        username: req.username,
        profile_content: req.profile_content,
        created_at: now,
        updated_at: now,
    };

    match write_person(&person) {
        Ok(()) => Json(person).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// PATCH /api/people/{id}
pub async fn people_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<PersonUpdateRequest>,
) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    let mut person = match load_person(&id) {
        Ok(Some(person)) => person,
        Ok(None) => return not_found(format!("Person '{}' not found", id)).into_response(),
        Err(error) => return internal_error(error).into_response(),
    };

    if let Some(name) = req.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return validation_error("Person name cannot be empty".to_string()).into_response();
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
        Err(error) => internal_error(error).into_response(),
    }
}

/// DELETE /api/people/{id}
pub async fn people_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    if let Err(error) = validate_id(&id) {
        return validation_error(error).into_response();
    }
    let path = person_path(&id);
    if !path.exists() {
        return not_found(format!("Person '{}' not found", id)).into_response();
    }
    if let Err(error) = std::fs::remove_file(&path).map_err(|err| err.to_string()) {
        return internal_error(error).into_response();
    }
    match load_people() {
        Ok(people) => Json(PeopleListResponse { people }).into_response(),
        Err(error) => internal_error(error).into_response(),
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
