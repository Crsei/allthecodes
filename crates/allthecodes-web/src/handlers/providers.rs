//! Provider CRUD handlers — list, create, update, delete, refresh models.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use allthecodes_config::settings::{
    codex_model_capabilities, codex_model_ids, load_global_config, user_settings_path,
    write_user_settings, ModelCapabilitySettings, ProviderProfileSettings, RawSettings,
    API_PROVIDER_OPENAI_CODEX, AUTH_PROFILE_CODEX,
};

use crate::handlers::models::{ModelSummary, ModelUpdateRequest};
use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<Value>,
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
    #[serde(default)]
    pub provider_options: Option<Value>,
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
    #[serde(default)]
    pub provider_options: Option<Value>,
}

#[derive(Serialize)]
pub struct ModelDiscoveryResponse {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexLocalStatusResponse {
    pub cli_installed: bool,
    pub auth_present: bool,
    pub auth_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub settings_path: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexApplyLocalResponse {
    pub cli_installed: bool,
    pub auth_present: bool,
    pub auth_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub settings_path: String,
    pub active: bool,
    pub model: String,
    pub message: String,
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
                command: profile_command(&profile),
                arguments: profile_arguments(&profile),
                provider_options: profile_provider_options(&profile),
            });
        }
    }

    providers
}

/// GET /api/providers/openai-codex/local-status — Inspect local Codex CLI auth.
pub async fn codex_local_status_handler() -> impl IntoResponse {
    Json(codex_local_status())
}

