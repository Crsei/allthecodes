use std::collections::HashMap;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde_json::json;

use allthecodes_config::settings::{
    load_global_config, write_user_settings, ProviderProfileSettings,
};
use allthecodes_protocol::ApiError as ProtocolApiError;

use crate::handlers::models::ModelSummary;
use crate::state::WebState;

use super::helpers::{
    normalize_models, normalized_json_object, normalized_non_empty, provider_kind_from_request,
    provider_presets, provider_summaries_from_settings,
};
use super::types::{
    ModelDiscoveryResponse, ProviderCreateRequest, ProviderListResponse, ProviderUpdateRequest,
};

/// GET /api/providers — List all providers.
pub async fn providers_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let providers = provider_summaries_from_settings();

    // Also add the engine's current provider if it's not already listed
    let engine = state.engine();
    let _current_model = engine.app_state().main_loop_model.clone();

    Json(ProviderListResponse {
        profile_id: None,
        providers,
        presets: provider_presets(),
    })
}

/// POST /api/providers — Create a new provider.
pub async fn providers_create_handler(Json(req): Json<ProviderCreateRequest>) -> Response {
    let mut settings = load_global_config().unwrap_or_default();
    let mut profiles = settings.auth_profiles.clone().unwrap_or_default();

    if profiles.contains_key(&req.name) {
        return (
            StatusCode::CONFLICT,
            Json(
                ProtocolApiError::Conflict {
                    reason: format!("Provider '{}' already exists", req.name),
                }
                .into_body(),
            ),
        )
            .into_response();
    }

    let mut extra = HashMap::new();
    extra.insert("enabled".to_string(), json!(req.enabled.unwrap_or(false)));
    if req.kind == "custom_acp" || req.kind == "acp" {
        extra.insert("providerType".to_string(), json!("acp"));
    }
    if let Some(command) = normalized_non_empty(req.command.as_deref()) {
        extra.insert("command".to_string(), json!(command));
    }
    if let Some(arguments) = req.arguments.clone() {
        extra.insert("arguments".to_string(), json!(arguments));
    }
    if let Some(provider_options) = req
        .provider_options
        .clone()
        .and_then(normalized_json_object)
    {
        extra.insert("providerOptions".to_string(), provider_options);
    }

    let kind = provider_kind_from_request(&req.kind, req.preset_id.as_deref());
    let profile = ProviderProfileSettings {
        backend: None,
        api_provider: Some(kind),
        model: None,
        available_models: req.models.map(normalize_models),
        model_capabilities: None,
        model_reasoning_effort: None,
        base_url: req.base_url.clone(),
        api_key: normalized_non_empty(req.api_key.as_deref()),
        env: req.env,
        auth_source: None,
        extra,
    };
    profiles.insert(req.name.clone(), profile);
    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
                presets: provider_presets(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
                .into_body(),
            ),
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
                Json(
                    ProtocolApiError::Conflict {
                        reason: format!("Provider '{}' already exists", name),
                    }
                    .into_body(),
                ),
            )
                .into_response();
        }
    }

    if !profiles.contains_key(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(
                ProtocolApiError::NotFound {
                    entity: "provider",
                    id: format!("Provider '{}' not found", id),
                }
                .into_body(),
            ),
        )
            .into_response();
    }

    if let Some(name) = &req.name {
        if name != &id {
            if let Some(renamed) = profiles.remove(&id) {
                profiles.insert(name.clone(), renamed);
            }
        }
    }

    let target_id = req.name.clone().unwrap_or(id.clone());
    let profile = match profiles.get_mut(&target_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    ProtocolApiError::NotFound {
                        entity: "provider",
                        id: format!("Provider '{}' not found", target_id),
                    }
                    .into_body(),
                ),
            )
                .into_response();
        }
    };

    if let Some(enabled) = req.enabled {
        profile.extra.insert("enabled".to_string(), json!(enabled));
    }
    if let Some(base_url) = req.base_url {
        profile.base_url = normalized_non_empty(Some(base_url.as_str()));
    }
    if let Some(api_key) = req.api_key {
        profile.api_key = normalized_non_empty(Some(api_key.as_str()));
    }
    if let Some(env) = req.env {
        profile.env = Some(env);
    }
    if let Some(command) = normalized_non_empty(req.command.as_deref()) {
        profile.extra.insert("command".to_string(), json!(command));
        profile
            .extra
            .insert("providerType".to_string(), json!("acp"));
    }
    if let Some(arguments) = req.arguments {
        profile
            .extra
            .insert("arguments".to_string(), json!(arguments));
    }
    if let Some(models) = req.models {
        profile.available_models = Some(normalize_models(models));
    }
    if let Some(provider_options) = req.provider_options.and_then(normalized_json_object) {
        profile
            .extra
            .insert("providerOptions".to_string(), provider_options);
    }

    settings.auth_profiles = Some(profiles);

    match write_user_settings(&settings) {
        Ok(_) => {
            let providers = provider_summaries_from_settings();
            Json(ProviderListResponse {
                profile_id: None,
                providers,
                presets: provider_presets(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
                .into_body(),
            ),
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
            Json(
                ProtocolApiError::NotFound {
                    entity: "provider",
                    id: format!("Provider '{}' not found", id),
                }
                .into_body(),
            ),
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
                presets: provider_presets(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
                .into_body(),
            ),
        )
            .into_response(),
    }
}

/// POST /api/providers/{id}/models/refresh — Refresh models from provider.
pub async fn providers_refresh_models_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    let settings = load_global_config().unwrap_or_default();
    let profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&id));
    let preset = provider_presets()
        .into_iter()
        .find(|preset| preset.id == id);
    let default_model = profile
        .and_then(|profile| profile.model.clone())
        .or_else(|| profile.and_then(|profile| profile.available_models.as_ref()?.first().cloned()))
        .or_else(|| preset.and_then(|preset| preset.default_model));
    let available = profile
        .and_then(|profile| profile.available_models.clone())
        .filter(|models| !models.is_empty())
        .or_else(|| default_model.map(|model| vec![model]))
        .unwrap_or_else(|| state.engine().app_state().settings.available_models.clone());

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
            supports_reasoning: None,
            supports_image_output: None,
            supports_embedding: None,
            provider_options: None,
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
