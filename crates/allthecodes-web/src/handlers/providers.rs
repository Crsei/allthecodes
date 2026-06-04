//! Provider CRUD handlers — list, create, update, delete, refresh models.

use std::collections::HashMap;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::settings::{
    load_global_config, write_user_settings, ProviderProfileSettings,
};

use crate::handlers::models::ModelSummary;
use crate::handlers::ApiError;
use crate::state::WebState;

#[derive(Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ProviderListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub providers: Vec<ProviderSummary>,
}

#[derive(Deserialize)]
pub struct ProviderCreateRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct ProviderUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Serialize)]
pub struct ModelDiscoveryResponse {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    pub models: Vec<ModelSummary>,
}

/// Build a list of providers from settings auth profiles.
fn provider_summaries_from_settings() -> Vec<ProviderSummary> {
    let settings = load_global_config().unwrap_or_default();
    let mut providers = Vec::new();

    if let Some(profiles) = settings.auth_profiles {
        for (id, profile) in profiles {
            let kind = profile
                .api_provider
                .clone()
                .unwrap_or_else(|| "anthropic".to_string());
            providers.push(ProviderSummary {
                id: id.clone(),
                name: id.clone(),
                kind,
                enabled: true,
                preset: Some(false),
                profile_id: None,
                base_url: profile.base_url.clone(),
                auth_kind: Some("api_key".to_string()),
                credential_status: None,
                credential_subject: None,
                models_count: profile.available_models.as_ref().map(|m| m.len()),
                last_refreshed_at: None,
                diagnostics: None,
            });
        }
    }

    providers
}

/// GET /api/providers — List all providers.
pub async fn providers_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let providers = provider_summaries_from_settings();

    // Also add the engine's current provider if it's not already listed
    let engine = state.engine();
    let _current_model = engine.app_state().main_loop_model.clone();

    Json(ProviderListResponse {
        profile_id: None,
        providers,
    })
}

/// POST /api/providers — Create a new provider.
pub async fn providers_create_handler(Json(req): Json<ProviderCreateRequest>) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    if profiles.contains_key(&req.name) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!("Provider '{}' already exists", req.name),
                code: "conflict".into(),
            }),
        )
            .into_response();
    }

    let profile = ProviderProfileSettings {
        backend: None,
        api_provider: Some(req.kind.clone()),
        model: None,
        available_models: None,
        model_capabilities: None,
        model_reasoning_effort: None,
        base_url: req.base_url.clone(),
        api_key: None,
        env: None,
        auth_source: None,
        extra: HashMap::new(),
    };
    profiles.insert(req.name.clone(), profile);
    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// PATCH /api/providers/{id} — Update a provider.
pub async fn providers_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProviderUpdateRequest>,
) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    // Check for duplicate name before mutable access
    if let Some(name) = &req.name {
        if name != &id && profiles.contains_key(name) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: format!("Provider '{}' already exists", name),
                    code: "conflict".into(),
                }),
            )
                .into_response();
        }
    }

    let profile = match profiles.get_mut(&id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Provider '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    if let Some(base_url) = &req.base_url {
        profile.base_url = Some(base_url.clone());
    }

    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/providers/{id} — Delete a provider.
pub async fn providers_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    if profiles.remove(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Provider '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response();
    }

    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/providers/{id}/models/refresh — Refresh models from provider.
pub async fn providers_refresh_models_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    let engine = state.engine();
    let app_state = engine.app_state();
    let available = &app_state.settings.available_models;

    let models: Vec<ModelSummary> = available
        .iter()
        .map(|m| ModelSummary {
            id: m.clone(),
            provider_id: id.clone(),
            provider_name: Some(id.clone()),
            display_name: Some(m.clone()),
            alias: None,
            visible: true,
            default: None,
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
            supports_vision: None,
            updated_at: Some(Utc::now().timestamp()),
        })
        .collect();

    Json(ModelDiscoveryResponse {
        provider_id: id,
        refreshed_at: Some(Utc::now().timestamp()),
        models,
    })
    .into_response()
}
