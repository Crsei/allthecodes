use std::collections::HashMap;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde_json::json;

use allthecodes_config::settings::{
    load_global_config, ModelCapabilitySettings, ProviderProfileReplacement,
    ProviderProfileSettings, ProviderProfileStore, ProviderProfileStoreError,
    ProviderSecretUpdates, SecretUpdate,
};
use allthecodes_protocol::ApiError as ProtocolApiError;

use crate::handlers::models::ModelSummary;
use crate::state::WebState;

use super::helpers::{
    normalize_models, normalized_json_object, normalized_non_empty, provider_kind_from_request,
    provider_presets, provider_summaries_from_settings,
};
use super::types::{
    ModelDiscoveryResponse, PatchField, ProviderCreateRequest, ProviderDetailResponse,
    ProviderListResponse, ProviderReplaceRequest, ProviderSecretUpdate, ProviderUpdateRequest,
};

fn list_response() -> Response {
    Json(ProviderListResponse {
        profile_id: None,
        providers: provider_summaries_from_settings(),
        presets: provider_presets(),
    })
    .into_response()
}

fn store_error(error: anyhow::Error) -> Response {
    let (status, body) = match error.downcast_ref::<ProviderProfileStoreError>() {
        Some(ProviderProfileStoreError::EmptyId) => (
            StatusCode::BAD_REQUEST,
            ProtocolApiError::BadRequest {
                code: "validation_error",
                message: error.to_string(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::AlreadyExists(_)) => (
            StatusCode::CONFLICT,
            ProtocolApiError::Conflict {
                reason: error.to_string(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::NotFound(id)) => (
            StatusCode::NOT_FOUND,
            ProtocolApiError::NotFound {
                entity: "provider",
                id: id.clone(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::ActiveProfile(_))
        | Some(ProviderProfileStoreError::UnsupportedProvider(_)) => (
            StatusCode::CONFLICT,
            ProtocolApiError::Conflict {
                reason: error.to_string(),
            }
            .into_body(),
        ),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ProtocolApiError::Internal {
                message: error.to_string(),
            }
            .into_body(),
        ),
    };
    (status, Json(body)).into_response()
}

fn detail_response(
    detail: allthecodes_config::settings::RedactedProviderProfile,
) -> ProviderDetailResponse {
    ProviderDetailResponse {
        id: detail.id,
        active: detail.active,
        runtime_support: detail.runtime_support,
        backend: detail.backend,
        api_provider: detail.api_provider,
        model: detail.model,
        available_models: detail.available_models,
        model_capabilities: detail.model_capabilities.map(|entries| {
            entries
                .into_iter()
                .map(|(id, capability)| {
                    (
                        id,
                        serde_json::to_value(capability).unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect()
        }),
        model_reasoning_effort: detail.model_reasoning_effort,
        base_url: detail.base_url,
        api_key_configured: detail.api_key_configured,
        env_keys: detail.env_keys,
        auth_source_configured: detail.auth_source_configured,
        extra: detail.extra,
    }
}

fn secret_update<T>(update: ProviderSecretUpdate<T>) -> SecretUpdate<T> {
    match update {
        ProviderSecretUpdate::Keep => SecretUpdate::Keep,
        ProviderSecretUpdate::Set(value) => SecretUpdate::Set(value),
        ProviderSecretUpdate::Clear => SecretUpdate::Clear,
    }
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
        presets: provider_presets(),
    })
}

/// POST /api/providers — Create a new provider.
pub async fn providers_create_handler(Json(req): Json<ProviderCreateRequest>) -> Response {
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
    match ProviderProfileStore::global().create(&req.name, profile) {
        Ok(()) => list_response(),
        Err(error) => store_error(error),
    }
}

/// GET /api/providers/{id} — Read a redacted provider profile.
pub async fn providers_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().get(&id) {
        Ok(detail) => Json(detail_response(detail)).into_response(),
        Err(error) => store_error(error),
    }
}

/// PUT /api/providers/{id} — Replace all non-secret provider profile fields.
pub async fn providers_replace_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProviderReplaceRequest>,
) -> Response {
    let model_capabilities = match req.model_capabilities {
        Some(entries) => {
            let mut parsed = HashMap::new();
            for (model, value) in entries {
                match serde_json::from_value::<ModelCapabilitySettings>(value) {
                    Ok(capability) => {
                        parsed.insert(model, capability);
                    }
                    Err(error) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(
                                ProtocolApiError::BadRequest {
                                    code: "validation_error",
                                    message: format!(
                                        "invalid model capability for `{model}`: {error}"
                                    ),
                                }
                                .into_body(),
                            ),
                        )
                            .into_response();
                    }
                }
            }
            Some(parsed)
        }
        None => None,
    };
    let replacement = ProviderProfileReplacement {
        profile: ProviderProfileSettings {
            backend: req.backend,
            api_provider: req.api_provider,
            model: req.model,
            available_models: req.available_models.map(normalize_models),
            model_capabilities,
            model_reasoning_effort: req.model_reasoning_effort,
            base_url: req
                .base_url
                .and_then(|value| normalized_non_empty(Some(&value))),
            api_key: None,
            env: None,
            auth_source: None,
            extra: req.extra,
        },
        secrets: ProviderSecretUpdates {
            api_key: secret_update(req.secret_updates.api_key),
            env: secret_update(req.secret_updates.env),
            auth_source: secret_update(req.secret_updates.auth_source),
        },
    };
    let store = ProviderProfileStore::global();
    match store
        .replace(&id, replacement)
        .and_then(|()| store.get(&id))
    {
        Ok(detail) => Json(detail_response(detail)).into_response(),
        Err(error) => store_error(error),
    }
}

/// PATCH /api/providers/{id} — Update a provider.
pub async fn providers_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProviderUpdateRequest>,
) -> Response {
    let new_name = req.name.clone();
    let result =
        ProviderProfileStore::global().update_and_rename(&id, new_name.as_deref(), |profile| {
            if let Some(enabled) = req.enabled {
                profile.extra.insert("enabled".to_string(), json!(enabled));
            }
            apply_optional_string(&mut profile.base_url, req.base_url);
            apply_optional_string(&mut profile.api_key, req.api_key);
            apply_patch(&mut profile.env, req.env);
            match req.command {
                PatchField::Missing => {}
                PatchField::Null => {
                    profile.extra.remove("command");
                    profile.extra.remove("providerType");
                }
                PatchField::Value(command) => {
                    if let Some(command) = normalized_non_empty(Some(&command)) {
                        profile.extra.insert("command".to_string(), json!(command));
                        profile
                            .extra
                            .insert("providerType".to_string(), json!("acp"));
                    } else {
                        profile.extra.remove("command");
                    }
                }
            }
            apply_extra_patch(&mut profile.extra, "arguments", req.arguments);
            match req.models {
                PatchField::Missing => {}
                PatchField::Null => profile.available_models = None,
                PatchField::Value(models) => {
                    profile.available_models = Some(normalize_models(models))
                }
            }
            match req.provider_options {
                PatchField::Missing => {}
                PatchField::Null => {
                    profile.extra.remove("providerOptions");
                }
                PatchField::Value(value) => {
                    if let Some(value) = normalized_json_object(value) {
                        profile.extra.insert("providerOptions".to_string(), value);
                    }
                }
            }
        });
    match result {
        Ok(_) => list_response(),
        Err(error) => store_error(error),
    }
}

/// DELETE /api/providers/{id} — Delete a provider.
pub async fn providers_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().delete(&id) {
        Ok(()) => list_response(),
        Err(error) => store_error(error),
    }
}

fn apply_optional_string(target: &mut Option<String>, patch: PatchField<String>) {
    match patch {
        PatchField::Missing => {}
        PatchField::Null => *target = None,
        PatchField::Value(value) => *target = normalized_non_empty(Some(&value)),
    }
}

fn apply_patch<T>(target: &mut Option<T>, patch: PatchField<T>) {
    match patch {
        PatchField::Missing => {}
        PatchField::Null => *target = None,
        PatchField::Value(value) => *target = Some(value),
    }
}

fn apply_extra_patch<T: serde::Serialize>(
    extra: &mut HashMap<String, serde_json::Value>,
    key: &str,
    patch: PatchField<T>,
) {
    match patch {
        PatchField::Missing => {}
        PatchField::Null => {
            extra.remove(key);
        }
        PatchField::Value(value) => {
            extra.insert(key.to_string(), json!(value));
        }
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
