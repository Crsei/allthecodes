use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::raw::RawSettings;

pub const API_PROVIDER_ANTHROPIC: &str = "anthropic";
pub const API_PROVIDER_OPENAI_CODEX: &str = "openai-codex";
pub const API_PROVIDER_OPENAI: &str = "openai";
pub const AUTH_PROFILE_CLAUDE_CODE: &str = "claude_code";
pub const AUTH_PROFILE_ANTHROPIC_LEGACY: &str = "anthropic";
pub const AUTH_PROFILE_CODEX: &str = "codex";
pub const AUTH_PROFILE_OPENAI: &str = "openai";
pub const AUTH_PROFILE_CUSTOM: &str = "custom";
pub const VALID_API_PROVIDERS: &[&str] = &[
    API_PROVIDER_ANTHROPIC,
    API_PROVIDER_OPENAI_CODEX,
    API_PROVIDER_OPENAI,
];

pub fn normalize_api_provider(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "anthropic" | "anthropic-method" | "anthropic_method" => Some(API_PROVIDER_ANTHROPIC),
        "openai-codex" | "openai_codex" | "codex" => Some(API_PROVIDER_OPENAI_CODEX),
        "openai" | "openai-api" | "openai_api" => Some(API_PROVIDER_OPENAI),
        _ => None,
    }
}

/// Provider/login profile stored under `authProfiles.<name>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderProfileSettings {
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub model: Option<String>,
    pub available_models: Option<Vec<String>>,
    pub model_capabilities: Option<HashMap<String, ModelCapabilitySettings>>,
    pub model_reasoning_effort: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub auth_source: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl ProviderProfileSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.backend.is_none()
            && self.api_provider.is_none()
            && self.model.is_none()
            && self
                .available_models
                .as_ref()
                .is_none_or(|models| models.is_empty())
            && self
                .model_capabilities
                .as_ref()
                .is_none_or(HashMap::is_empty)
            && self.model_reasoning_effort.is_none()
            && self.base_url.is_none()
            && self.api_key.is_none()
            && self.env.as_ref().is_none_or(HashMap::is_empty)
            && self.auth_source.is_none()
            && self.extra.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelCapabilitySettings {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub provider_options: Option<Value>,
    pub default_reasoning_level: Option<String>,
    pub supported_reasoning_levels: Vec<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub effective_context_window_percent: Option<u8>,
    pub supports_fast_mode: bool,
    pub supports_reasoning_summaries: bool,
    pub supports_reasoning: Option<bool>,
    pub supports_image_output: Option<bool>,
    pub supports_embedding: Option<bool>,
    pub support_verbosity: bool,
    pub supports_parallel_tool_calls: bool,
    pub supports_image_detail_original: bool,
    pub supports_search_tool: bool,
    pub supported_in_api: bool,
    pub input_modalities: Vec<String>,
    pub service_tiers: Vec<String>,
}

impl ModelCapabilitySettings {
    pub fn display_name_or<'a>(&'a self, model: &'a str) -> &'a str {
        self.display_name.as_deref().unwrap_or(model)
    }
}

pub(crate) fn merge_provider_profile(
    base: &mut ProviderProfileSettings,
    over: ProviderProfileSettings,
) {
    if over.backend.is_some() {
        base.backend = over.backend;
    }
    if over.api_provider.is_some() {
        base.api_provider = over.api_provider;
    }
    if over.model.is_some() {
        base.model = over.model;
    }
    if over.available_models.is_some() {
        base.available_models = over.available_models;
    }
    if over.model_capabilities.is_some() {
        base.model_capabilities = over.model_capabilities;
    }
    if over.model_reasoning_effort.is_some() {
        base.model_reasoning_effort = over.model_reasoning_effort;
    }
    if over.base_url.is_some() {
        base.base_url = over.base_url;
    }
    if over.api_key.is_some() {
        base.api_key = over.api_key;
    }
    if let Some(env) = over.env {
        let mut merged = base.env.take().unwrap_or_default();
        for (k, v) in env {
            merged.insert(k, v);
        }
        base.env = Some(merged);
    }
    if over.auth_source.is_some() {
        base.auth_source = over.auth_source;
    }
    for (k, v) in over.extra {
        base.extra.insert(k, v);
    }
}

pub fn codex_model_ids() -> Vec<String> {
    [
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.3-codex",
        "gpt-5.3-codex-spark",
        "gpt-5.2",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

pub fn codex_model_capabilities() -> HashMap<String, ModelCapabilitySettings> {
    codex_capability_entries()
        .into_iter()
        .map(|(id, capability)| (id.to_string(), capability))
        .collect()
}

struct CodexCapabilitySpec<'a> {
    display_name: &'a str,
    description: &'a str,
    default_reasoning_level: &'a str,
    context_window: u64,
    max_context_window: u64,
    supports_fast_mode: bool,
    supports_image_detail_original: bool,
    supported_in_api: bool,
    input_modalities: &'a [&'a str],
}

