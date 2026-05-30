use serde_json::Value;
use tracing::warn;

pub(crate) fn resolve_startup_model(
    requested: Option<&str>,
    provider_default: Option<&str>,
    hardcoded_default: &str,
    available: &[String],
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> String {
    for candidate in [requested, provider_default, Some(hardcoded_default)] {
        let Some(candidate) = candidate else { continue };
        if allthecodes_commands::model::is_removed_legacy_model_alias(candidate) {
            warn!(
                model = %candidate,
                replacement = ?allthecodes_models::replacement_for_removed_legacy_alias(candidate),
                "legacy model alias ignored during startup"
            );
            continue;
        }
        let model = resolve_model_alias_for_effective_settings(candidate, settings);
        if check_startup_available(&model, available, settings).is_ok() {
            return model;
        }
    }

    if let Some(first_allowed) = available
        .iter()
        .find_map(|entry| resolve_startup_model_list_entry(entry, settings))
    {
        warn!(
            fallback = %first_allowed,
            "no requested/default model satisfied availableModels; falling back to the first allowed entry"
        );
        return first_allowed;
    }

    if !available.is_empty() {
        warn!(
            "availableModels contained no usable model entries; using the hardcoded default model"
        );
    }
    resolve_model_alias_for_effective_settings(hardcoded_default, settings)
}

pub(crate) fn resolve_startup_model_list_entry(
    entry: &str,
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Option<String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() || allthecodes_commands::model::is_removed_legacy_model_alias(trimmed) {
        None
    } else {
        Some(resolve_model_alias_for_effective_settings(
            trimmed, settings,
        ))
    }
}

pub(crate) fn resolve_model_alias_for_effective_settings(
    name: &str,
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> String {
    let trimmed = name.trim();
    match trimmed.to_ascii_uppercase().as_str() {
        "SOTA" => settings
            .sota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        "MOTA" => settings
            .mota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        "FOTA" => settings
            .fota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        _ => allthecodes_commands::model::resolve_model_alias(trimmed),
    }
}

pub(crate) fn settings_thinking_enabled(
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Option<bool> {
    let thinking = settings.thinking.as_ref()?;
    if let Some(enabled) = thinking.as_bool() {
        return Some(enabled);
    }
    let kind = thinking
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| thinking.as_str())?
        .trim()
        .to_ascii_lowercase();
    match kind.as_str() {
        "enabled" | "adaptive" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

pub(crate) fn output_config_effort(output_config: Option<&Value>) -> Option<String> {
    output_config?
        .get("effort")
        .and_then(allthecodes_engine::effort::normalize_output_effort_json)
}

pub(crate) fn settings_effort_value(
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Option<String> {
    output_config_effort(settings.output_config.as_ref()).or_else(|| settings.effort_level.clone())
}

pub(crate) fn check_startup_available(
    model: &str,
    available: &[String],
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Result<(), String> {
    if available.is_empty() {
        return Ok(());
    }
    if available.iter().any(|entry| {
        resolve_startup_model_list_entry(entry, settings)
            .as_deref()
            .is_some_and(|allowed| allowed == model)
    }) {
        return Ok(());
    }
    Err(format!("Model '{model}' is not in availableModels."))
}
