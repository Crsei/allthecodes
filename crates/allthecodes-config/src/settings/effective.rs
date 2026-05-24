use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use super::providers::ModelCapabilitySettings;
use super::providers::ProviderProfileSettings;
use super::raw::{merge_str_lists, RawSettings};
use super::source::{SettingsSource, SourceMap};
use super::types::{PermissionsSettings, SandboxSettings, SpinnerTipsSettings, StatusLineSettings};

// ---------------------------------------------------------------------------
// EffectiveSettings — runtime-ready, merged form
// ---------------------------------------------------------------------------

/// Fully-merged, runtime-ready settings.
///
/// Fields that have reasonable defaults are fully materialised (e.g.
/// `verbose: bool` rather than `Option<bool>`). Fields that have no
/// meaningful default stay `Option`.
///
/// Paired with a [`SourceMap`] via [`LoadedSettings`].
#[derive(Debug, Clone, Default)]
pub struct EffectiveSettings {
    // -- Legacy (consumed by main.rs) ----------------------------------
    pub model: Option<String>,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub active_auth_profile: Option<String>,
    pub auth_profiles: HashMap<String, ProviderProfileSettings>,
    pub theme: Option<String>,
    pub verbose: bool,
    pub permission_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub system_prompt: Option<String>,
    pub hooks: HashMap<String, Value>,
    pub claude_in_chrome_default_enabled: Option<bool>,
    pub api_key: Option<String>,
    pub env: HashMap<String, String>,
    pub extra: HashMap<String, Value>,

    // -- New typed fields ----------------------------------------------
    pub permissions: PermissionsSettings,
    pub sandbox: SandboxSettings,
    pub status_line: StatusLineSettings,
    pub spinner_tips: SpinnerTipsSettings,
    pub output_style: Option<String>,
    pub language: Option<String>,
    pub voice_enabled: Option<bool>,
    pub editor_mode: Option<String>,
    pub view_mode: Option<String>,
    pub terminal_progress_bar_enabled: Option<bool>,
    pub thinking: Option<Value>,
    pub output_config: Option<Value>,
    pub default_model: Option<String>,
    pub fallback_model: Option<String>,
    pub fast_model: Option<String>,
    pub sota_model: Option<String>,
    pub mota_model: Option<String>,
    pub fota_model: Option<String>,
    pub available_models: Vec<String>,
    pub model_capabilities: HashMap<String, ModelCapabilitySettings>,
    pub effort_level: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub fast_mode: Option<bool>,
    pub fast_mode_per_session_opt_in: Option<bool>,
    pub teammate_mode: Option<bool>,
    /// Auto-memory toggle (issue #45). `None` means "inherit default" (off).
    pub auto_memory_enabled: Option<bool>,
    /// Advisor model id (issue #33).
    pub advisor_model: Option<String>,
}

impl EffectiveSettings {
    pub(crate) fn from_raw(raw: RawSettings) -> Self {
        let mut perms = raw.permissions.unwrap_or_default();
        // Fold legacy top-level fields into the nested struct so downstream
        // code only needs to look in one place.
        if perms.default_mode.is_none() {
            perms.default_mode = raw.permission_mode.clone();
        }
        if let Some(legacy) = raw.allowed_tools.as_ref() {
            perms.allow = merge_str_lists(Some(&perms.allow), Some(legacy));
        }

        Self {
            model: raw.model,
            backend: raw.backend,
            api_provider: raw.api_provider,
            active_auth_profile: raw.active_auth_profile,
            auth_profiles: raw.auth_profiles.unwrap_or_default(),
            theme: raw.theme,
            verbose: raw.verbose.unwrap_or(false),
            permission_mode: perms.default_mode.clone().or(raw.permission_mode),
            allowed_tools: perms.allow.clone(),
            system_prompt: raw.system_prompt,
            hooks: raw.hooks.unwrap_or_default(),
            claude_in_chrome_default_enabled: raw.claude_in_chrome_default_enabled,
            api_key: raw.api_key,
            env: raw.env.unwrap_or_default(),
            extra: raw.extra,
            permissions: perms,
            sandbox: raw.sandbox.unwrap_or_default(),
            status_line: raw.status_line.unwrap_or_default(),
            spinner_tips: raw.spinner_tips.unwrap_or_default(),
            output_style: raw.output_style,
            language: raw.language,
            voice_enabled: raw.voice_enabled,
            editor_mode: raw.editor_mode,
            view_mode: raw.view_mode,
            terminal_progress_bar_enabled: raw.terminal_progress_bar_enabled,
            thinking: raw.thinking,
            output_config: raw.output_config,
            default_model: raw.default_model,
            fallback_model: raw.fallback_model,
            fast_model: raw.fast_model,
            sota_model: raw.sota_model,
            mota_model: raw.mota_model,
            fota_model: raw.fota_model,
            available_models: raw.available_models.unwrap_or_default(),
            model_capabilities: HashMap::new(),
            effort_level: raw.effort_level,
            model_reasoning_effort: raw.model_reasoning_effort,
            fast_mode: raw.fast_mode,
            fast_mode_per_session_opt_in: raw.fast_mode_per_session_opt_in,
            teammate_mode: raw.teammate_mode,
            auto_memory_enabled: raw.auto_memory_enabled,
            advisor_model: raw.advisor_model,
        }
    }
}

// ---------------------------------------------------------------------------
// LoadedSettings — effective + raw layers + source map
// ---------------------------------------------------------------------------

/// Result of [`load_effective`]. Holds the merged [`EffectiveSettings`]
/// and a [`SourceMap`] recording which layer provided each key, plus the
/// raw per-layer contents for diagnostics.
#[derive(Debug, Clone, Default)]
pub struct LoadedSettings {
    pub effective: EffectiveSettings,
    pub sources: SourceMap,
    pub managed: Option<RawSettings>,
    pub user: Option<RawSettings>,
    pub project: Option<RawSettings>,
    pub local: Option<RawSettings>,
    /// Paths that were actually read (present on disk).
    pub loaded_paths: Vec<(SettingsSource, PathBuf)>,
}

impl LoadedSettings {
    /// Source of a specific key (e.g. `"model"`, `"permissions"`).
    ///
    /// Returns [`SettingsSource::Default`] if no layer provided the key.
    /// Used by `/config sources` and tests; reserved for downstream callers
    /// that want to inspect provenance without iterating the full map.
    pub fn source_of(&self, key: &str) -> SettingsSource {
        self.sources
            .get(key)
            .copied()
            .unwrap_or(SettingsSource::Default)
    }
}
