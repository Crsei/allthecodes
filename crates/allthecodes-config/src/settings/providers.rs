use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::raw::RawSettings;

pub const API_PROVIDER_ANTHROPIC: &str = "anthropic";
pub const API_PROVIDER_OPENAI_CODEX: &str = "openai-codex";
pub const API_PROVIDER_OPENAI: &str = "openai";
pub const API_PROVIDER_BEDROCK: &str = "bedrock";
pub const API_PROVIDER_VERTEX: &str = "vertex";
pub const API_PROVIDER_FOUNDRY: &str = "azure-foundry";
pub const AUTH_PROFILE_CLAUDE_CODE: &str = "claude_code";
pub const AUTH_PROFILE_ANTHROPIC_LEGACY: &str = "anthropic";
pub const AUTH_PROFILE_CODEX: &str = "codex";
pub const AUTH_PROFILE_OPENAI: &str = "openai";
pub const AUTH_PROFILE_CUSTOM: &str = "custom";
pub const VALID_API_PROVIDERS: &[&str] = &[
    "anthropic",
    "azure",
    "openai",
    "openai-codex",
    "google",
    "groq",
    "openrouter",
    "deepseek",
    "zhipu",
    "qwen",
    "moonshot",
    "baichuan",
    "minimax",
    "yi",
    "siliconflow",
    "stepfun",
    "spark",
    "bedrock",
    "vertex",
    "azure-foundry",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRuntimeSupport {
    Supported,
    Unsupported,
    Unknown,
}

pub fn provider_runtime_support(value: &str) -> ProviderRuntimeSupport {
    match normalize_api_provider(value) {
        Some(API_PROVIDER_FOUNDRY) => ProviderRuntimeSupport::Unsupported,
        Some(_) => ProviderRuntimeSupport::Supported,
        None => ProviderRuntimeSupport::Unknown,
    }
}

pub fn normalize_api_provider(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "anthropic" | "anthropic-method" | "anthropic_method" => Some(API_PROVIDER_ANTHROPIC),
        "openai-codex" | "openai_codex" | "codex" => Some(API_PROVIDER_OPENAI_CODEX),
        "openai" | "openai-api" | "openai_api" => Some(API_PROVIDER_OPENAI),
        "azure-openai" => Some("azure"),
        "foundry" | "microsoft-foundry" => Some(API_PROVIDER_FOUNDRY),
        other => VALID_API_PROVIDERS
            .iter()
            .copied()
            .find(|known| *known == other),
    }
}

pub fn provider_env_key(value: &str) -> Option<&'static str> {
    match normalize_api_provider(value)? {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "azure" => Some("AZURE_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "openai-codex" => Some("OPENAI_CODEX_AUTH_TOKEN"),
        "google" => Some("GOOGLE_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "zhipu" => Some("ZHIPU_API_KEY"),
        "qwen" => Some("DASHSCOPE_API_KEY"),
        "moonshot" => Some("MOONSHOT_API_KEY"),
        "baichuan" => Some("BAICHUAN_API_KEY"),
        "minimax" => Some("MINIMAX_API_KEY"),
        "yi" => Some("YI_API_KEY"),
        "siliconflow" => Some("SILICONFLOW_API_KEY"),
        "stepfun" => Some("STEPFUN_API_KEY"),
        "spark" => Some("SPARK_API_KEY"),
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
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
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

// ---------------------------------------------------------------------------
// Bundled Codex capability catalog provenance
// ---------------------------------------------------------------------------
//
// `codex_capability_entries()` is a *bundled* snapshot of the Codex model
// line that ships with this binary. It is not a live query to any provider
// API — when Codex releases a new model or changes a reasoning-level set,
// this table only changes by shipping a new binary. The two constants below
// are exposed so that runtime callers (`/login`, `/model`, `/fast`,
// `provenance_for_model`) can label the origin of `modelCapabilities` they
// copy into user settings, and so settings-tooling can detect "this entry
// came from a bundled snapshot of version X" vs "a user edited this entry".
//
// Update protocol:
//   1. Bump `BUNDLED_CODEX_CATALOG_VERSION` (semver-style, e.g. "2026.07.18"
//      for a date-stamped catalog, or `"0.2.0"` for a numbered catalog).
//   2. Update `BUNDLED_CODEX_CATALOG_UPDATED` to the merge date.
//   3. Update `codex_capability_entries()` below with the new model rows.
//   4. Run `cargo test -p allthecodes-config --lib settings::providers` to
//      confirm the catalog still serializes round-trip and that
//      `bundled_codex_catalog_provenance()` returns the new constants.

/// Version stamp for the bundled Codex capability catalog in this binary.
/// Bumped whenever `codex_capability_entries()` is updated.
pub const BUNDLED_CODEX_CATALOG_VERSION: &str = "2026.07.18";

/// ISO-style date (UTC) on which `codex_capability_entries()` was last
/// refreshed in this binary. Used for the "updated" provenance field.
pub const BUNDLED_CODEX_CATALOG_UPDATED: &str = "2026-07-18";

/// Maintenance source label exposed to runtime/UI consumers. Stable string;
/// do not include the version here (callers compose it with the constants).
pub const BUNDLED_CODEX_CATALOG_SOURCE: &str = "allthecodes-bundled-codex-catalog";

/// Identifier for entries the bundled catalog stamps into user settings via
/// `copy_bundled_catalog_into_profile`. Used by legacy-snapshot detection
/// to distinguish "snapshotted from binary revision X" vs "user edited".
pub fn bundled_codex_catalog_provenance() -> &'static str {
    BUNDLED_CODEX_CATALOG_SOURCE
}

/// Composed provenance string for display, e.g.
/// `allthecodes-bundled-codex-catalog@2026.07.18 (2026-07-18)`.
pub fn bundled_codex_catalog_display_label() -> String {
    format!(
        "{}@{} ({})",
        BUNDLED_CODEX_CATALOG_SOURCE, BUNDLED_CODEX_CATALOG_VERSION, BUNDLED_CODEX_CATALOG_UPDATED
    )
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
    supported_reasoning_levels: &'a [&'a str],
    input_modalities: &'a [&'a str],
}

