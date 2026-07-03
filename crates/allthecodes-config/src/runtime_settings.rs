//! Runtime projection of effective settings.
//!
//! This type used to live in `types::app_state` in the root crate. It was
//! moved here in Phase 3 (issue #72) because:
//!
//! 1. Its fields already reference concrete types from [`crate::settings`]
//!    (`PermissionsSettings`, `SandboxSettings`, `StatusLineSettings`,
//!    `SpinnerTipsSettings`, `SourceMap`), so cc-config is the natural
//!    home.
//! 2. `cc-config::validation` reads `SettingsJson` directly; keeping
//!    `SettingsJson` in the root crate would force a reverse dep
//!    cc-config → allthecodes.
//!
//! The root crate keeps `types::app_state::SettingsJson` as a re-export of
//! this type so existing call sites compile unchanged.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::settings::{
    EffectiveSettings, ModelCapabilitySettings, PermissionsSettings as SettingsPermissionsSettings,
    ProviderProfileSettings, SandboxSettings as SettingsSandboxSettings, SourceMap,
    SpinnerTipsSettings, StatusLineSettings,
};

#[derive(Debug, Clone, Default)]
pub struct RuntimeSettings {
    pub core: CoreSettings,
    pub model: ModelSettings,
    pub permissions: PermissionSettings,
    pub sandbox: SandboxSettings,
    pub ui: UiSettings,
    pub memory: MemorySettings,
    pub network: NetworkSettings,
    pub speech: SpeechSettings,
    pub integrations: IntegrationSettings,
    pub sources: SourceMap,
}

