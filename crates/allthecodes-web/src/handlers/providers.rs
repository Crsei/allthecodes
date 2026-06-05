//! Provider CRUD handlers — list, create, update, delete, refresh models.

use std::collections::HashMap;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use allthecodes_config::settings::{
    load_global_config, write_user_settings, ModelCapabilitySettings, ProviderProfileSettings,
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
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub auth_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    pub supported: bool,
}

#[derive(Serialize)]
pub struct ProviderListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub providers: Vec<ProviderSummary>,
    pub presets: Vec<ProviderPreset>,
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
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct ProviderUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub models: Option<Vec<String>>,
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
            let enabled = profile_enabled(&profile);
            providers.push(ProviderSummary {
                id: id.clone(),
                name: id.clone(),
                kind,
                enabled,
                preset: Some(false),
                profile_id: None,
                base_url: profile.base_url.clone(),
                auth_kind: Some(profile_auth_kind(&profile)),
                credential_status: Some(profile_credential_status(&profile)),
                credential_subject: None,
                models_count: profile.available_models.as_ref().map(|m| m.len()),
                last_refreshed_at: None,
                diagnostics: provider_diagnostics(&profile),
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
            Json(ApiError {
                error: format!("Provider '{}' already exists", req.name),
                code: "conflict".into(),
            }),
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

    if !profiles.contains_key(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Provider '{}' not found", id),
                code: "not_found".into(),
            }),
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
                Json(ApiError {
                    error: format!("Provider '{}' not found", target_id),
                    code: "not_found".into(),
                }),
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
                presets: provider_presets(),
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

pub(crate) fn configured_provider_models() -> Vec<ModelSummary> {
    let settings = load_global_config().unwrap_or_default();
    let mut models = Vec::new();
    let presets = provider_presets();

    if let Some(profiles) = settings.auth_profiles {
        for (provider_id, profile) in profiles {
            let provider_name = provider_id.clone();
            let available = profile
                .available_models
                .clone()
                .filter(|items| !items.is_empty())
                .or_else(|| profile.model.clone().map(|model| vec![model]))
                .or_else(|| {
                    let kind = profile.api_provider.as_deref()?;
                    presets
                        .iter()
                        .find(|preset| preset.id == kind)
                        .and_then(|preset| preset.default_model.clone())
                        .map(|model| vec![model])
                })
                .unwrap_or_default();

            for model in available {
                let capability = profile
                    .model_capabilities
                    .as_ref()
                    .and_then(|capabilities| capabilities.get(&model));
                models.push(ModelSummary {
                    id: model.clone(),
                    provider_id: provider_id.clone(),
                    provider_name: Some(provider_name.clone()),
                    display_name: capability
                        .and_then(|capability| capability.display_name.clone())
                        .or_else(|| Some(model.clone())),
                    alias: capability.and_then(|capability| capability.description.clone()),
                    visible: profile_enabled(&profile),
                    default: None,
                    context_window: capability
                        .and_then(|capability| capability.context_window)
                        .and_then(|value| u32::try_from(value).ok()),
                    max_output_tokens: None,
                    supports_tools: capability
                        .map(|capability| capability.supports_parallel_tool_calls),
                    supports_vision: capability.map(|capability| {
                        capability
                            .input_modalities
                            .iter()
                            .any(|modality| modality == "image")
                    }),
                    updated_at: None,
                });
            }
        }
    }

    models
}

pub(crate) fn provider_for_model(model_id: &str) -> Option<(String, bool)> {
    let settings = load_global_config().ok()?;
    let profiles = settings.auth_profiles?;
    for (id, profile) in profiles {
        let matches = profile.model.as_deref() == Some(model_id)
            || profile
                .available_models
                .as_ref()
                .is_some_and(|models| models.iter().any(|model| model == model_id));
        if matches {
            return Some((id, profile_enabled(&profile)));
        }
    }
    None
}

pub(crate) fn update_configured_model(
    model_id: &str,
    alias: Option<String>,
    context_window: Option<u32>,
    visible: Option<bool>,
) -> anyhow::Result<()> {
    let mut settings = load_global_config().unwrap_or_default();
    let Some(profiles) = settings.auth_profiles.as_mut() else {
        return Ok(());
    };

    for profile in profiles.values_mut() {
        let matches = profile.model.as_deref() == Some(model_id)
            || profile
                .available_models
                .as_ref()
                .is_some_and(|models| models.iter().any(|model| model == model_id));
        if !matches {
            continue;
        }
        if let Some(visible) = visible {
            profile.extra.insert("enabled".to_string(), json!(visible));
        }
        let capabilities = profile.model_capabilities.get_or_insert_with(HashMap::new);
        let entry = capabilities
            .entry(model_id.to_string())
            .or_insert_with(ModelCapabilitySettings::default);
        if let Some(alias) = alias.clone() {
            entry.description = normalized_non_empty(Some(alias.as_str()));
        }
        if let Some(context_window) = context_window {
            entry.context_window = Some(u64::from(context_window));
        }
    }

    write_user_settings(&settings)?;
    Ok(())
}

fn provider_presets() -> Vec<ProviderPreset> {
    let mut presets: Vec<ProviderPreset> = allthecodes_api::api::providers::PROVIDERS
        .iter()
        .map(|provider| {
            let capabilities =
                allthecodes_api::api::providers::capabilities_for_provider_info(provider);
            ProviderPreset {
                id: provider.name.to_string(),
                name: provider.label.to_string(),
                kind: provider.name.to_string(),
                auth_kind: auth_kind_for_provider(provider.name).to_string(),
                base_url: Some(provider.base_url.to_string()),
                description: Some(format!(
                    "{} protocol, {}",
                    protocol_label(provider.protocol),
                    provider.env_key
                )),
                default_model: Some(provider.default_model.to_string()),
                protocol: Some(protocol_label(provider.protocol).to_string()),
                supported: capabilities.is_usable(),
            }
        })
        .collect();

    for id in ["bedrock", "vertex", "azure-foundry"] {
        if let Some(capabilities) =
            allthecodes_api::api::providers::capabilities_for_provider_name(id)
        {
            presets.push(ProviderPreset {
                id: capabilities.name.to_string(),
                name: match capabilities.name {
                    "bedrock" => "Amazon Bedrock".to_string(),
                    "vertex" => "Google Vertex AI".to_string(),
                    "azure-foundry" => "Microsoft Foundry".to_string(),
                    other => other.to_string(),
                },
                kind: capabilities.name.to_string(),
                auth_kind: "api_key".to_string(),
                base_url: None,
                description: capabilities.status.reason().map(str::to_string),
                default_model: None,
                protocol: Some(format!("{:?}", capabilities.protocol).to_ascii_lowercase()),
                supported: capabilities.is_usable(),
            });
        }
    }

    presets
}

fn provider_kind_from_request(kind: &str, preset_id: Option<&str>) -> String {
    if let Some(preset_id) = normalized_non_empty(preset_id) {
        return preset_id;
    }
    match kind {
        "custom_acp" | "acp" => "custom_acp".to_string(),
        "openai_compatible" => "openai".to_string(),
        other => other.to_string(),
    }
}

fn profile_enabled(profile: &ProviderProfileSettings) -> bool {
    profile
        .extra
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn profile_auth_kind(profile: &ProviderProfileSettings) -> String {
    if profile.extra.get("providerType").and_then(Value::as_str) == Some("acp") {
        return "none".to_string();
    }
    if profile
        .api_key
        .as_ref()
        .is_some_and(|value| !value.is_empty())
    {
        "api_key".to_string()
    } else if profile.env.as_ref().is_some_and(|env| !env.is_empty()) {
        "api_key".to_string()
    } else {
        auth_kind_for_provider(profile.api_provider.as_deref().unwrap_or("")).to_string()
    }
}

fn auth_kind_for_provider(provider: &str) -> &'static str {
    match provider {
        "openai-codex" | "anthropic" => "oauth",
        _ => "api_key",
    }
}

fn profile_credential_status(profile: &ProviderProfileSettings) -> String {
    if profile.extra.get("providerType").and_then(Value::as_str) == Some("acp") {
        return "configured".to_string();
    }
    if profile
        .api_key
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        || profile.env.as_ref().is_some_and(|env| !env.is_empty())
    {
        "configured".to_string()
    } else {
        "missing".to_string()
    }
}

fn provider_diagnostics(profile: &ProviderProfileSettings) -> Option<Vec<String>> {
    let mut diagnostics = Vec::new();
    if !profile_enabled(profile) {
        diagnostics.push("Provider is disabled".to_string());
    }
    if profile.extra.get("providerType").and_then(Value::as_str) == Some("acp")
        && profile
            .extra
            .get("command")
            .and_then(Value::as_str)
            .is_none()
    {
        diagnostics.push("ACP command is not configured".to_string());
    }
    if diagnostics.is_empty() {
        None
    } else {
        Some(diagnostics)
    }
}

fn normalize_models(models: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = models
        .into_iter()
        .filter_map(|model| normalized_non_empty(Some(model.as_str())))
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn normalized_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn protocol_label(protocol: allthecodes_api::api::providers::ProviderProtocol) -> &'static str {
    match protocol {
        allthecodes_api::api::providers::ProviderProtocol::Anthropic => "anthropic",
        allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat => "openai_compat",
        allthecodes_api::api::providers::ProviderProtocol::Google => "google",
    }
}