fn common_codex_capability(spec: CodexCapabilitySpec<'_>) -> ModelCapabilitySettings {
    ModelCapabilitySettings {
        display_name: Some(spec.display_name.to_string()),
        description: Some(spec.description.to_string()),
        default_reasoning_level: Some(spec.default_reasoning_level.to_string()),
        provider_options: None,
        supported_reasoning_levels: ["low", "medium", "high", "xhigh"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        context_window: Some(spec.context_window),
        max_context_window: Some(spec.max_context_window),
        max_output_tokens: None,
        effective_context_window_percent: Some(95),
        supports_fast_mode: spec.supports_fast_mode,
        supports_reasoning_summaries: true,
        supports_reasoning: None,
        supports_image_output: None,
        supports_embedding: None,
        support_verbosity: true,
        supports_parallel_tool_calls: true,
        supports_image_detail_original: spec.supports_image_detail_original,
        supports_search_tool: true,
        supported_in_api: spec.supported_in_api,
        input_modalities: spec
            .input_modalities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        service_tiers: if spec.supports_fast_mode {
            vec!["priority".to_string()]
        } else {
            Vec::new()
        },
    }
}

fn codex_capability_entries() -> Vec<(&'static str, ModelCapabilitySettings)> {
    vec![
        (
            "gpt-5.5",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.5",
                description: "Frontier model for complex coding, research, and real-world work.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: true,
                supports_image_detail_original: true,
                supported_in_api: true,
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.4",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "gpt-5.4",
                description: "Strong model for everyday coding.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 1_000_000,
                supports_fast_mode: true,
                supports_image_detail_original: true,
                supported_in_api: true,
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.4-mini",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.4-Mini",
                description: "Small, fast, and cost-efficient model for simpler coding tasks.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: false,
                supports_image_detail_original: true,
                supported_in_api: true,
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.3-codex",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "gpt-5.3-codex",
                description: "Coding-optimized model.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: false,
                supports_image_detail_original: true,
                supported_in_api: true,
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.3-codex-spark",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.3-Codex-Spark",
                description: "Ultra-fast coding model.",
                default_reasoning_level: "high",
                context_window: 128_000,
                max_context_window: 128_000,
                supports_fast_mode: false,
                supports_image_detail_original: false,
                supported_in_api: false,
                input_modalities: &["text"],
            }),
        ),
        (
            "gpt-5.2",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "gpt-5.2",
                description: "Optimized for professional work and long-running agents.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: false,
                supports_image_detail_original: false,
                supported_in_api: true,
                input_modalities: &["text", "image"],
            }),
        ),
    ]
}

pub fn upsert_auth_profile(
    raw: &mut RawSettings,
    name: &str,
    profile: ProviderProfileSettings,
    make_active: bool,
) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    let profiles = raw.auth_profiles.get_or_insert_with(HashMap::new);
    profiles
        .entry(trimmed.to_string())
        .and_modify(|existing| merge_provider_profile(existing, profile.clone()))
        .or_insert(profile);
    if make_active {
        raw.active_auth_profile = Some(trimmed.to_string());
    }
}

pub fn auth_profile_name_for_provider(api_provider: &str) -> &'static str {
    match normalize_api_provider(api_provider).unwrap_or(api_provider) {
        API_PROVIDER_OPENAI_CODEX => AUTH_PROFILE_CODEX,
        API_PROVIDER_ANTHROPIC => AUTH_PROFILE_CLAUDE_CODE,
        API_PROVIDER_OPENAI => AUTH_PROFILE_OPENAI,
        _ => AUTH_PROFILE_CUSTOM,
    }
}

pub fn auth_profile_lookup_names_for_provider(api_provider: &str) -> &'static [&'static str] {
    match normalize_api_provider(api_provider).unwrap_or(api_provider) {
        API_PROVIDER_OPENAI_CODEX => &[AUTH_PROFILE_CODEX],
        API_PROVIDER_ANTHROPIC => &[AUTH_PROFILE_CLAUDE_CODE, AUTH_PROFILE_ANTHROPIC_LEGACY],
        API_PROVIDER_OPENAI => &[AUTH_PROFILE_OPENAI],
        _ => &[AUTH_PROFILE_CUSTOM],
    }
}

pub fn get_auth_profile_for_provider<'a>(
    profiles: &'a HashMap<String, ProviderProfileSettings>,
    api_provider: &str,
) -> Option<&'a ProviderProfileSettings> {
    auth_profile_lookup_names_for_provider(api_provider)
        .iter()
        .find_map(|name| profiles.get(*name))
}

pub fn display_auth_profile_name(profile_name: &str) -> &str {
    if profile_name.eq_ignore_ascii_case(AUTH_PROFILE_ANTHROPIC_LEGACY) {
        AUTH_PROFILE_CLAUDE_CODE
    } else {
        profile_name
    }
}