#[derive(Debug, Clone, Default)]
pub struct CoreSettings {
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub active_auth_profile: Option<String>,
    pub auth_profiles: HashMap<String, ProviderProfileSettings>,
    pub theme: Option<String>,
    pub verbose: Option<bool>,
    pub extra: HashMap<String, Value>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelSettings {
    pub model: Option<String>,
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
    pub advisor_model: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PermissionSettings {
    pub permission_mode: Option<String>,
    pub permissions: SettingsPermissionsSettings,
}

#[derive(Debug, Clone, Default)]
pub struct SandboxSettings {
    pub sandbox: SettingsSandboxSettings,
}

#[derive(Debug, Clone, Default)]
pub struct UiSettings {
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
}

#[derive(Debug, Clone, Default)]
pub struct MemorySettings {
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
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSettings {
    pub env: HashMap<String, String>,
    pub proxy_enabled: Option<bool>,
    pub proxy_url: Option<String>,
    pub prefer_ipv4: Option<bool>,
    pub request_timeout: Option<u64>,
    pub retry_attempts: Option<u8>,
    pub custom_user_agent: Option<String>,
    pub search_engine: Option<String>,
    pub web_search_provider: Option<String>,
    pub web_search_tavily_api_key: Option<String>,
    pub web_search_brave_api_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SpeechSettings {
    pub speech_enabled: Option<bool>,
    pub speech_active_model: Option<String>,
    pub speech_language: Option<String>,
    pub tts_provider: Option<String>,
    pub tts_api_key: Option<String>,
    pub tts_voice: Option<String>,
    pub tts_voice_custom_id: Option<String>,
    pub tts_model: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct IntegrationSettings {
    pub teammate_mode: Option<bool>,
    pub claude_in_chrome_default_enabled: Option<bool>,
    pub cloud_sync_enabled: Option<bool>,
    pub cloud_sync_path: Option<String>,
    pub token_savings_tracking: Option<bool>,
}

impl RuntimeSettings {
    pub fn from_effective(effective: &EffectiveSettings, sources: SourceMap) -> Self {
        Self {
            core: CoreSettings {
                backend: effective.backend.clone(),
                api_provider: effective.api_provider.clone(),
                active_auth_profile: effective.active_auth_profile.clone(),
                auth_profiles: effective.auth_profiles.clone(),
                theme: effective.theme.clone(),
                verbose: Some(effective.verbose),
                extra: effective.extra.clone(),
                system_prompt: effective.system_prompt.clone(),
            },
            model: ModelSettings {
                model: effective.model.clone(),
                thinking: effective.thinking.clone(),
                output_config: effective.output_config.clone(),
                default_model: effective.default_model.clone(),
                fallback_model: effective.fallback_model.clone(),
                fast_model: effective.fast_model.clone(),
                sota_model: effective.sota_model.clone(),
                mota_model: effective.mota_model.clone(),
                fota_model: effective.fota_model.clone(),
                available_models: effective.available_models.clone(),
                model_capabilities: effective.model_capabilities.clone(),
                effort_level: effective.effort_level.clone(),
                model_reasoning_effort: effective.model_reasoning_effort.clone(),
                fast_mode: effective.fast_mode,
                fast_mode_per_session_opt_in: effective.fast_mode_per_session_opt_in,
                context_window: effective.context_window,
                max_messages: effective.max_messages,
                auto_title: effective.auto_title,
                temperature: effective.temperature,
                max_tokens: effective.max_tokens,
                streaming: effective.streaming,
                show_token_usage: effective.show_token_usage,
                show_reasoning_details: effective.show_reasoning_details,
                markdown_rendering: effective.markdown_rendering,
                single_dollar_math: effective.single_dollar_math,
                infographic: effective.infographic,
                auto_collapse_reasoning: effective.auto_collapse_reasoning,
                quick_reply_suggestions: effective.quick_reply_suggestions,
                default_tool_selection: effective.default_tool_selection.clone(),
                default_skill_selection: effective.default_skill_selection.clone(),
                sound_effects: effective.sound_effects,
                auto_compact: effective.auto_compact,
                compact_threshold: effective.compact_threshold,
                keep_recent_messages: effective.keep_recent_messages,
                hashline_mode: effective.hashline_mode,
                advisor_model: effective.advisor_model.clone(),
            },
            permissions: PermissionSettings {
                permission_mode: effective.permission_mode.clone(),
                permissions: effective.permissions.clone(),
            },
            sandbox: SandboxSettings {
                sandbox: effective.sandbox.clone(),
            },
            ui: UiSettings {
                status_line: effective.status_line.clone(),
                spinner_tips: effective.spinner_tips.clone(),
                output_style: effective.output_style.clone(),
                language: effective.language.clone(),
                voice_enabled: effective.voice_enabled,
                editor_mode: effective.editor_mode.clone(),
                view_mode: effective.view_mode.clone(),
                terminal_progress_bar_enabled: effective.terminal_progress_bar_enabled,
                app_icon: effective.app_icon.clone(),
                auto_start: effective.auto_start,
                start_minimized: effective.start_minimized,
                minimize_to_tray: effective.minimize_to_tray,
                close_to_tray: effective.close_to_tray,
                quick_chat_hide_on_blur: effective.quick_chat_hide_on_blur,
                quick_chat_inject_screen: effective.quick_chat_inject_screen,
                quick_chat_ambient: effective.quick_chat_ambient,
                auto_approve_tools: effective.auto_approve_tools,
                analytics_enabled: effective.analytics_enabled,
            },
            memory: MemorySettings {
                auto_memory_enabled: effective.auto_memory_enabled,
                memory_auto_retrieve: effective.memory_auto_retrieve,
                memory_query_rewriting: effective.memory_query_rewriting,
                memory_max_retrieved: effective.memory_max_retrieved,
                memory_similarity_threshold: effective.memory_similarity_threshold,
                memory_auto_summarize: effective.memory_auto_summarize,
                memory_nightly: effective.memory_nightly,
                memory_sleep_time: effective.memory_sleep_time.clone(),
                memory_temp_ttl: effective.memory_temp_ttl,
                memory_archive_retention: effective.memory_archive_retention,
                memory_tool_model: effective.memory_tool_model.clone(),
                memory_embedding_model: effective.memory_embedding_model.clone(),
            },
            network: NetworkSettings {
                env: effective.env.clone(),
                proxy_enabled: effective.proxy_enabled,
                proxy_url: effective.proxy_url.clone(),
                prefer_ipv4: effective.prefer_ipv4,
                request_timeout: effective.request_timeout,
                retry_attempts: effective.retry_attempts,
                custom_user_agent: effective.custom_user_agent.clone(),
                search_engine: effective.search_engine.clone(),
                web_search_provider: effective.web_search_provider.clone(),
                web_search_tavily_api_key: effective.web_search_tavily_api_key.clone(),
                web_search_brave_api_key: effective.web_search_brave_api_key.clone(),
            },
            speech: SpeechSettings {
                speech_enabled: effective.speech_enabled,
                speech_active_model: effective.speech_active_model.clone(),
                speech_language: effective.speech_language.clone(),
                tts_provider: effective.tts_provider.clone(),
                tts_api_key: effective.tts_api_key.clone(),
                tts_voice: effective.tts_voice.clone(),
                tts_voice_custom_id: effective.tts_voice_custom_id.clone(),
                tts_model: effective.tts_model.clone(),
            },
            integrations: IntegrationSettings {
                teammate_mode: effective.teammate_mode,
                claude_in_chrome_default_enabled: effective.claude_in_chrome_default_enabled,
                cloud_sync_enabled: effective.cloud_sync_enabled,
                cloud_sync_path: effective.cloud_sync_path.clone(),
                token_savings_tracking: effective.token_savings_tracking,
            },
            sources,
        }
    }
}

/// Runtime projection of [`crate::settings::EffectiveSettings`] —
/// start-up merges raw settings into this, `/config set` writes back here,
/// and serialization converts it to [`crate::settings::RawSettings`].
#[derive(Debug, Clone, Default)]
pub struct SettingsJson {
    // -- Core identity --------------------------------------------------
    pub model: Option<String>,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub active_auth_profile: Option<String>,
    pub auth_profiles: HashMap<String, ProviderProfileSettings>,
    pub theme: Option<String>,
    pub verbose: Option<bool>,
    pub extra: HashMap<String, Value>,
    pub env: HashMap<String, String>,

    // -- Permissions / sandbox -----------------------------------------
    pub permission_mode: Option<String>,
    pub permissions: SettingsPermissionsSettings,
    pub sandbox: SettingsSandboxSettings,

    // -- UI / UX --------------------------------------------------------
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

    // -- Models / effort -----------------------------------------------
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
    /// Optional advisor model id (issue #33). Persisted under
    /// `settings.json::advisorModel`. When set and the active provider
    /// supports advisors, this model is attached to the Messages request
    /// via `MessagesRequest::advisor_model`.
    pub advisor_model: Option<String>,

    // -- Prompts --------------------------------------------------------
    pub system_prompt: Option<String>,

    // -- Modes / integrations ------------------------------------------
    pub teammate_mode: Option<bool>,
    pub claude_in_chrome_default_enabled: Option<bool>,

    // -- Memory (issue #45) --------------------------------------------
    /// Whether auto-memory capture + injection is enabled for this session.
    /// Persisted via `settings.json::autoMemoryEnabled`; toggled by
    /// `/memory auto on|off`. Default is `None` (off).
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

    // -- Network / search / speech -------------------------------------
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

    // -- Data / savings -------------------------------------------------
    pub cloud_sync_enabled: Option<bool>,
    pub cloud_sync_path: Option<String>,
    pub token_savings_tracking: Option<bool>,

    // -- Per-key source (provenance) -----------------------------------
    /// 来源映射: key -> 哪个 layer 提供了该值。由启动路径 + `/config set`
    /// 在写入对应键时一并更新。`/config show` 读取此 map 显示来源信息。
    pub sources: SourceMap,
}

impl SettingsJson {
    pub fn from_effective(effective: &EffectiveSettings, sources: SourceMap) -> Self {
        RuntimeSettings::from_effective(effective, sources).into()
    }

    /// Flatten user-facing settings into the map consumed by the web UI.
    /// Sensitive values are intentionally omitted.
    pub fn settings_map(&self) -> HashMap<String, Value> {
        let mut out = sanitized_extra_map(&self.extra);

        macro_rules! insert_opt {
            ($key:expr, $field:expr) => {
                if let Some(value) = $field {
                    out.insert($key.to_string(), json!(value));
                }
            };
        }
        macro_rules! insert_opt_ref {
            ($key:expr, $field:expr) => {
                if let Some(value) = $field.as_ref() {
                    out.insert($key.to_string(), json!(value));
                }
            };
        }

        insert_opt_ref!("model", self.model);
        insert_opt_ref!("backend", self.backend);
        insert_opt_ref!("api_provider", self.api_provider);
        insert_opt_ref!("active_auth_profile", self.active_auth_profile);
        insert_opt_ref!("theme", self.theme);
        insert_opt!("verbose", self.verbose);
        insert_opt_ref!("permission_mode", self.permission_mode);
        insert_opt_ref!("output_style", self.output_style);
        insert_opt_ref!("language", self.language);
        insert_opt!("voice_enabled", self.voice_enabled);
        insert_opt_ref!("editor_mode", self.editor_mode);
        insert_opt_ref!("view_mode", self.view_mode);
        insert_opt!(
            "terminal_progress_bar_enabled",
            self.terminal_progress_bar_enabled
        );
        insert_opt_ref!("app_icon", self.app_icon);
        insert_opt!("auto_start", self.auto_start);
        insert_opt!("start_minimized", self.start_minimized);
        insert_opt!("minimize_to_tray", self.minimize_to_tray);
        insert_opt!("close_to_tray", self.close_to_tray);
        insert_opt!("quick_chat_hide_on_blur", self.quick_chat_hide_on_blur);
        insert_opt!("quick_chat_inject_screen", self.quick_chat_inject_screen);
        insert_opt!("quick_chat_ambient", self.quick_chat_ambient);
        insert_opt!("auto_approve_tools", self.auto_approve_tools);
        insert_opt!("analytics_enabled", self.analytics_enabled);
        insert_opt_ref!("default_model", self.default_model);
        insert_opt_ref!("fallback_model", self.fallback_model);
        insert_opt_ref!("fast_model", self.fast_model);
        insert_opt_ref!("sota_model", self.sota_model);
        insert_opt_ref!("mota_model", self.mota_model);
        insert_opt_ref!("fota_model", self.fota_model);
        insert_opt_ref!("effort_level", self.effort_level);
        insert_opt_ref!("model_reasoning_effort", self.model_reasoning_effort);
        insert_opt!("fast_mode", self.fast_mode);
        insert_opt!(
            "fast_mode_per_session_opt_in",
            self.fast_mode_per_session_opt_in
        );
        insert_opt!("context_window", self.context_window);
        insert_opt!("max_messages", self.max_messages);
        insert_opt!("auto_title", self.auto_title);
        insert_opt!("temperature", self.temperature);
        insert_opt!("max_tokens", self.max_tokens);
        insert_opt!("streaming", self.streaming);
        insert_opt!("show_token_usage", self.show_token_usage);
        insert_opt!("show_reasoning_details", self.show_reasoning_details);
        insert_opt!("markdown_rendering", self.markdown_rendering);
        insert_opt!("single_dollar_math", self.single_dollar_math);
        insert_opt!("infographic", self.infographic);
        insert_opt!("auto_collapse_reasoning", self.auto_collapse_reasoning);
        insert_opt!("quick_reply_suggestions", self.quick_reply_suggestions);
        insert_opt_ref!("default_tool_selection", self.default_tool_selection);
        insert_opt_ref!("default_skill_selection", self.default_skill_selection);
        insert_opt!("sound_effects", self.sound_effects);
        insert_opt!("auto_compact", self.auto_compact);
        insert_opt!("compact_threshold", self.compact_threshold);
        insert_opt!("keep_recent_messages", self.keep_recent_messages);
        insert_opt!("hashline_mode", self.hashline_mode);
        insert_opt!("teammate_mode", self.teammate_mode);
        insert_opt!(
            "claude_in_chrome_default_enabled",
            self.claude_in_chrome_default_enabled
        );
        insert_opt!("auto_memory_enabled", self.auto_memory_enabled);
        insert_opt!("memory_auto_retrieve", self.memory_auto_retrieve);
        insert_opt!("memory_query_rewriting", self.memory_query_rewriting);
        insert_opt!("memory_max_retrieved", self.memory_max_retrieved);
        insert_opt!(
            "memory_similarity_threshold",
            self.memory_similarity_threshold
        );
        insert_opt!("memory_auto_summarize", self.memory_auto_summarize);
        insert_opt!("memory_nightly", self.memory_nightly);
        insert_opt_ref!("memory_sleep_time", self.memory_sleep_time);
        insert_opt!("memory_temp_ttl", self.memory_temp_ttl);
        insert_opt!("memory_archive_retention", self.memory_archive_retention);
        insert_opt_ref!("memory_tool_model", self.memory_tool_model);
        insert_opt_ref!("memory_embedding_model", self.memory_embedding_model);
        insert_opt!("proxy_enabled", self.proxy_enabled);
        insert_opt_ref!("proxy_url", self.proxy_url);
        insert_opt!("prefer_ipv4", self.prefer_ipv4);
        insert_opt!("request_timeout", self.request_timeout);
        insert_opt!("retry_attempts", self.retry_attempts);
        insert_opt_ref!("custom_user_agent", self.custom_user_agent);
        insert_opt!("speech_enabled", self.speech_enabled);
        insert_opt_ref!("speech_active_model", self.speech_active_model);
        insert_opt_ref!("speech_language", self.speech_language);
        insert_opt_ref!("tts_provider", self.tts_provider);
        insert_opt_ref!("tts_voice", self.tts_voice);
        insert_opt_ref!("tts_voice_custom_id", self.tts_voice_custom_id);
        insert_opt_ref!("tts_model", self.tts_model);
        insert_opt_ref!("search_engine", self.search_engine);
        insert_opt_ref!("web_search_provider", self.web_search_provider);
        insert_opt!(
            "web_search_tavily_configured",
            self.web_search_tavily_api_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
        );
        insert_opt!(
            "web_search_brave_configured",
            self.web_search_brave_api_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
        );
        insert_opt!("cloud_sync_enabled", self.cloud_sync_enabled);
        insert_opt_ref!("cloud_sync_path", self.cloud_sync_path);
        insert_opt!("token_savings_tracking", self.token_savings_tracking);
        insert_opt_ref!("advisor_model", self.advisor_model);
        insert_opt_ref!("system_prompt", self.system_prompt);

        out
    }
}

impl From<RuntimeSettings> for SettingsJson {
    fn from(runtime: RuntimeSettings) -> Self {
        Self {
            model: runtime.model.model,
            backend: runtime.core.backend,
            api_provider: runtime.core.api_provider,
            active_auth_profile: runtime.core.active_auth_profile,
            auth_profiles: runtime.core.auth_profiles,
            theme: runtime.core.theme,
            verbose: runtime.core.verbose,
            extra: runtime.core.extra,
            env: runtime.network.env,
            permission_mode: runtime.permissions.permission_mode,
            permissions: runtime.permissions.permissions,
            sandbox: runtime.sandbox.sandbox,
            status_line: runtime.ui.status_line,
            spinner_tips: runtime.ui.spinner_tips,
            output_style: runtime.ui.output_style,
            language: runtime.ui.language,
            voice_enabled: runtime.ui.voice_enabled,
            editor_mode: runtime.ui.editor_mode,
            view_mode: runtime.ui.view_mode,
            terminal_progress_bar_enabled: runtime.ui.terminal_progress_bar_enabled,
            app_icon: runtime.ui.app_icon,
            auto_start: runtime.ui.auto_start,
            start_minimized: runtime.ui.start_minimized,
            minimize_to_tray: runtime.ui.minimize_to_tray,
            close_to_tray: runtime.ui.close_to_tray,
            quick_chat_hide_on_blur: runtime.ui.quick_chat_hide_on_blur,
            quick_chat_inject_screen: runtime.ui.quick_chat_inject_screen,
            quick_chat_ambient: runtime.ui.quick_chat_ambient,
            auto_approve_tools: runtime.ui.auto_approve_tools,
            analytics_enabled: runtime.ui.analytics_enabled,
            thinking: runtime.model.thinking,
            output_config: runtime.model.output_config,
            default_model: runtime.model.default_model,
            fallback_model: runtime.model.fallback_model,
            fast_model: runtime.model.fast_model,
            sota_model: runtime.model.sota_model,
            mota_model: runtime.model.mota_model,
            fota_model: runtime.model.fota_model,
            available_models: runtime.model.available_models,
            model_capabilities: runtime.model.model_capabilities,
            effort_level: runtime.model.effort_level,
            model_reasoning_effort: runtime.model.model_reasoning_effort,
            fast_mode: runtime.model.fast_mode,
            fast_mode_per_session_opt_in: runtime.model.fast_mode_per_session_opt_in,
            context_window: runtime.model.context_window,
            max_messages: runtime.model.max_messages,
            auto_title: runtime.model.auto_title,
            temperature: runtime.model.temperature,
            max_tokens: runtime.model.max_tokens,
            streaming: runtime.model.streaming,
            show_token_usage: runtime.model.show_token_usage,
            show_reasoning_details: runtime.model.show_reasoning_details,
            markdown_rendering: runtime.model.markdown_rendering,
            single_dollar_math: runtime.model.single_dollar_math,
            infographic: runtime.model.infographic,
            auto_collapse_reasoning: runtime.model.auto_collapse_reasoning,
            quick_reply_suggestions: runtime.model.quick_reply_suggestions,
            default_tool_selection: runtime.model.default_tool_selection,
            default_skill_selection: runtime.model.default_skill_selection,
            sound_effects: runtime.model.sound_effects,
            auto_compact: runtime.model.auto_compact,
            compact_threshold: runtime.model.compact_threshold,
            keep_recent_messages: runtime.model.keep_recent_messages,
            hashline_mode: runtime.model.hashline_mode,
            advisor_model: runtime.model.advisor_model,
            system_prompt: runtime.core.system_prompt,
            teammate_mode: runtime.integrations.teammate_mode,
            claude_in_chrome_default_enabled: runtime.integrations.claude_in_chrome_default_enabled,
            auto_memory_enabled: runtime.memory.auto_memory_enabled,
            memory_auto_retrieve: runtime.memory.memory_auto_retrieve,
            memory_query_rewriting: runtime.memory.memory_query_rewriting,
            memory_max_retrieved: runtime.memory.memory_max_retrieved,
            memory_similarity_threshold: runtime.memory.memory_similarity_threshold,
            memory_auto_summarize: runtime.memory.memory_auto_summarize,
            memory_nightly: runtime.memory.memory_nightly,
            memory_sleep_time: runtime.memory.memory_sleep_time,
            memory_temp_ttl: runtime.memory.memory_temp_ttl,
            memory_archive_retention: runtime.memory.memory_archive_retention,
            memory_tool_model: runtime.memory.memory_tool_model,
            memory_embedding_model: runtime.memory.memory_embedding_model,
            proxy_enabled: runtime.network.proxy_enabled,
            proxy_url: runtime.network.proxy_url,
            prefer_ipv4: runtime.network.prefer_ipv4,
            request_timeout: runtime.network.request_timeout,
            retry_attempts: runtime.network.retry_attempts,
            custom_user_agent: runtime.network.custom_user_agent,
            speech_enabled: runtime.speech.speech_enabled,
            speech_active_model: runtime.speech.speech_active_model,
            speech_language: runtime.speech.speech_language,
            tts_provider: runtime.speech.tts_provider,
            tts_api_key: runtime.speech.tts_api_key,
            tts_voice: runtime.speech.tts_voice,
            tts_voice_custom_id: runtime.speech.tts_voice_custom_id,
            tts_model: runtime.speech.tts_model,
            search_engine: runtime.network.search_engine,
            web_search_provider: runtime.network.web_search_provider,
            web_search_tavily_api_key: runtime.network.web_search_tavily_api_key,
            web_search_brave_api_key: runtime.network.web_search_brave_api_key,
            cloud_sync_enabled: runtime.integrations.cloud_sync_enabled,
            cloud_sync_path: runtime.integrations.cloud_sync_path,
            token_savings_tracking: runtime.integrations.token_savings_tracking,
            sources: runtime.sources,
        }
    }
}

fn sanitized_extra_map(extra: &HashMap<String, Value>) -> HashMap<String, Value> {
    extra
        .iter()
        .filter_map(|(key, value)| {
            if is_sensitive_key(key) {
                None
            } else {
                sanitized_extra_value(value).map(|value| (key.clone(), value))
            }
        })
        .collect()
}

fn sanitized_extra_value(value: &Value) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let clean: serde_json::Map<String, Value> = map
                .iter()
                .filter_map(|(key, value)| {
                    if is_sensitive_key(key) {
                        None
                    } else {
                        sanitized_extra_value(value).map(|value| (key.clone(), value))
                    }
                })
                .collect();
            Some(Value::Object(clean))
        }
        Value::Array(items) => Some(Value::Array(
            items.iter().filter_map(sanitized_extra_value).collect(),
        )),
        other => Some(other.clone()),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("apikey")
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_map_includes_phase1_fields_and_redacts_sensitive_values() {
        let settings = SettingsJson {
            language: Some("zh-CN".to_string()),
            proxy_enabled: Some(true),
            show_reasoning_details: Some(true),
            tts_api_key: Some("sk-secret".to_string()),
            web_search_provider: Some("tavily".to_string()),
            web_search_tavily_api_key: Some("tvly-secret".to_string()),
            web_search_brave_api_key: Some("brave-secret".to_string()),
            extra: HashMap::from([
                ("appMode".to_string(), json!("desktop")),
                ("apiToken".to_string(), json!("secret")),
                (
                    "nested".to_string(),
                    json!({
                        "safe": true,
                        "password": "hidden"
                    }),
                ),
            ]),
            ..Default::default()
        };

        let map = settings.settings_map();
        assert_eq!(map.get("language"), Some(&json!("zh-CN")));
        assert_eq!(map.get("proxy_enabled"), Some(&json!(true)));
        assert_eq!(map.get("show_reasoning_details"), Some(&json!(true)));
        assert_eq!(map.get("web_search_provider"), Some(&json!("tavily")));
        assert_eq!(map.get("web_search_tavily_configured"), Some(&json!(true)));
        assert_eq!(map.get("web_search_brave_configured"), Some(&json!(true)));
        assert!(!map.contains_key("tts_api_key"));
        assert!(!map.contains_key("web_search_tavily_api_key"));
        assert!(!map.contains_key("web_search_brave_api_key"));
        assert!(!map.contains_key("apiToken"));
        assert_eq!(map.get("appMode"), Some(&json!("desktop")));
        assert_eq!(map.get("nested"), Some(&json!({ "safe": true })));
    }
}
