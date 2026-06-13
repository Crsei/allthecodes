use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put, MethodRouter};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use allthecodes_web_state::{
    normalize_owner, LayoutCreate, LayoutUpdate, PromptCreate, PromptFilter, PromptUpdate,
    ThemeSchemeCreate, ThemeSchemeUpdate,
};

use crate::api_errors::api_error_body;
use crate::state::WebState;

fn versioned_web_path(path: &str) -> String {
    path.replacen("/api/web", "/api/v2/web", 1)
}

pub fn routes() -> Router<WebState> {
    let mut web_routes: Vec<(String, MethodRouter<WebState>)> = Vec::new();
    for (path, method_router) in raw_routes() {
        web_routes.push((path.to_string(), method_router.clone()));
        web_routes.push((versioned_web_path(path), method_router));
    }
    let mut router = Router::new();
    for (path, method_router) in web_routes {
        router = router.route(&path, method_router);
    }
    router
}

fn raw_routes() -> Vec<(&'static str, MethodRouter<WebState>)> {
    vec![
        ("/api/web/health", get(health).into()),
        ("/api/web/preferences/fields", get(preference_fields).into()),
        (
            "/api/web/preferences",
            get(get_preferences).put(update_preferences).into(),
        ),
        (
            "/api/web/themes",
            get(list_themes).post(create_theme).into(),
        ),
        (
            "/api/web/themes/{id}",
            put(update_theme).delete(delete_theme).into(),
        ),
        (
            "/api/web/prompts",
            get(list_prompts).post(create_prompt).into(),
        ),
        (
            "/api/web/prompts/{id}",
            put(update_prompt).delete(delete_prompt).into(),
        ),
        (
            "/api/web/layouts",
            get(list_layouts).post(create_layout).into(),
        ),
        (
            "/api/web/layouts/{id}",
            put(update_layout).delete(delete_layout).into(),
        ),
        (
            "/api/web/layouts/{id}/set-default",
            post(set_default_layout).into(),
        ),
    ]
}

#[derive(Deserialize)]
struct ProfileQuery {
    #[serde(default)]
    profile_id: Option<String>,
}

#[derive(Deserialize)]
struct PreferenceFieldsQuery {
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    keys: Option<String>,
}

#[derive(Deserialize)]
struct PromptQuery {
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    favorite: Option<bool>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: String,
    db: &'static str,
}

#[derive(Serialize)]
struct ThemesResponse<T> {
    schemes: T,
}

#[derive(Serialize)]
struct ThemeResponse<T> {
    scheme: T,
}

#[derive(Serialize)]
struct PromptResponse<T> {
    prompt: T,
}

#[derive(Serialize)]
struct LayoutsResponse<T> {
    layouts: T,
}

#[derive(Serialize)]
struct LayoutResponse<T> {
    layout: T,
}

