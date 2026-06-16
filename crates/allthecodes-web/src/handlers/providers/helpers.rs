use serde_json::Value;

use allthecodes_config::settings::{
    load_global_config, ModelCapabilitySettings, ProviderProfileSettings, API_PROVIDER_OPENAI_CODEX,
};

use super::codex::codex_local_auth_status;
use super::types::{ProviderPreset, ProviderSummary};

/// Build a list of providers from settings auth profiles.
pub(super) fn provider_summaries_from_settings() -> Vec<ProviderSummary> {
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

pub(super) fn provider_presets() -> Vec<ProviderPreset> {
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

pub(super) fn provider_kind_from_request(kind: &str, preset_id: Option<&str>) -> String {
    if let Some(preset_id) = normalized_non_empty(preset_id) {
        return preset_id;
    }
    match kind {
        "custom_acp" | "acp" => "custom_acp".to_string(),
        "openai_compatible" => "openai".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn profile_command(profile: &ProviderProfileSettings) -> Option<String> {
    profile
        .extra
        .get("command")
        .and_then(Value::as_str)
        .and_then(|value| normalized_non_empty(Some(value)))
}

pub(super) fn profile_arguments(profile: &ProviderProfileSettings) -> Option<Vec<String>> {
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

pub(super) fn profile_provider_options(profile: &ProviderProfileSettings) -> Option<Value> {
    profile
        .extra
        .get("providerOptions")
        .cloned()
        .and_then(normalized_json_object)
}

pub(super) fn profile_enabled(profile: &ProviderProfileSettings) -> bool {
    profile
        .extra
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn profile_auth_kind(profile: &ProviderProfileSettings) -> String {
    if profile.extra.get("providerType").and_then(Value::as_str) == Some("acp") {
        return "none".to_string();
    }
    if profile
        .api_key
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        || profile.env.as_ref().is_some_and(|env| !env.is_empty())
    {
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

pub(super) fn profile_credential_status(profile: &ProviderProfileSettings) -> String {
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

pub(super) fn provider_diagnostics(profile: &ProviderProfileSettings) -> Option<Vec<String>> {
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

pub(super) fn normalize_models(models: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = models
        .into_iter()
        .filter_map(|model| normalized_non_empty(Some(model.as_str())))
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(super) fn normalized_json_object(value: Value) -> Option<Value> {
    match value {
        Value::Object(map) => Some(Value::Object(map)),
        _ => None,
    }
}

fn protocol_label(protocol: allthecodes_api::api::providers::ProviderProtocol) -> &'static str {
    match protocol {
        allthecodes_api::api::providers::ProviderProtocol::Anthropic => "anthropic",
        allthecodes_api::api::providers::ProviderProtocol::OpenAiCompat => "openai_compat",
        allthecodes_api::api::providers::ProviderProtocol::Google => "google",
    }
}

pub(super) fn normalized_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn capability_supports_reasoning(capability: &ModelCapabilitySettings) -> bool {
    capability.supports_reasoning.unwrap_or_else(|| {
        capability.default_reasoning_level.is_some()
            || !capability.supported_reasoning_levels.is_empty()
            || capability.supports_reasoning_summaries
    })
}

pub(super) fn set_input_modality(modalities: &mut Vec<String>, modality: &str, enabled: bool) {
    if enabled {
        if !modalities.iter().any(|item| item == modality) {
            modalities.push(modality.to_string());
        }
    } else {
        modalities.retain(|item| item != modality);
    }
}
