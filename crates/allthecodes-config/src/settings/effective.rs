use std::collections::HashMap;
use std::path::PathBuf;

use allthecodes_types::mcp::McpBinding;
use serde_json::Value;

use super::layers::{ConfigLayer, ConfigLayerEntry};
use super::providers::ModelCapabilitySettings;
use super::providers::ProviderProfileSettings;
use super::raw::{merge_str_lists, RawSettings};
use super::requirements::RequirementViolation;
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
    pub mcp_bindings: Vec<McpBinding>,
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
    pub app_icon: Option<String>,
    pub auto_start: Option<bool>,
    pub start_minimized: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub quick_chat_hide_on_blur: Option<bool>,
    pub quick_chat_inject_screen: Option<bool>,
    pub quick_chat_ambient: Option<bool>,
    pub auto_approve_tools: Option<bool>,
    pub analytics_enabled: Option<bool>,
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
    pub context_window: Option<u64>,
    pub max_messages: Option<u64>,
    pub auto_title: Option<bool>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub streaming: Option<bool>,
    pub show_token_usage: Option<bool>,
    pub show_reasoning_details: Option<bool>,
    pub markdown_rendering: Option<bool>,
    pub single_dollar_math: Option<bool>,
    pub infographic: Option<bool>,
    pub auto_collapse_reasoning: Option<bool>,
    pub quick_reply_suggestions: Option<bool>,
    pub default_tool_selection: Option<String>,
    pub default_skill_selection: Option<String>,
    pub sound_effects: Option<bool>,
    pub auto_compact: Option<bool>,
    pub compact_threshold: Option<u8>,
    pub keep_recent_messages: Option<u8>,
    pub hashline_mode: Option<bool>,
    pub hermes_enabled: Option<bool>,
    pub teammate_mode: Option<bool>,
    /// Auto-memory toggle (issue #45). `None` means "inherit default" (off).
    pub auto_memory_enabled: Option<bool>,
    pub memory_auto_retrieve: Option<bool>,
    pub memory_query_rewriting: Option<bool>,
    pub memory_max_retrieved: Option<u8>,
    pub memory_similarity_threshold: Option<u8>,
    pub memory_auto_summarize: Option<bool>,
    pub memory_nightly: Option<bool>,
    pub memory_sleep_time: Option<String>,
    pub memory_temp_ttl: Option<u32>,
    pub memory_archive_retention: Option<u32>,
    pub memory_tool_model: Option<String>,
    pub memory_embedding_model: Option<String>,
    pub proxy_enabled: Option<bool>,
    pub proxy_url: Option<String>,
    pub prefer_ipv4: Option<bool>,
    pub request_timeout: Option<u64>,
    pub retry_attempts: Option<u8>,
    pub custom_user_agent: Option<String>,
    pub speech_enabled: Option<bool>,
    pub speech_active_model: Option<String>,
    pub speech_language: Option<String>,
    pub tts_provider: Option<String>,
    pub tts_api_key: Option<String>,
    pub tts_voice: Option<String>,
    pub tts_voice_custom_id: Option<String>,
    pub tts_model: Option<String>,
    pub search_engine: Option<String>,
    pub web_search_provider: Option<String>,
    pub web_search_tavily_api_key: Option<String>,
    pub web_search_brave_api_key: Option<String>,
    pub cloud_sync_enabled: Option<bool>,
    pub cloud_sync_path: Option<String>,
    pub token_savings_tracking: Option<bool>,
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
            mcp_bindings: raw.mcp_bindings.unwrap_or_default(),
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
            app_icon: raw.app_icon,
            auto_start: raw.auto_start,
            start_minimized: raw.start_minimized,
            minimize_to_tray: raw.minimize_to_tray,
            close_to_tray: raw.close_to_tray,
            quick_chat_hide_on_blur: raw.quick_chat_hide_on_blur,
            quick_chat_inject_screen: raw.quick_chat_inject_screen,
            quick_chat_ambient: raw.quick_chat_ambient,
            auto_approve_tools: raw.auto_approve_tools,
            analytics_enabled: raw.analytics_enabled,
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
            context_window: raw.context_window,
            max_messages: raw.max_messages,
            auto_title: raw.auto_title,
            temperature: raw.temperature,
            max_tokens: raw.max_tokens,
            streaming: raw.streaming,
            show_token_usage: raw.show_token_usage,
            show_reasoning_details: raw.show_reasoning_details,
            markdown_rendering: raw.markdown_rendering,
            single_dollar_math: raw.single_dollar_math,
            infographic: raw.infographic,
            auto_collapse_reasoning: raw.auto_collapse_reasoning,
            quick_reply_suggestions: raw.quick_reply_suggestions,
            default_tool_selection: raw.default_tool_selection,
            default_skill_selection: raw.default_skill_selection,
            sound_effects: raw.sound_effects,
            auto_compact: raw.auto_compact,
            compact_threshold: raw.compact_threshold,
            keep_recent_messages: raw.keep_recent_messages,
            hashline_mode: raw.hashline_mode,
            hermes_enabled: raw.hermes_enabled,
            teammate_mode: raw.teammate_mode,
            auto_memory_enabled: raw.auto_memory_enabled,
            memory_auto_retrieve: raw.memory_auto_retrieve,
            memory_query_rewriting: raw.memory_query_rewriting,
            memory_max_retrieved: raw.memory_max_retrieved,
            memory_similarity_threshold: raw.memory_similarity_threshold,
            memory_auto_summarize: raw.memory_auto_summarize,
            memory_nightly: raw.memory_nightly,
            memory_sleep_time: raw.memory_sleep_time,
            memory_temp_ttl: raw.memory_temp_ttl,
            memory_archive_retention: raw.memory_archive_retention,
            memory_tool_model: raw.memory_tool_model,
            memory_embedding_model: raw.memory_embedding_model,
            proxy_enabled: raw.proxy_enabled,
            proxy_url: raw.proxy_url,
            prefer_ipv4: raw.prefer_ipv4,
            request_timeout: raw.request_timeout,
            retry_attempts: raw.retry_attempts,
            custom_user_agent: raw.custom_user_agent,
            speech_enabled: raw.speech_enabled,
            speech_active_model: raw.speech_active_model,
            speech_language: raw.speech_language,
            tts_provider: raw.tts_provider,
            tts_api_key: raw.tts_api_key,
            tts_voice: raw.tts_voice,
            tts_voice_custom_id: raw.tts_voice_custom_id,
            tts_model: raw.tts_model,
            search_engine: raw.search_engine,
            web_search_provider: raw.web_search_provider,
            web_search_tavily_api_key: raw.web_search_tavily_api_key,
            web_search_brave_api_key: raw.web_search_brave_api_key,
            cloud_sync_enabled: raw.cloud_sync_enabled,
            cloud_sync_path: raw.cloud_sync_path,
            token_savings_tracking: raw.token_savings_tracking,
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
    pub layers: Vec<ConfigLayer>,
    pub entries: Vec<ConfigLayerEntry>,
    pub requirement_violations: Vec<RequirementViolation>,
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
