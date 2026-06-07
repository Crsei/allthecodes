//! User-level hooks REST handlers.

use std::collections::HashMap;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_config::settings::{load_global_config, write_user_settings, RawSettings};

use crate::handlers::ApiError;

const KNOWN_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "WorktreeCreate",
    "WorktreeRemove",
    "Notification",
];

#[derive(Serialize)]
pub struct HooksListResponse {
    pub hooks: HashMap<String, Vec<Value>>,
    pub known_events: Vec<String>,
}

#[derive(Serialize)]
pub struct HookEventResponse {
    pub event: String,
    pub configs: Vec<Value>,
}

#[derive(Deserialize)]
pub struct HookEventRequest {
    pub event: String,
    pub configs: Vec<Value>,
}

#[derive(Deserialize)]
pub struct HookEventUpdateRequest {
    pub configs: Vec<Value>,
}

/// GET /api/hooks
pub async fn hooks_list_handler() -> Response {
    match load_user_hooks() {
        Ok(hooks) => Json(HooksListResponse {
            hooks,
            known_events: known_events(),
        })
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// GET /api/hooks/{event}
pub async fn hooks_detail_handler(AxumPath(event): AxumPath<String>) -> Response {
    if let Err(error) = validate_event(&event) {
        return validation_error(error).into_response();
    }
    match load_user_hooks() {
        Ok(hooks) => match hooks.get(&event) {
            Some(configs) => Json(HookEventResponse {
                event,
                configs: configs.clone(),
            })
            .into_response(),
            None => not_found(format!("Hook event '{}' not found", event)).into_response(),
        },
        Err(error) => internal_error(error).into_response(),
    }
}

/// POST /api/hooks
pub async fn hooks_create_handler(Json(req): Json<HookEventRequest>) -> Response {
    if let Err(error) = validate_event(&req.event).and_then(|_| validate_configs(&req.configs)) {
        return validation_error(error).into_response();
    }
    match mutate_user_hooks(|raw| {
        let hooks = raw.hooks.get_or_insert_with(HashMap::new);
        if hooks.contains_key(&req.event) {
            return Err(MutationError::Conflict(format!(
                "Hook event '{}' already exists",
                req.event
            )));
        }
        hooks.insert(req.event.clone(), Value::Array(req.configs.clone()));
        Ok(())
    }) {
        Ok(()) => Json(HookEventResponse {
            event: req.event,
            configs: req.configs,
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}

/// PATCH /api/hooks/{event}
pub async fn hooks_update_handler(
    AxumPath(event): AxumPath<String>,
    Json(req): Json<HookEventUpdateRequest>,
) -> Response {
    if let Err(error) = validate_event(&event).and_then(|_| validate_configs(&req.configs)) {
        return validation_error(error).into_response();
    }
    match mutate_user_hooks(|raw| {
        let hooks = raw.hooks.get_or_insert_with(HashMap::new);
        if !hooks.contains_key(&event) {
            return Err(MutationError::NotFound(format!(
                "Hook event '{}' not found",
                event
            )));
        }
        hooks.insert(event.clone(), Value::Array(req.configs.clone()));
        Ok(())
    }) {
        Ok(()) => Json(HookEventResponse {
            event,
            configs: req.configs,
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}

/// DELETE /api/hooks/{event}
pub async fn hooks_delete_handler(AxumPath(event): AxumPath<String>) -> Response {
    if let Err(error) = validate_event(&event) {
        return validation_error(error).into_response();
    }
    match mutate_user_hooks(|raw| {
        let hooks = raw.hooks.get_or_insert_with(HashMap::new);
        if hooks.remove(&event).is_none() {
            return Err(MutationError::NotFound(format!(
                "Hook event '{}' not found",
                event
            )));
        }
        Ok(())
    }) {
        Ok(()) => match load_user_hooks() {
            Ok(hooks) => Json(HooksListResponse {
                hooks,
                known_events: known_events(),
            })
            .into_response(),
            Err(error) => internal_error(error).into_response(),
        },
        Err(error) => error.into_response(),
    }
}

/// POST /api/hooks/test
pub async fn hooks_test_handler(Json(req): Json<HookEventRequest>) -> Response {
    if let Err(error) = validate_event(&req.event).and_then(|_| validate_configs(&req.configs)) {
        return validation_error(error).into_response();
    }
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiError {
            error: "Hook test execution is not implemented".to_string(),
            code: "hook_test_not_implemented".into(),

            details: serde_json::json!({}),
        }),
    )
        .into_response()
}

fn load_user_hooks() -> Result<HashMap<String, Vec<Value>>, String> {
    let raw = load_global_config().map_err(|err| err.to_string())?;
    Ok(normalize_hooks(raw.hooks.unwrap_or_default()))
}

fn mutate_user_hooks<F>(mutator: F) -> Result<(), MutationError>
where
    F: FnOnce(&mut RawSettings) -> Result<(), MutationError>,
{
    let mut raw = load_global_config().map_err(|err| MutationError::Internal(err.to_string()))?;
    mutator(&mut raw)?;
    write_user_settings(&raw)
        .map(|_| ())
        .map_err(|err| MutationError::Internal(err.to_string()))
}

fn normalize_hooks(raw: HashMap<String, Value>) -> HashMap<String, Vec<Value>> {
    raw.into_iter()
        .map(|(event, value)| {
            let configs = match value {
                Value::Array(items) => items,
                Value::Null => Vec::new(),
                other => vec![other],
            };
            (event, configs)
        })
        .collect()
}

fn validate_event(event: &str) -> Result<(), String> {
    if event.trim().is_empty() {
        Err("hook event cannot be empty".to_string())
    } else {
        Ok(())
    }
}

fn validate_configs(configs: &[Value]) -> Result<(), String> {
    for (idx, config) in configs.iter().enumerate() {
        let Some(object) = config.as_object() else {
            return Err(format!("hook config at index {idx} must be an object"));
        };
        match object.get("hooks") {
            Some(Value::Array(_)) => {}
            _ => {
                return Err(format!(
                    "hook config at index {idx} must contain a hooks array"
                ))
            }
        }
    }
    Ok(())
}

fn known_events() -> Vec<String> {
    KNOWN_EVENTS
        .iter()
        .map(|event| (*event).to_string())
        .collect()
}

enum MutationError {
    Conflict(String),
    NotFound(String),
    Internal(String),
}

impl MutationError {
    fn into_response(self) -> Response {
        match self {
            MutationError::Conflict(error) => conflict(error).into_response(),
            MutationError::NotFound(error) => not_found(error).into_response(),
            MutationError::Internal(error) => internal_error(error).into_response(),
        }
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
