use std::collections::HashMap;

use serde_json::json;

use allthecodes_config::settings::{
    load_global_config, write_user_settings, ModelCapabilitySettings,
};

use crate::handlers::models::{ModelSummary, ModelUpdateRequest};

use super::helpers::{
    capability_supports_reasoning, normalized_json_object, normalized_non_empty, profile_enabled,
    provider_presets, set_input_modality,
};

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