/// POST /api/providers/openai-codex/apply-local — Use local Codex CLI OAuth.
pub async fn codex_apply_local_handler(State(state): State<WebState>) -> Response {
    let before = codex_local_status();
    if !before.cli_installed {
        return codex_apply_bad_request(
            before,
            "Codex CLI was not found on PATH. Install Codex CLI, run `codex login`, then refresh.",
        );
    }
    match before.auth_status.as_str() {
        "valid" | "expired_refreshable" => {}
        "missing" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI auth.json was not found. Run `codex login`, then refresh.",
            );
        }
        "expired" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI credentials are expired and have no refresh token. Run `codex login` again.",
            );
        }
        "invalid" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI auth.json could not be read. Run `codex login` again.",
            );
        }
        _ => {
            return codex_apply_bad_request(before, "Codex CLI OAuth is not available.");
        }
    }

    if let Err(error) = allthecodes_auth::try_resolve_codex_cli_auth_token()
        .and_then(|token| token.ok_or_else(|| anyhow::anyhow!("Codex CLI OAuth token not found")))
        .map(drop)
    {
        return codex_apply_bad_request(before, &format!("Codex CLI OAuth is not usable: {error}"));
    }

    let default_model = default_codex_model();
    let mut settings = load_global_config().unwrap_or_default();
    apply_codex_profile_to_settings(&mut settings, &default_model);

    match write_user_settings(&settings) {
        Ok(settings_path) => {
            apply_codex_profile_to_app_state(&state, &settings);
            Json(CodexApplyLocalResponse {
                cli_installed: before.cli_installed,
                auth_present: true,
                auth_status: "valid".to_string(),
                expires_at: before.expires_at,
                settings_path: settings_path.display().to_string(),
                active: true,
                model: default_model,
                message: "Codex profile applied from local Codex CLI OAuth.".to_string(),
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
                    max_output_tokens: capability
                        .and_then(|capability| capability.max_output_tokens)
                        .and_then(|value| u32::try_from(value).ok()),
                    supports_tools: capability
                        .map(|capability| capability.supports_parallel_tool_calls),
                    supports_vision: capability.map(|capability| {
                        capability
                            .input_modalities
                            .iter()
                            .any(|modality| modality == "image")
                    }),
                    supports_reasoning: capability.map(capability_supports_reasoning),
                    supports_image_output: capability
                        .and_then(|capability| capability.supports_image_output),
                    supports_embedding: capability
                        .and_then(|capability| capability.supports_embedding),
                    provider_options: capability.and_then(|capability| {
                        capability
                            .provider_options
                            .clone()
                            .and_then(normalized_json_object)
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
    req: ModelUpdateRequest,
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
        if let Some(visible) = req.visible {
            profile.extra.insert("enabled".to_string(), json!(visible));
        }
        let capabilities = profile.model_capabilities.get_or_insert_with(HashMap::new);
        let entry = capabilities
            .entry(model_id.to_string())
            .or_insert_with(ModelCapabilitySettings::default);
        if let Some(alias) = req.alias.clone() {
            entry.description = normalized_non_empty(Some(alias.as_str()));
        }
        if let Some(context_window) = req.context_window {
            entry.context_window = Some(u64::from(context_window));
        }
        if let Some(max_output_tokens) = req.max_output_tokens {
            entry.max_output_tokens = Some(u64::from(max_output_tokens));
        }
        if let Some(supports_tools) = req.supports_tools {
            entry.supports_parallel_tool_calls = supports_tools;
        }
        if let Some(supports_vision) = req.supports_vision {
            set_input_modality(&mut entry.input_modalities, "image", supports_vision);
        }
        if let Some(supports_reasoning) = req.supports_reasoning {
            entry.supports_reasoning = Some(supports_reasoning);
            entry.supports_reasoning_summaries = supports_reasoning;
            if supports_reasoning {
                if entry.supported_reasoning_levels.is_empty() {
                    entry.supported_reasoning_levels =
                        vec!["low".to_string(), "medium".to_string(), "high".to_string()];
                }
                if entry.default_reasoning_level.is_none() {
                    entry.default_reasoning_level = Some("medium".to_string());
                }
            } else {
                entry.supported_reasoning_levels.clear();
                entry.default_reasoning_level = None;
            }
        }
        if let Some(supports_image_output) = req.supports_image_output {
            entry.supports_image_output = Some(supports_image_output);
        }
        if let Some(supports_embedding) = req.supports_embedding {
            entry.supports_embedding = Some(supports_embedding);
        }
        if let Some(provider_options) = req
            .provider_options
            .clone()
            .and_then(normalized_json_object)
        {
            entry.provider_options = Some(provider_options);
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

fn profile_command(profile: &ProviderProfileSettings) -> Option<String> {
    profile
        .extra
        .get("command")
        .and_then(Value::as_str)
        .and_then(|value| normalized_non_empty(Some(value)))
}

fn profile_arguments(profile: &ProviderProfileSettings) -> Option<Vec<String>> {
    let arguments = profile
        .extra
        .get("arguments")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|value| normalized_non_empty(Some(value)))
        .collect::<Vec<_>>();
    if arguments.is_empty() {
        None
    } else {
        Some(arguments)
    }
}

fn profile_provider_options(profile: &ProviderProfileSettings) -> Option<Value> {
    profile
        .extra
        .get("providerOptions")
        .cloned()
        .and_then(normalized_json_object)
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
        .api_provider
        .as_deref()
        .is_some_and(|provider| provider == API_PROVIDER_OPENAI_CODEX)
    {
        return match codex_local_auth_status().0.as_str() {
            "valid" | "expired_refreshable" => "configured".to_string(),
            _ => "missing".to_string(),
        };
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

fn normalized_json_object(value: Value) -> Option<Value> {
    match value {
        Value::Object(map) => Some(Value::Object(map)),
        _ => None,
    }
}

fn codex_apply_bad_request(status: CodexLocalStatusResponse, message: &str) -> Response {
    let message = format!("{} {}", message, status.message);
    (
        StatusCode::BAD_REQUEST,
        Json(
            ProtocolApiError::BadRequest {
                code: "codex_local_oauth_unavailable",
                message,
            }
            .into_body(),
        ),
    )
        .into_response()
}

fn codex_local_status() -> CodexLocalStatusResponse {
    let cli_installed = is_command_on_path("codex");
    let (auth_status, expires_at) = codex_local_auth_status();
    let auth_present = auth_status != "missing";
    let settings = load_global_config().unwrap_or_default();
    let (active, model) = codex_settings_state(&settings);
    let message = codex_status_message(cli_installed, auth_present, &auth_status, active);

    CodexLocalStatusResponse {
        cli_installed,
        auth_present,
        auth_status,
        expires_at,
        settings_path: user_settings_path().display().to_string(),
        active,
        model,
        message,
    }
}

fn codex_local_auth_status() -> (String, Option<i64>) {
    match allthecodes_auth::codex_cli::read_codex_cli_credential() {
        Ok(Some(credential)) => {
            let expired = allthecodes_auth::codex_cli::is_credential_expired(&credential);
            let status = if expired {
                if credential
                    .refresh_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                {
                    "expired_refreshable"
                } else {
                    "expired"
                }
            } else {
                "valid"
            };
            (status.to_string(), credential.expires_at)
        }
        Ok(None) => {
            if allthecodes_auth::codex_cli::codex_cli_auth_path().is_some() {
                ("invalid".to_string(), None)
            } else {
                ("missing".to_string(), None)
            }
        }
        Err(_) => ("invalid".to_string(), None),
    }
}

fn codex_settings_state(settings: &RawSettings) -> (bool, Option<String>) {
    let codex_profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(AUTH_PROFILE_CODEX));
    let profile_is_codex = codex_profile.is_some_and(|profile| {
        profile.backend.as_deref() == Some("codex")
            || profile.api_provider.as_deref() == Some(API_PROVIDER_OPENAI_CODEX)
    });
    let top_level_is_codex = settings.api_provider.as_deref() == Some(API_PROVIDER_OPENAI_CODEX)
        || settings.backend.as_deref() == Some("codex");
    let active = settings.active_auth_profile.as_deref() == Some(AUTH_PROFILE_CODEX)
        && (profile_is_codex || top_level_is_codex);
    let model = codex_profile
        .and_then(|profile| profile.model.clone())
        .or_else(|| settings.model.clone());
    (active, model)
}

fn codex_status_message(
    cli_installed: bool,
    auth_present: bool,
    auth_status: &str,
    active: bool,
) -> String {
    if !cli_installed {
        return "Codex CLI was not found on PATH.".to_string();
    }
    if !auth_present {
        return "Codex CLI auth.json was not found. Run `codex login`.".to_string();
    }
    match auth_status {
        "valid" if active => "Local Codex OAuth is active in allthecodes settings.".to_string(),
        "valid" => "Local Codex OAuth is available.".to_string(),
        "expired_refreshable" => {
            "Local Codex OAuth is expired but has a refresh token.".to_string()
        }
        "expired" => "Local Codex OAuth is expired and has no refresh token.".to_string(),
        "invalid" => "Codex CLI auth.json could not be read.".to_string(),
        _ => "Codex CLI OAuth is not available.".to_string(),
    }
}

fn apply_codex_profile_to_settings(settings: &mut RawSettings, default_model: &str) {
    let models = codex_model_ids();
    let profile = ProviderProfileSettings {
        backend: Some("codex".to_string()),
        api_provider: Some(API_PROVIDER_OPENAI_CODEX.to_string()),
        model: Some(default_model.to_string()),
        available_models: Some(models.clone()),
        model_capabilities: Some(codex_model_capabilities()),
        model_reasoning_effort: Some("medium".to_string()),
        base_url: None,
        api_key: None,
        env: None,
        auth_source: Some(json!({ "type": "codex_cli" })),
        extra: HashMap::from([("enabled".to_string(), json!(true))]),
    };

    let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
    profiles.insert(AUTH_PROFILE_CODEX.to_string(), profile);
    settings.active_auth_profile = Some(AUTH_PROFILE_CODEX.to_string());
    settings.api_provider = Some(API_PROVIDER_OPENAI_CODEX.to_string());
    settings.backend = Some("codex".to_string());
    settings.model = Some(default_model.to_string());
    settings.available_models = Some(models);
    settings.model_reasoning_effort = Some("medium".to_string());
}

fn apply_codex_profile_to_app_state(state: &WebState, settings: &RawSettings) {
    let profiles = settings.auth_profiles.clone().unwrap_or_default();
    let model = settings.model.clone().unwrap_or_else(default_codex_model);
    let available_models = settings
        .available_models
        .clone()
        .unwrap_or_else(codex_model_ids);
    let model_capabilities = profiles
        .get(AUTH_PROFILE_CODEX)
        .and_then(|profile| profile.model_capabilities.clone())
        .unwrap_or_else(codex_model_capabilities);

    state.engine().update_app_state(|app_state| {
        app_state.main_loop_model = model.clone();
        app_state.main_loop_backend = "codex".to_string();
        app_state.settings.model = Some(model.clone());
        app_state.settings.backend = Some("codex".to_string());
        app_state.settings.api_provider = Some(API_PROVIDER_OPENAI_CODEX.to_string());
        app_state.settings.active_auth_profile = Some(AUTH_PROFILE_CODEX.to_string());
        app_state.settings.auth_profiles = profiles;
        app_state.settings.available_models = available_models;
        app_state.settings.model_capabilities = model_capabilities;
        app_state.settings.model_reasoning_effort = settings.model_reasoning_effort.clone();
    });
}

fn default_codex_model() -> String {
    codex_model_ids()
        .into_iter()
        .next()
        .unwrap_or_else(|| "gpt-5.5".to_string())
}

fn is_command_on_path(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return is_executable_file(Path::new(command));
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths)
        .any(|dir| command_candidates(&dir, command).any(|path| is_executable_file(&path)))
}

fn command_candidates<'a>(dir: &'a Path, command: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    #[cfg(windows)]
    {
        let extensions = std::env::var_os("PATHEXT")
            .map(|value| {
                std::env::split_paths(&value)
                    .filter_map(|path| path.into_os_string().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]);
        let mut candidates = vec![dir.join(command)];
        candidates.extend(
            extensions
                .into_iter()
                .map(move |ext| dir.join(format!("{command}{ext}"))),
        );
        candidates.into_iter()
    }
    #[cfg(not(windows))]
    {
        std::iter::once(dir.join(command))
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn normalized_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn capability_supports_reasoning(capability: &ModelCapabilitySettings) -> bool {
    capability.supports_reasoning.unwrap_or_else(|| {
        capability.default_reasoning_level.is_some()
            || !capability.supported_reasoning_levels.is_empty()
            || capability.supports_reasoning_summaries
    })
}

fn set_input_modality(modalities: &mut Vec<String>, modality: &str, enabled: bool) {
    if enabled {
        if !modalities.iter().any(|item| item == modality) {
            modalities.push(modality.to_string());
        }
    } else {
        modalities.retain(|item| item != modality);
    }
}

fn protocol_label(protocol: allthecodes_api::api::providers::ProviderProtocol) -> &'static str {
    match protocol {
        allthecodes_api::api::providers::ProviderProtocol::Anthropic => "anthropic",
        allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat => "openai_compat",
        allthecodes_api::api::providers::ProviderProtocol::Google => "google",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::{
        make_web_state, read_user_settings, response_json, temp_home, EnvGuard,
    };
    use axum::response::IntoResponse;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serial_test::serial;

    fn jwt_with_exp(exp: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#).as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(b"sig");
        format!("{header}.{payload}.{signature}")
    }

    fn write_codex_auth(codex_home: &Path, access_token: &str, refresh_token: Option<&str>) {
        let refresh = refresh_token
            .map(|token| format!(r#","refresh_token":"{token}""#))
            .unwrap_or_default();
        std::fs::create_dir_all(codex_home).expect("codex home");
        std::fs::write(
            codex_home.join("auth.json"),
            format!(
                r#"{{
                    "auth_mode": "chatgpt",
                    "tokens": {{
                        "access_token": "{access_token}"{refresh}
                    }}
                }}"#
            ),
        )
        .expect("auth json");
    }

    fn create_path_codex(bin_dir: &Path) {
        std::fs::create_dir_all(bin_dir).expect("bin dir");
        std::fs::write(bin_dir.join("codex"), "#!/bin/sh\n").expect("codex shim");
    }

    #[tokio::test]
    #[serial]
    async fn codex_local_status_missing_without_cli_or_auth() {
        let (_home, _home_guard) = temp_home();
        let codex_home = tempfile::tempdir().expect("codex home");
        let path_dir = tempfile::tempdir().expect("path dir");
        let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
        let _path_guard = EnvGuard::set_path("PATH", path_dir.path());

        let status = codex_local_status();

        assert!(!status.cli_installed);
        assert!(!status.auth_present);
        assert_eq!(status.auth_status, "missing");

        let response = codex_apply_local_handler(State(make_web_state()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("codex_local_oauth_unavailable"));
    }

    #[tokio::test]
    #[serial]
    async fn codex_apply_local_writes_codex_profile_without_tokens() {
        let (home, _home_guard) = temp_home();
        let codex_home = tempfile::tempdir().expect("codex home");
        let path_dir = tempfile::tempdir().expect("path dir");
        create_path_codex(path_dir.path());
        let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
        let _path_guard = EnvGuard::set_path("PATH", path_dir.path());
        write_codex_auth(
            codex_home.path(),
            &jwt_with_exp(Utc::now().timestamp() + 3600),
            Some("refresh-token"),
        );
        let state = make_web_state();

        let response = codex_apply_local_handler(State(state.clone()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["auth_status"], json!("valid"));
        assert_eq!(body["active"], json!(true));

        let settings = read_user_settings(&home);
        assert_eq!(
            settings.api_provider.as_deref(),
            Some(API_PROVIDER_OPENAI_CODEX)
        );
        assert_eq!(
            settings.active_auth_profile.as_deref(),
            Some(AUTH_PROFILE_CODEX)
        );
        assert_eq!(settings.backend.as_deref(), Some("codex"));
        let profile = settings
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(AUTH_PROFILE_CODEX))
            .expect("codex profile");
        assert_eq!(
            profile.api_provider.as_deref(),
            Some(API_PROVIDER_OPENAI_CODEX)
        );
        assert_eq!(profile.backend.as_deref(), Some("codex"));
        assert!(profile.api_key.is_none());
        assert!(profile.env.is_none());
        assert_eq!(
            profile.auth_source.as_ref(),
            Some(&json!({ "type": "codex_cli" }))
        );

        let app_state = state.engine().app_state();
        assert_eq!(app_state.main_loop_backend, "codex");
        assert_eq!(
            app_state.settings.active_auth_profile.as_deref(),
            Some("codex")
        );
        assert_eq!(
            app_state.settings.api_provider.as_deref(),
            Some(API_PROVIDER_OPENAI_CODEX)
        );
    }

    #[tokio::test]
    #[serial]
    async fn codex_apply_local_rejects_expired_unrefreshable_auth_without_changing_settings() {
        let (home, _home_guard) = temp_home();
        let codex_home = tempfile::tempdir().expect("codex home");
        let path_dir = tempfile::tempdir().expect("path dir");
        create_path_codex(path_dir.path());
        let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
        let _path_guard = EnvGuard::set_path("PATH", path_dir.path());
        write_codex_auth(
            codex_home.path(),
            &jwt_with_exp(Utc::now().timestamp() - 3600),
            None,
        );

        let response = codex_apply_local_handler(State(make_web_state()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            !home.path().join("settings.json").exists(),
            "apply failure must not create settings"
        );
    }
}
