//! Model alias resolution and request preparation.

use anyhow::{bail, Result};
use serde_json::Value;

use super::types::{
    ApiProvider, MessagesRequest,
    ANTHROPIC_DEFAULT_FOTA_MODEL_ENV, ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
    ANTHROPIC_DEFAULT_MODEL_ALIAS, ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_OPUS_MODEL_ENV, ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
    ANTHROPIC_DEFAULT_SOTA_MODEL_ENV, ANTHROPIC_LEGACY_MODEL_ENV_WARNING,
    ANTHROPIC_OFFICIAL_FOTA_MODEL, ANTHROPIC_OFFICIAL_MOTA_MODEL,
    ANTHROPIC_OFFICIAL_SOTA_MODEL,
};
use crate::api::providers::AnthropicEndpointKind;

// ---------------------------------------------------------------------------
// Token counting body
// ---------------------------------------------------------------------------

/// Build the JSON body for a `count_tokens` request by stripping
/// stream/model-specific fields from the original request.
pub(crate) fn build_anthropic_count_tokens_body(request: &MessagesRequest) -> Value {
    let mut body = serde_json::to_value(request).unwrap_or_else(|_| serde_json::json!({}));
    if let Value::Object(map) = &mut body {
        map.remove("stream");
        map.remove("max_tokens");
        map.remove("advisor_model");
    }
    body
}

// ---------------------------------------------------------------------------
// Advisor support check
// ---------------------------------------------------------------------------

/// Return `true` when the given provider supports the advisor-model field.
///
/// Only the Anthropic Messages API currently recognizes `advisor_model`.
/// For Bedrock/Vertex (which ultimately reach the same Anthropic shape) we
/// also pass it through; OpenAI-compatible and Google providers don't have
/// the field in their native schema, so we drop it there and the `/advisor`
/// command surfaces an "inactive" message.
pub fn provider_supports_advisor(provider: &ApiProvider) -> bool {
    matches!(
        provider,
        ApiProvider::Anthropic {
            endpoint_kind: AnthropicEndpointKind::DirectAnthropic,
            ..
        } | ApiProvider::Azure { .. }
            | ApiProvider::Bedrock { .. }
            | ApiProvider::Vertex { .. }
    )
}

// ---------------------------------------------------------------------------
// Env helpers
// ---------------------------------------------------------------------------

/// Read `ANTHROPIC_BASE_URL` from the environment.
pub(super) fn anthropic_base_url_from_env() -> Option<String> {
    std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Read the selected API provider from the settings file.
pub(super) fn selected_api_provider_from_settings() -> Result<Option<String>> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let loaded = allthecodes_config::settings::load_effective(&cwd)?;
    let Some(raw) = loaded.effective.api_provider else {
        return Ok(None);
    };
    let Some(provider) = allthecodes_config::settings::normalize_api_provider(&raw) else {
        bail!(
            "Unknown apiProvider `{}`. Known providers: {}.",
            raw,
            allthecodes_config::settings::VALID_API_PROVIDERS.join(", ")
        );
    };
    Ok(Some(provider.to_string()))
}

// ---------------------------------------------------------------------------
// Anthropic model alias logic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct AnthropicModelAlias {
    name: &'static str,
    default_env: &'static str,
    legacy_env: &'static str,
    official_default: &'static str,
}

impl AnthropicModelAlias {
    fn for_name(model: &str) -> Option<Self> {
        let trimmed = model.trim();
        if trimmed.eq_ignore_ascii_case("SOTA") {
            Some(Self {
                name: "SOTA",
                default_env: ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_SOTA_MODEL,
            })
        } else if trimmed.eq_ignore_ascii_case("MOTA") {
            Some(Self {
                name: "MOTA",
                default_env: ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_MOTA_MODEL,
            })
        } else if trimmed.eq_ignore_ascii_case("FOTA") {
            Some(Self {
                name: "FOTA",
                default_env: ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
                legacy_env: ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
                official_default: ANTHROPIC_OFFICIAL_FOTA_MODEL,
            })
        } else {
            None
        }
    }
}

fn warn_legacy_anthropic_model_env_once(legacy_env: &str, default_env: &str) {
    ANTHROPIC_LEGACY_MODEL_ENV_WARNING.call_once(|| {
        tracing::warn!(
            legacy_env,
            default_env,
            "legacy Anthropic model fallback env var is deprecated; use the SOTA/MOTA/FOTA env var instead"
        );
    });
}

fn anthropic_model_env_for_alias(alias: AnthropicModelAlias) -> Option<String> {
    if let Some(model) = non_empty_env(alias.default_env) {
        return Some(model);
    }
    let model = non_empty_env(alias.legacy_env)?;
    warn_legacy_anthropic_model_env_once(alias.legacy_env, alias.default_env);
    Some(model)
}

/// Resolve a model alias (SOTA/MOTA/FOTA) to a concrete model ID using
/// environment overrides or the official default.
pub(super) fn resolve_anthropic_model_alias(
    model: &str,
    endpoint_kind: AnthropicEndpointKind,
) -> Result<String> {
    let trimmed = model.trim();
    let Some(alias) = AnthropicModelAlias::for_name(trimmed) else {
        return Ok(trimmed.to_string());
    };

    if let Some(model) = anthropic_model_env_for_alias(alias) {
        return Ok(model);
    }

    match endpoint_kind {
        AnthropicEndpointKind::DirectAnthropic => Ok(alias.official_default.to_string()),
        AnthropicEndpointKind::CompatibleAnthropic => bail!(
            "Anthropic-compatible provider cannot resolve model alias `{}` without {}. Set ANTHROPIC_MODEL to an explicit provider model ID, or set {} for this compatible endpoint.",
            alias.name,
            alias.default_env,
            alias.default_env
        ),
    }
}

/// Resolve the default Anthropic model using `ANTHROPIC_MODEL` env var or
/// the built-in alias.
pub(super) fn resolve_anthropic_default_model(
    endpoint_kind: AnthropicEndpointKind,
) -> Result<String> {
    let selected = non_empty_env("ANTHROPIC_MODEL")
        .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL_ALIAS.to_string());
    resolve_anthropic_model_alias(&selected, endpoint_kind)
}

fn resolve_model_for_request(provider: &ApiProvider, model: &str) -> Result<String> {
    match provider {
        ApiProvider::Anthropic { endpoint_kind, .. } => {
            resolve_anthropic_model_alias(model, *endpoint_kind)
        }
        ApiProvider::Bedrock { .. } | ApiProvider::Vertex { .. } => {
            resolve_anthropic_model_alias(model, AnthropicEndpointKind::DirectAnthropic)
        }
        _ => Ok(model.trim().to_string()),
    }
}

/// Resolve model aliases in a request for the given provider.
pub(super) fn resolve_request_model_for_provider(
    provider: &ApiProvider,
    mut request: MessagesRequest,
) -> Result<MessagesRequest> {
    request.model = resolve_model_for_request(provider, &request.model)?;
    Ok(request)
}