fn common_codex_capability(spec: CodexCapabilitySpec<'_>) -> ModelCapabilitySettings {
    ModelCapabilitySettings {
        display_name: Some(spec.display_name.to_string()),
        description: Some(spec.description.to_string()),
        default_reasoning_level: Some(spec.default_reasoning_level.to_string()),
        provider_options: None,
        supported_reasoning_levels: spec
            .supported_reasoning_levels
            .iter()
            .copied()
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
            "gpt-5.6-sol",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.6-Sol",
                description: "Latest frontier agentic coding model.",
                default_reasoning_level: "low",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: true,
                supports_image_detail_original: true,
                supported_in_api: true,
                supported_reasoning_levels: &["low", "medium", "high", "xhigh", "max"],
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.6-terra",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.6-Terra",
                description: "Balanced agentic coding model for everyday work.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: true,
                supports_image_detail_original: true,
                supported_in_api: true,
                supported_reasoning_levels: &["low", "medium", "high", "xhigh", "max"],
                input_modalities: &["text", "image"],
            }),
        ),
        (
            "gpt-5.6-luna",
            common_codex_capability(CodexCapabilitySpec {
                display_name: "GPT-5.6-Luna",
                description: "Fast and affordable agentic coding model.",
                default_reasoning_level: "medium",
                context_window: 272_000,
                max_context_window: 272_000,
                supports_fast_mode: true,
                supports_image_detail_original: true,
                supported_in_api: true,
                supported_reasoning_levels: &["low", "medium", "high", "xhigh", "max"],
                input_modalities: &["text", "image"],
            }),
        ),
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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
                supported_reasoning_levels: &["low", "medium", "high", "xhigh"],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_codex_catalog_provenance_constants_are_stable() {
        // Stable machine identifier used to mark bundled-snapshot entries in
        // user settings. Changing this string breaks the legacy-snapshot
        // detector; bump via a coordinated migration instead.
        assert_eq!(
            BUNDLED_CODEX_CATALOG_SOURCE,
            "allthecodes-bundled-codex-catalog"
        );
        // Clippy flags `!const.is_empty()` as always-false since the const is
        // a non-empty literal at compile time; bind the value through a `let`
        // so the assertion actually executes if the constant ever changes.
        let version = BUNDLED_CODEX_CATALOG_VERSION;
        let updated = BUNDLED_CODEX_CATALOG_UPDATED;
        assert!(!version.is_empty(), "version must never be empty");
        assert!(!updated.is_empty(), "updated timestamp must never be empty");
    }

    #[test]
    fn bundled_codex_catalog_display_label_composes_source_version_and_date() {
        let label = bundled_codex_catalog_display_label();
        assert!(
            label.starts_with(BUNDLED_CODEX_CATALOG_SOURCE),
            "label should start with the source identifier: {label}"
        );
        assert!(
            label.contains(BUNDLED_CODEX_CATALOG_VERSION),
            "label should embed the version: {label}"
        );
        assert!(
            label.contains(BUNDLED_CODEX_CATALOG_UPDATED),
            "label should embed the updated date: {label}"
        );
    }

    #[test]
    fn bundled_codex_catalog_provenance_accessor_matches_constant() {
        assert_eq!(
            bundled_codex_catalog_provenance(),
            BUNDLED_CODEX_CATALOG_SOURCE
        );
    }

    #[test]
    fn codex_catalog_starts_with_gpt_5_6_family() {
        let models = codex_model_ids();
        assert_eq!(
            &models[..3],
            &["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
        );
    }

    #[test]
    fn gpt_5_6_capabilities_include_max_reasoning() {
        let capabilities = codex_model_capabilities();
        for model in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            let capability = capabilities.get(model).expect("GPT-5.6 capability");
            assert!(capability
                .supported_reasoning_levels
                .contains(&"max".to_string()));
            assert!(capability.supports_image_detail_original);
            assert!(capability.supports_fast_mode);
        }
        assert_eq!(
            capabilities["gpt-5.6-sol"]
                .default_reasoning_level
                .as_deref(),
            Some("low")
        );
    }
}