async fn health(State(state): State<WebState>) -> Response {
    match state.web_ui_store.health().await {
        Ok(()) => Json(HealthResponse {
            status: "ok",
            version: state.app_version().to_string(),
            db: "connected",
        })
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn get_preferences(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
) -> Response {
    match state
        .web_ui_store
        .get_preferences(&owner(query.profile_id))
        .await
    {
        Ok(preferences) => Json(preferences).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn update_preferences(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, patch) = split_profile_id(body);
    let owner = owner(body_profile_id.or(query.profile_id));
    match state.web_ui_store.update_preferences(&owner, patch).await {
        Ok(preferences) => Json(preferences).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn preference_fields(
    State(state): State<WebState>,
    Query(query): Query<PreferenceFieldsQuery>,
) -> Response {
    let keys = query
        .keys
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    match state
        .web_ui_store
        .preference_fields(&owner(query.profile_id), &keys)
        .await
    {
        Ok(fields) => Json(fields).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn list_themes(State(state): State<WebState>, Query(query): Query<ProfileQuery>) -> Response {
    match state
        .web_ui_store
        .list_themes(&owner(query.profile_id))
        .await
    {
        Ok(schemes) => Json(ThemesResponse { schemes }).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn create_theme(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<ThemeSchemeCreate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .create_theme(&owner(body_profile_id.or(query.profile_id)), req)
        .await
    {
        Ok(scheme) => (StatusCode::CREATED, Json(ThemeResponse { scheme })).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn update_theme(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<ThemeSchemeUpdate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .update_theme(&owner(body_profile_id.or(query.profile_id)), &id, req)
        .await
    {
        Ok(Some(scheme)) => Json(ThemeResponse { scheme }).into_response(),
        Ok(None) => not_found(format!("Theme scheme '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn delete_theme(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .web_ui_store
        .delete_theme(&owner(query.profile_id), &id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(format!("Theme scheme '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn list_prompts(State(state): State<WebState>, Query(query): Query<PromptQuery>) -> Response {
    let owner = owner(query.profile_id);
    let filter = PromptFilter {
        category: query.category,
        tag: query.tag,
        favorite: query.favorite,
        limit: query.limit,
        offset: query.offset,
    };
    match state.web_ui_store.list_prompts(&owner, filter).await {
        Ok(list) => Json(list).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn create_prompt(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<PromptCreate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .create_prompt(&owner(body_profile_id.or(query.profile_id)), req)
        .await
    {
        Ok(prompt) => (StatusCode::CREATED, Json(PromptResponse { prompt })).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn update_prompt(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<PromptUpdate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .update_prompt(&owner(body_profile_id.or(query.profile_id)), &id, req)
        .await
    {
        Ok(Some(prompt)) => Json(PromptResponse { prompt }).into_response(),
        Ok(None) => not_found(format!("Prompt '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn delete_prompt(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .web_ui_store
        .delete_prompt(&owner(query.profile_id), &id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(format!("Prompt '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn list_layouts(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
) -> Response {
    match state
        .web_ui_store
        .list_layouts(&owner(query.profile_id))
        .await
    {
        Ok(layouts) => Json(LayoutsResponse { layouts }).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn create_layout(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<LayoutCreate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .create_layout(&owner(body_profile_id.or(query.profile_id)), req)
        .await
    {
        Ok(layout) => (StatusCode::CREATED, Json(LayoutResponse { layout })).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn update_layout(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let (body_profile_id, body) = split_profile_id(body);
    let req = match serde_json::from_value::<LayoutUpdate>(body) {
        Ok(req) => req,
        Err(error) => return validation_error(error).into_response(),
    };
    match state
        .web_ui_store
        .update_layout(&owner(body_profile_id.or(query.profile_id)), &id, req)
        .await
    {
        Ok(Some(layout)) => Json(LayoutResponse { layout }).into_response(),
        Ok(None) => not_found(format!("Workspace layout '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn set_default_layout(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .web_ui_store
        .set_default_layout(&owner(query.profile_id), &id)
        .await
    {
        Ok(Some(layout)) => Json(LayoutResponse { layout }).into_response(),
        Ok(None) => not_found(format!("Workspace layout '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn delete_layout(
    State(state): State<WebState>,
    Query(query): Query<ProfileQuery>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state
        .web_ui_store
        .delete_layout(&owner(query.profile_id), &id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(format!("Workspace layout '{}' not found", id)).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

fn owner(profile_id: Option<String>) -> String {
    normalize_owner(profile_id.as_deref().unwrap_or_default())
}

fn split_profile_id(body: Value) -> (Option<String>, Value) {
    let Value::Object(mut map) = body else {
        return (None, json!({}));
    };
    let profile_id =
        take_string(&mut map, "profile_id").or_else(|| take_string(&mut map, "profileId"));
    (profile_id, Value::Object(map))
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    match map.remove(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn validation_error(
    error: impl std::fmt::Display,
) -> (StatusCode, Json<crate::api_errors::ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(api_error_body(error.to_string(), "validation_error")),
    )
}

fn not_found(error: impl Into<String>) -> (StatusCode, Json<crate::api_errors::ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(api_error_body(error.into(), "not_found")),
    )
}

fn internal_error(
    error: impl std::fmt::Display,
) -> (StatusCode, Json<crate::api_errors::ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(api_error_body(error.to_string(), "internal_error")),
    )
}
